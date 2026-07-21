// Brutal Awake — Tauri backend.
// Keeps the machine awake by three overlapping mechanisms, because no single
// one is enough on modern hardware:
//   1. SetThreadExecutionState — the classic "system/display required" hint.
//   2. Sleep + hibernate idle timeouts forced to 0 (never) on the active power
//      plan. THIS is what actually keeps Modern Standby (S0) laptops awake,
//      where SetThreadExecutionState's system-required flag is ignored.
//   3. Lid-close action forced to "Do Nothing" so a closed lid can't sleep it.
// Every power-plan value is saved before it is changed and restored on OFF or
// exit. Runs as a normal user — no elevation required.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

// Power-setting subgroup + setting GUIDs (stable across Windows installs).
const SUB_BUTTONS: &str = "4f971e89-eebd-4455-a8de-9e59040e7347";
const LIDACTION: &str = "5ca83367-6e45-459f-a27b-476b1d01c936";
const SUB_SLEEP: &str = "238c9fa8-0aad-41ed-83f4-97be242c8f20";
const STANDBYIDLE: &str = "29f6c1db-86da-48c5-9fdb-f2b67b1f44da"; // "Sleep after"
const HIBERNATEIDLE: &str = "9d7815a6-7ee4-497e-8888-515a05f02364"; // "Hibernate after"

// Original values of every power setting we override, so we can put them back.
#[derive(Default)]
struct Saved {
    lid_ac: Option<u32>,
    lid_dc: Option<u32>,
    sleep_ac: Option<u32>,
    sleep_dc: Option<u32>,
    hib_ac: Option<u32>,
    hib_dc: Option<u32>,
}

struct Shared {
    on: Mutex<bool>,
    cv: Condvar,
    saved: Mutex<Saved>,
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

fn run_powercfg(args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("powercfg");
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.output() {
        Ok(output) if output.status.success() => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!(
                "powercfg {:?} failed (code: {:?}): {}",
                args,
                output.status.code(),
                stderr.trim()
            ))
        }
        Err(err) => Err(format!("failed to execute powercfg {:?}: {}", args, err)),
    }
}

fn powercfg(args: &[&str]) -> String {
    match run_powercfg(args) {
        Ok(out) => out,
        Err(err) => {
            eprintln!("{err}");
            String::new()
        }
    }
}

fn parse_first_hex(out: &str) -> Option<u32> {
    for token in out.split_whitespace() {
        if let Some(hex) = token.strip_prefix("0x") {
            if let Ok(value) = u32::from_str_radix(hex.trim(), 16) {
                return Some(value);
            }
        }
    }
    None
}

// Read the current AC and DC index for one power setting.
fn query_setting(sub: &str, setting: &str) -> (Option<u32>, Option<u32>) {
    let ac_out = powercfg(&["/getacvalueindex", "SCHEME_CURRENT", sub, setting]);
    let dc_out = powercfg(&["/getdcvalueindex", "SCHEME_CURRENT", sub, setting]);
    (
        parse_first_hex(&ac_out),
        parse_first_hex(&dc_out),
    )
}

// Set one power setting's AC and DC index (no /setactive — caller batches it).
fn set_setting(sub: &str, setting: &str, value: u32) {
    let v = value.to_string();
    powercfg(&["/setacvalueindex", "SCHEME_CURRENT", sub, setting, &v]);
    powercfg(&["/setdcvalueindex", "SCHEME_CURRENT", sub, setting, &v]);
}

// Restore one setting's AC/DC index if we recorded originals for it.
fn restore_setting(sub: &str, setting: &str, ac: Option<u32>, dc: Option<u32>) {
    if let Some(v) = ac {
        powercfg(&["/setacvalueindex", "SCHEME_CURRENT", sub, setting, &v.to_string()]);
    }
    if let Some(v) = dc {
        powercfg(&["/setdcvalueindex", "SCHEME_CURRENT", sub, setting, &v.to_string()]);
    }
}

// Save every value we are about to override (only on the first transition to
// ON — guarded by the caller so we never re-save our own forced values), then
// force lid=Do Nothing and sleep/hibernate timeouts to 0 (never).
fn force_awake(saved: &mut Saved) {
    if saved.lid_ac.is_none() && saved.lid_dc.is_none() {
        let (ac, dc) = query_setting(SUB_BUTTONS, LIDACTION);
        saved.lid_ac = ac;
        saved.lid_dc = dc;
        let (sac, sdc) = query_setting(SUB_SLEEP, STANDBYIDLE);
        saved.sleep_ac = sac;
        saved.sleep_dc = sdc;
        let (hac, hdc) = query_setting(SUB_SLEEP, HIBERNATEIDLE);
        saved.hib_ac = hac;
        saved.hib_dc = hdc;
    }
    set_setting(SUB_BUTTONS, LIDACTION, 0); // 0 = Do nothing on lid close
    set_setting(SUB_SLEEP, STANDBYIDLE, 0); // 0 = never sleep
    set_setting(SUB_SLEEP, HIBERNATEIDLE, 0); // 0 = never hibernate
    powercfg(&["/setactive", "SCHEME_CURRENT"]);
}

// Put every overridden power setting back to what the user had.
fn restore_all(saved: &Saved) {
    restore_setting(SUB_BUTTONS, LIDACTION, saved.lid_ac, saved.lid_dc);
    restore_setting(SUB_SLEEP, STANDBYIDLE, saved.sleep_ac, saved.sleep_dc);
    restore_setting(SUB_SLEEP, HIBERNATEIDLE, saved.hib_ac, saved.hib_dc);
    powercfg(&["/setactive", "SCHEME_CURRENT"]);
}

// --- Commands -----------------------------------------------------------

#[tauri::command]
fn set_awake(on: bool, shared: State<Arc<Shared>>) {
    {
        let mut saved = shared.saved.lock().unwrap();
        if on {
            force_awake(&mut saved);
        } else {
            restore_all(&saved);
            *saved = Saved::default();
        }
    }
    *shared.on.lock().unwrap() = on;
    shared.cv.notify_all();
}

#[tauri::command]
fn quit(app: AppHandle, shared: State<Arc<Shared>>) {
    let on = *shared.on.lock().unwrap();
    if on {
        let saved = shared.saved.lock().unwrap();
        restore_all(&saved);
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
        saved: Mutex::new(Saved::default()),
    });

    // Save the user's current power settings and force the machine awake from
    // the moment it launches: never sleep, never hibernate, ignore lid close.
    {
        let mut saved = shared.saved.lock().unwrap();
        force_awake(&mut saved);
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
                    let saved = shared.saved.lock().unwrap();
                    restore_all(&saved);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Brutal Awake");
}
