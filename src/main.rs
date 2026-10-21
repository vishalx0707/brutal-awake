// The UI thread owns a continuous Windows execution request. No timer,
// power-plan writes, browser runtime, async runtime, or background workers.
#![windows_subsystem = "windows"]

use std::{mem::zeroed, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::DwmSetWindowAttribute, Gdi::*},
    System::{LibraryLoader::GetModuleHandleW, Power::*, Threading::CreateMutexW},
    UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

const CLASS: &str = "BrutalAwakeNative";
const TOGGLE: usize = 100;
const MINIMIZE: usize = 101;
const CLOSE: usize = 102;
const PAPER: u32 = 0x00f2f6f7;
const INK: u32 = 0x00111111;
const MUTED: u32 = 0x00838a8c;
const TRACK_OFF: u32 = 0x00c9d1d3;

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

struct App {
    on: bool,
    toggle: HWND,
    minimize: HWND,
    close: HWND,
    scale: f32,
    label_font: HFONT,
    status_font: HFONT,
    controls_font: HFONT,
}

impl App {
    unsafe fn fonts(&mut self, dpi: u32) {
        for f in [self.label_font, self.status_font, self.controls_font] {
            if !f.is_null() { DeleteObject(f); }
        }
        self.scale = dpi as f32 / 96.0;
        self.label_font = self.font(11, 600);
        self.status_font = self.font(46, 600);
        self.controls_font = self.font(16, 400);
    }

    unsafe fn font(&self, size: i32, weight: i32) -> HFONT {
        CreateFontW(-self.px(size), 0, 0, 0, weight, 0, 0, 0,
            DEFAULT_CHARSET as u32, OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32, CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32, wide("Segoe UI").as_ptr())
    }

    fn px(&self, n: i32) -> i32 { (n as f32 * self.scale).round() as i32 }

    unsafe fn layout(&self, hwnd: HWND) {
        let mut r = zeroed();
        GetClientRect(hwnd, &mut r);
        let center = (r.bottom + self.px(6)) / 2;
        MoveWindow(self.toggle, (r.right-self.px(82))/2, center+self.px(64), self.px(82), self.px(50), 1);
        MoveWindow(self.minimize, r.right-self.px(74), self.px(6), self.px(30), self.px(26), 1);
        MoveWindow(self.close, r.right-self.px(42), self.px(6), self.px(30), self.px(26), 1);
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }

    unsafe fn text(&self, dc: HDC, text: &str, font: HFONT, color: u32, mut r: RECT) {
        let old = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
        DrawTextW(dc, wide(text).as_ptr(), -1, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(dc, old);
    }

    unsafe fn paint(&self, hwnd: HWND) {
        let mut ps = zeroed();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut r: RECT = zeroed();
        GetClientRect(hwnd, &mut r);
        fill(dc, &r, PAPER);
        let center = (r.bottom+self.px(6))/2;
        let label = RECT { left: 0, top: center-self.px(95), right: r.right, bottom: center-self.px(79) };
        let old_spacing = SetTextCharacterExtra(dc, self.px(2));
        self.text(dc, "BRUTAL AWAKE", self.label_font, MUTED, label);
        SetTextCharacterExtra(dc, old_spacing);
        fill(dc, &RECT { left: (r.right-self.px(34))/2, top: center-self.px(59), right: (r.right+self.px(34))/2, bottom: center-self.px(58) }, 0x00d4dcde);
        self.text(dc, if self.on { "AWAKE" } else { "ASLEEP" }, self.status_font,
            if self.on { INK } else { MUTED }, RECT { left: 0, top: center-self.px(34), right: r.right, bottom: center+self.px(24) });
        EndPaint(hwnd, &ps);
    }

    unsafe fn draw_button(&self, draw: &DRAWITEMSTRUCT) {
        let dc = draw.hDC;
        let r = draw.rcItem;
        fill(dc, &r, PAPER);
        if draw.CtlID as usize == TOGGLE {
            let pad = self.px(8);
            pill(dc, pad, pad, r.right-pad, r.bottom-pad, self.px(34), if self.on { INK } else { TRACK_OFF });
            let x = pad + self.px(if self.on { 36 } else { 4 });
            pill(dc, x, pad+self.px(4), x+self.px(26), pad+self.px(30), self.px(26), 0x00ffffff);
            if draw.itemState & ODS_FOCUS != 0 && draw.itemState & ODS_NOFOCUSRECT == 0 {
                let focus = RECT { left: 1, top: 1, right: r.right-1, bottom: r.bottom-1 };
                DrawFocusRect(dc, &focus);
            }
        } else {
            if draw.itemState & ODS_SELECTED != 0 { fill(dc, &r, 0x00dedcda); }
            self.text(dc, if draw.CtlID as usize == MINIMIZE { "−" } else { "×" }, self.controls_font, MUTED, r);
            if draw.itemState & ODS_FOCUS != 0 { DrawFocusRect(dc, &r); }
        }
    }
}

unsafe fn set_awake(app: *mut App, hwnd: HWND, on: bool) {
    let flags = ES_CONTINUOUS | if on { ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED } else { 0 };
    if SetThreadExecutionState(flags) == 0 {
        MessageBoxW(hwnd, wide("Windows could not change the keep-awake request. The switch has not changed.").as_ptr(), wide("Brutal Awake").as_ptr(), MB_OK | MB_ICONERROR);
        return;
    }
    (*app).on = on;
    // No Rust reference is held across calls that can dispatch window messages.
    SetWindowTextW((*app).toggle, wide(if on { "Keep awake: on" } else { "Keep awake: off" }).as_ptr());
    InvalidateRect(hwnd, std::ptr::null(), 0);
    InvalidateRect((*app).toggle, std::ptr::null(), 0);
}

unsafe fn fill(dc: HDC, r: &RECT, color: u32) {
    let brush = CreateSolidBrush(color);
    FillRect(dc, r, brush);
    DeleteObject(brush);
}

unsafe fn pill(dc: HDC, left: i32, top: i32, right: i32, bottom: i32, radius: i32, color: u32) {
    let brush = CreateSolidBrush(color);
    let old_brush = SelectObject(dc, brush);
    let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
    RoundRect(dc, left, top, right, bottom, radius, radius);
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(brush);
}

unsafe fn button(parent: HWND, id: usize, name: &str) -> HWND {
    CreateWindowExW(0, wide("BUTTON").as_ptr(), wide(name).as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32,
        0, 0, 0, 0, parent, id as HMENU, GetModuleHandleW(std::ptr::null()), std::ptr::null())
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(lp as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if ptr.is_null() { return DefWindowProcW(hwnd, msg, wp, lp); }
    // Win32 calls can re-enter this procedure: never hold a mutable reference
    // across calls that send messages (MoveWindow, SetFocus, MessageBox, etc.).
    match msg {
        WM_CREATE => {
            (*ptr).fonts(GetDpiForWindow(hwnd));
            (*ptr).toggle = button(hwnd, TOGGLE, "Keep awake: off");
            (*ptr).minimize = button(hwnd, MINIMIZE, "Minimize");
            (*ptr).close = button(hwnd, CLOSE, "Close");
            if (*ptr).toggle.is_null() || (*ptr).minimize.is_null() || (*ptr).close.is_null() { return -1; }
            (*ptr).layout(hwnd);
            0
        }
        WM_COMMAND if (wp >> 16) == BN_CLICKED as usize => {
            match wp & 0xffff {
                TOGGLE => set_awake(ptr, hwnd, !(*ptr).on),
                MINIMIZE => { ShowWindow(hwnd, SW_MINIMIZE); },
                CLOSE => { SendMessageW(hwnd, WM_CLOSE, 0, 0); },
                _ => (),
            }
            0
        }
        WM_PAINT => { (*ptr).paint(hwnd); 0 }
        WM_DRAWITEM => { (*ptr).draw_button(&*(lp as *const DRAWITEMSTRUCT)); 1 }
        WM_ERASEBKGND => 1,
        // Let our paper surface fill the frame. DWM still supplies the shadow
        // and rounded corners; hit testing below keeps native edge resizing.
        WM_NCCALCSIZE if wp != 0 => 0,
        WM_SIZE => { (*ptr).layout(hwnd); 0 }
        WM_SETFOCUS => { SetFocus((*ptr).toggle); 0 }
        WM_DPICHANGED => {
            (*ptr).fonts((wp & 0xffff) as u32);
            let r = &*(lp as *const RECT);
            SetWindowPos(hwnd, null_mut(), r.left, r.top, r.right-r.left, r.bottom-r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            (*ptr).layout(hwnd);
            0
        }
        WM_NCHITTEST => {
            let result = DefWindowProcW(hwnd, msg, wp, lp);
            if result == HTCLIENT as isize {
                let mut point = POINT { x: lp as i16 as i32, y: (lp >> 16) as i16 as i32 };
                ScreenToClient(hwnd, &mut point);
                let mut r = zeroed();
                GetClientRect(hwnd, &mut r);
                let edge = (*ptr).px(6);
                let left = point.x < edge;
                let right = point.x >= r.right-edge;
                let top = point.y < edge;
                let bottom = point.y >= r.bottom-edge;
                if top && left { return HTTOPLEFT as isize; }
                if top && right { return HTTOPRIGHT as isize; }
                if bottom && left { return HTBOTTOMLEFT as isize; }
                if bottom && right { return HTBOTTOMRIGHT as isize; }
                if left { return HTLEFT as isize; }
                if right { return HTRIGHT as isize; }
                if top { return HTTOP as isize; }
                if bottom { return HTBOTTOM as isize; }
                if point.y < (*ptr).px(40) && point.x < r.right-(*ptr).px(80) { return HTCAPTION as isize; }
            }
            result
        }
        WM_GETMINMAXINFO => {
            let info = &mut *(lp as *mut MINMAXINFO);
            info.ptMinTrackSize = POINT { x: (*ptr).px(320), y: (*ptr).px(440) };
            0
        }
        WM_CLOSE => {
            SetThreadExecutionState(ES_CONTINUOUS);
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => { SetThreadExecutionState(ES_CONTINUOUS); PostQuitMessage(0); 0 }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn main() {
    unsafe {
        // Use v1's mutex name, preventing an installed old copy running alongside.
        let instance = CreateMutexW(std::ptr::null(), 0, wide("Local\\BrutalAwakeSingleInstance").as_ptr());
        if instance.is_null() { return; }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let hwnd = FindWindowW(std::ptr::null(), wide("Brutal Awake").as_ptr());
            if !hwnd.is_null() { ShowWindow(hwnd, SW_RESTORE); SetForegroundWindow(hwnd); }
            CloseHandle(instance);
            return;
        }
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let module = GetModuleHandleW(std::ptr::null());
        let class_name = wide(CLASS);
        let mut wc: WNDCLASSW = zeroed();
        wc.lpfnWndProc = Some(window_proc);
        wc.hInstance = module;
        wc.lpszClassName = class_name.as_ptr();
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
        if RegisterClassW(&wc) == 0 { CloseHandle(instance); return; }
        let mut app = Box::new(App { on: false, toggle: null_mut(), minimize: null_mut(), close: null_mut(), scale: 1.0, label_font: null_mut(), status_font: null_mut(), controls_font: null_mut() });
        let scale = GetDpiForSystem() as f32 / 96.0;
        let width = (360.0*scale) as i32;
        let height = (480.0*scale) as i32;
        let hwnd = CreateWindowExW(WS_EX_APPWINDOW, class_name.as_ptr(), wide("Brutal Awake").as_ptr(),
            WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_SYSMENU | WS_CLIPCHILDREN,
            (GetSystemMetrics(SM_CXSCREEN)-width)/2, (GetSystemMetrics(SM_CYSCREEN)-height)/2,
            width, height, null_mut(), null_mut(), module, (&mut *app as *mut App).cast());
        if !hwnd.is_null() {
            let round: u32 = 2;
            DwmSetWindowAttribute(hwnd, 33, (&round as *const u32).cast(), 4);
            // Preserve awake-on-launch; show ON only when the request succeeds.
            set_awake(&mut *app, hwnd, true);
            SendMessageW(hwnd, WM_CHANGEUISTATE, UIS_SET as usize | (((UISF_HIDEFOCUS | UISF_HIDEACCEL) as usize) << 16), 0);
            ShowWindow(hwnd, SW_SHOW);
            let mut msg: MSG = zeroed();
            loop {
                let result = GetMessageW(&mut msg, null_mut(), 0, 0);
                if result <= 0 { break; }
                if msg.message == WM_KEYDOWN && msg.wParam == VK_ESCAPE as usize {
                    SendMessageW(hwnd, WM_CLOSE, 0, 0);
                } else if IsDialogMessageW(hwnd, &msg) == 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        SetThreadExecutionState(ES_CONTINUOUS);
        for font in [app.label_font, app.status_font, app.controls_font] {
            if !font.is_null() { DeleteObject(font); }
        }
        CloseHandle(instance);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_sets_and_clears_the_owning_threads_execution_flags() {
        unsafe {
            // Invisible test window: same toggle path, no power-plan writes.
            let hwnd = CreateWindowExW(0, wide("STATIC").as_ptr(), wide("Awake test").as_ptr(),
                WS_POPUP, 0, 0, 10, 10, null_mut(), null_mut(),
                GetModuleHandleW(std::ptr::null()), std::ptr::null());
            assert!(!hwnd.is_null());
            struct Cleanup(HWND);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    unsafe { SetThreadExecutionState(ES_CONTINUOUS); DestroyWindow(self.0); }
                }
            }
            let _cleanup = Cleanup(hwnd);
            let mut app = App { on: false, toggle: button(hwnd, TOGGLE, "Keep awake: off"),
                minimize: null_mut(), close: null_mut(), scale: 1.0,
                label_font: null_mut(), status_font: null_mut(), controls_font: null_mut() };
            assert!(!app.toggle.is_null());
            let required = ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED;
            for _ in 0..10 {
                set_awake(&mut app, hwnd, true);
                assert!(app.on);
                // The API returns the calling thread's previous flags.
                let previous = SetThreadExecutionState(ES_CONTINUOUS | required);
                assert_eq!(previous & required, required);
                assert_eq!(previous & ES_AWAYMODE_REQUIRED, 0);
                set_awake(&mut app, hwnd, false);
                assert!(!app.on);
                let previous = SetThreadExecutionState(ES_CONTINUOUS);
                assert_ne!(previous, 0);
                assert_eq!(previous & required, 0);
            }
        }
    }
}
