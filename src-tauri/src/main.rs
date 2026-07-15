// Brutal Awake — Tauri backend.
// Keeps the machine awake (SetThreadExecutionState) and, while ON, sets the
// lid-close action to "Do Nothing" via powercfg, restoring the original value
// on OFF or exit. Runs as a normal user — no elevation required.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

const SUB_BUTTONS: &str = "4f971e89-eebd-4455-a8de-9e59040e7347";
const LIDACTION: &str = "5ca83367-6e45-459f-a27b-476b1d01c936";

#[derive(Default)]
struct Lid {
    ac: Option<u32>,
    dc: Option<u32>,
}

struct Shared {
    on: Mutex<bool>,
    cv: Condvar,
    lid: Mutex<Lid>,
}

// --- Keep-awake (SetThreadExecutionState) -------------------------------

#[cfg(windows)]
fn apply_execution_state(on: bool) {
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_AWAYMODE_REQUIRED, ES_CONTINUOUS, ES_DISPLAY_REQUIRED,
        ES_SYSTEM_REQUIRED,
    };
    let flags = if on {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED | ES_AWAYMODE_REQUIRED
    } else {
        ES_CONTINUOUS
    };
    unsafe {
        SetThreadExecutionState(flags);
    }
}

#[cfg(not(windows))]
fn apply_execution_state(_on: bool) {}

// --- Single-instance guard (named mutex) ---------------------------------
//
// A second launch must exit before touching any power settings: otherwise it
// would observe the already-forced lid value 0 and save it as the "original",
// clobbering the real value the first instance will restore on exit.

#[cfg(windows)]
fn another_instance_running() -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    // UTF-16, NUL-terminated. Per-session Local\ namespace is enough.
    let name: Vec<u16> = "Local\\BrutalAwakeSingleInstance\0".encode_utf16().collect();
    unsafe {
        // Handle is intentionally leaked — the OS holds the mutex for the
        // lifetime of the process and releases it on exit (however it exits).
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        handle.is_null() || GetLastError() == ERROR_ALREADY_EXISTS
    }
}

#[cfg(not(windows))]
fn another_instance_running() -> bool {
    false
}

// --- Lid-close action (powercfg) ----------------------------------------

fn powercfg(args: &[&str]) -> String {
    let mut cmd = Command::new("powercfg");
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn parse_hex(out: &str, key: &str) -> Option<u32> {
    out.lines()
        .find(|l| l.contains(key))
        .and_then(|l| l.split("0x").nth(1))
        .and_then(|h| u32::from_str_radix(h.trim(), 16).ok())
}

fn query_lid() -> (Option<u32>, Option<u32>) {
    let out = powercfg(&["/query", "SCHEME_CURRENT", SUB_BUTTONS, LIDACTION]);
    (
        parse_hex(&out, "Current AC Power Setting Index:"),
        parse_hex(&out, "Current DC Power Setting Index:"),
    )
}

fn set_lid(value: u32) {
    let v = value.to_string();
    powercfg(&["/setacvalueindex", "SCHEME_CURRENT", SUB_BUTTONS, LIDACTION, &v]);
    powercfg(&["/setdcvalueindex", "SCHEME_CURRENT", SUB_BUTTONS, LIDACTION, &v]);
    powercfg(&["/setactive", "SCHEME_CURRENT"]);
}

fn restore_lid(lid: &Lid) {
    if let Some(ac) = lid.ac {
        powercfg(&["/setacvalueindex", "SCHEME_CURRENT", SUB_BUTTONS, LIDACTION, &ac.to_string()]);
    }
    if let Some(dc) = lid.dc {
        powercfg(&["/setdcvalueindex", "SCHEME_CURRENT", SUB_BUTTONS, LIDACTION, &dc.to_string()]);
    }
    powercfg(&["/setactive", "SCHEME_CURRENT"]);
}

// --- Commands -----------------------------------------------------------

#[tauri::command]
fn set_awake(on: bool, shared: State<Arc<Shared>>) {
    {
        let mut lid = shared.lid.lock().unwrap();
        if on {
            if lid.ac.is_none() && lid.dc.is_none() {
                let (ac, dc) = query_lid();
                lid.ac = ac;
                lid.dc = dc;
            }
            set_lid(0); // 0 = Do nothing on lid close
        } else {
            restore_lid(&lid);
            *lid = Lid::default();
        }
    }
    *shared.on.lock().unwrap() = on;
    shared.cv.notify_all();
}

#[tauri::command]
fn quit(app: AppHandle, shared: State<Arc<Shared>>) {
    let on = *shared.on.lock().unwrap();
    if on {
        let lid = shared.lid.lock().unwrap();
        restore_lid(&lid);
    }
    app.exit(0);
}

fn main() {
    // Single instance only — and this check MUST come before the lid-value
    // save below, so a second launch can never record the forced value 0 as
    // the user's "original" setting or disturb the running instance.
    if another_instance_running() {
        return;
    }

    // Start awake by default: the user launches the app and the laptop stays
    // awake until they toggle off or shut down.
    let shared = Arc::new(Shared {
        on: Mutex::new(true),
        cv: Condvar::new(),
        lid: Mutex::new(Lid::default()),
    });

    // Save the user's current lid-close action and force "Do nothing" so the
    // machine survives a closed lid from the moment it launches.
    {
        let mut lid = shared.lid.lock().unwrap();
        let (ac, dc) = query_lid();
        lid.ac = ac;
        lid.dc = dc;
        set_lid(0); // 0 = Do nothing on lid close
    }

    // Dedicated thread owns the execution-state assertion and re-asserts it
    // periodically so it survives regardless of the async runtime's threads.
    // With `on` true at startup it asserts immediately on its first iteration.
    {
        let keeper = shared.clone();
        std::thread::spawn(move || loop {
            let on = *keeper.on.lock().unwrap();
            apply_execution_state(on);
            let guard = keeper.on.lock().unwrap();
            let _ = keeper.cv.wait_timeout(guard, Duration::from_secs(25));
        });
    }

    tauri::Builder::default()
        .manage(shared.clone())
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            // Rounded Windows 11 corners for the frameless window.
            #[cfg(target_os = "windows")]
            if let Ok(hwnd) = window.hwnd() {
                use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
                const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
                const DWMWCP_ROUND: u32 = 2;
                unsafe {
                    DwmSetWindowAttribute(
                        hwnd.0 as _,
                        DWMWA_WINDOW_CORNER_PREFERENCE,
                        &DWMWCP_ROUND as *const u32 as *const _,
                        std::mem::size_of::<u32>() as u32,
                    );
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![set_awake, quit])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let shared = window.state::<Arc<Shared>>();
                let on = *shared.on.lock().unwrap();
                if on {
                    let lid = shared.lid.lock().unwrap();
                    restore_lid(&lid);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Brutal Awake");
}
