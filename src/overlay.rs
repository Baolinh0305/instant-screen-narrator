use winapi::shared::windef::{POINT, RECT, HWND};
use winapi::shared::minwindef::{WPARAM, LPARAM, LRESULT};
use winapi::um::winuser::{GetSystemMetrics, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, CreateWindowExW, RegisterClassW, WNDCLASSW, UnregisterClassW, SetLayeredWindowAttributes, LWA_ALPHA, LoadCursorW, IDC_CROSS, FillRect, FrameRect, InvalidateRect, SetCapture, ReleaseCapture, GetCursorPos, PostMessageW, WM_CLOSE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_LBUTTONUP, WM_PAINT, WM_DESTROY, VK_ESCAPE, BeginPaint, EndPaint, PAINTSTRUCT, GetMessageW, TranslateMessage, DispatchMessageW, MSG, DefWindowProcW, PostQuitMessage, WS_EX_LAYERED, WS_EX_TOPMOST, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE, DrawTextW, DT_CENTER, DT_VCENTER, DT_SINGLELINE, GetClientRect, GetWindowTextLengthW, GetWindowTextW, DT_LEFT, DT_WORDBREAK, SetTimer, KillTimer, WM_TIMER, GetDC, ReleaseDC, DT_CALCRECT};
use winapi::um::wingdi::{CreateCompatibleDC, CreateCompatibleBitmap, SelectObject, DeleteObject, DeleteDC, BitBlt, SRCCOPY, CreateSolidBrush, SetTextColor, TRANSPARENT, CreateFontW, SetBkMode};
use winapi::um::libloaderapi::GetModuleHandleW;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use crate::config;

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

static mut START_POS: POINT = POINT { x: 0, y: 0 };
static mut CURR_POS: POINT = POINT { x: 0, y: 0 };
static mut IS_DRAGGING: bool = false;

pub fn show_selection_overlay() {
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = to_wide("SnippingOverlay");

        let mut wc: WNDCLASSW = unsafe { std::mem::zeroed() };
        wc.lpfnWndProc = Some(selection_wnd_proc);
        wc.hInstance = instance;
        wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_CROSS);
        wc.lpszClassName = class_name.as_ptr();
        wc.hbrBackground = CreateSolidBrush(0x00000000);
        RegisterClassW(&wc);

        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            to_wide("Snipping").as_ptr(),
            WS_POPUP | WS_VISIBLE,
            x, y, w, h,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut()
        );

        SetLayeredWindowAttributes(hwnd, 0, 100, LWA_ALPHA);

        let mut msg: MSG = unsafe { std::mem::zeroed() };
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if msg.message == WM_CLOSE { break; }
        }

        UnregisterClassW(class_name.as_ptr(), instance);
    }
}

unsafe extern "system" fn selection_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_KEYDOWN => {
            if wparam == VK_ESCAPE as usize {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
            0
        }
        WM_TIMER => {
            KillTimer(hwnd, 1);
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
            0
        }
        WM_LBUTTONDOWN => {
            IS_DRAGGING = true;
            GetCursorPos(std::ptr::addr_of_mut!(START_POS));
            CURR_POS = START_POS;
            SetCapture(hwnd);
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_MOUSEMOVE => {
            if IS_DRAGGING {
                GetCursorPos(std::ptr::addr_of_mut!(CURR_POS));
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_LBUTTONUP => {
            if IS_DRAGGING {
                IS_DRAGGING = false;
                ReleaseCapture();

                let rect = RECT {
                    left: START_POS.x.min(CURR_POS.x),
                    top: START_POS.y.min(CURR_POS.y),
                    right: START_POS.x.max(CURR_POS.x),
                    bottom: START_POS.y.max(CURR_POS.y),
                };

                if (rect.right - rect.left) > 10 && (rect.bottom - rect.top) > 10 {
                    let region = config::Region {
                        x: rect.left,
                        y: rect.top,
                        width: (rect.right - rect.left) as u32,
                        height: (rect.bottom - rect.top) as u32,
                    };
                    let hwnd_usize = hwnd as usize;
                    std::thread::spawn(move || {
                        process_region(region);
                        unsafe { PostMessageW(hwnd_usize as HWND, WM_CLOSE, 0, 0); }
                    });
                } else {
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                }
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
            let hdc = BeginPaint(hwnd, &mut ps);

            let mem_dc = CreateCompatibleDC(hdc);
            let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            let mem_bitmap = CreateCompatibleBitmap(hdc, width, height);
            SelectObject(mem_dc, mem_bitmap as *mut winapi::ctypes::c_void);

            let brush = CreateSolidBrush(0x00000000);
            let full_rect = RECT { left: 0, top: 0, right: width, bottom: height };
            FillRect(mem_dc, &full_rect, brush);
            DeleteObject(brush as *mut winapi::ctypes::c_void);

            if IS_DRAGGING {
                let rect = RECT {
                    left: (START_POS.x.min(CURR_POS.x)) - GetSystemMetrics(SM_XVIRTUALSCREEN),
                    top: (START_POS.y.min(CURR_POS.y)) - GetSystemMetrics(SM_YVIRTUALSCREEN),
                    right: (START_POS.x.max(CURR_POS.x)) - GetSystemMetrics(SM_XVIRTUALSCREEN),
                    bottom: (START_POS.y.max(CURR_POS.y)) - GetSystemMetrics(SM_YVIRTUALSCREEN),
                };

                let frame_brush = CreateSolidBrush(0x00FFFFFF);
                FrameRect(mem_dc, &rect, frame_brush);
                DeleteObject(frame_brush as *mut winapi::ctypes::c_void);
            }

            BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);
            DeleteObject(mem_bitmap as *mut winapi::ctypes::c_void);
            DeleteDC(mem_dc);
            EndPaint(hwnd, &mut ps);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn process_region(region: config::Region) {
    let mut config = config::Config::load();
    config.fixed_regions.clear();
    config.fixed_regions.push(region);
    config.save().unwrap();
}

pub fn show_result_window(target_rect: RECT, text: String, duration_ms: u32) {
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = to_wide("TranslationResult");

        let mut wc: WNDCLASSW = unsafe { std::mem::zeroed() };
        wc.lpfnWndProc = Some(result_wnd_proc);
        wc.hInstance = instance;
        wc.lpszClassName = class_name.as_ptr();
        wc.hbrBackground = CreateSolidBrush(0x00000000);
        RegisterClassW(&wc);

        // Calculate text size
        let hdc_screen = GetDC(std::ptr::null_mut());
        let font_size = 20;
        let hfont = CreateFontW(font_size, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 2, 0, to_wide("Segoe UI").as_ptr());
        SelectObject(hdc_screen, hfont as *mut winapi::ctypes::c_void);
        let mut text_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let wide_text = to_wide(&text);
        DrawTextW(hdc_screen, wide_text.as_ptr(), -1, &mut text_rect, DT_CALCRECT | DT_WORDBREAK);
        let text_width = text_rect.right - text_rect.left;
        let text_height = text_rect.bottom - text_rect.top;
        DeleteObject(hfont as *mut winapi::ctypes::c_void);
        ReleaseDC(std::ptr::null_mut(), hdc_screen);

        let padding = 8;
        let width = text_width + padding * 2;
        let height = text_height + padding * 2;

        let region_width = (target_rect.right - target_rect.left).abs();
        let region_center_x = target_rect.left + region_width / 2;
        let x = region_center_x - width / 2;
        let mut y = target_rect.top - height - 10; // A little above
        if y < 0 {
            y = target_rect.top + 10; // Below if above is off-screen
        }

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            to_wide(&text).as_ptr(),
            WS_POPUP | WS_VISIBLE,
            x, y, width, height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut()
        );

        SetLayeredWindowAttributes(hwnd, 0, 220, LWA_ALPHA);
        SetTimer(hwnd, 1, duration_ms, None);

        let mut msg: MSG = unsafe { std::mem::zeroed() };
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if msg.message == WM_CLOSE { break; }
        }

        UnregisterClassW(class_name.as_ptr(), instance);
    }
}

unsafe extern "system" fn result_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_LBUTTONUP => {
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
            0
        }
        WM_KEYDOWN => {
            if wparam == VK_ESCAPE as usize {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
            0
        }
        WM_MOUSEMOVE => 0,
        WM_TIMER => {
            KillTimer(hwnd, 1);
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rect: RECT = unsafe { std::mem::zeroed() };
            GetClientRect(hwnd, &mut rect);

            // Paint black background
            let black_brush = CreateSolidBrush(0x00000000);
            FillRect(hdc, &rect, black_brush);
            DeleteObject(black_brush as *mut winapi::ctypes::c_void);

            SetBkMode(hdc, 1);
            SetTextColor(hdc, 0x00FFFFFF);

            let text_len = GetWindowTextLengthW(hwnd) + 1;
            let mut buf = vec![0u16; text_len as usize];
            GetWindowTextW(hwnd, buf.as_mut_ptr(), text_len);

            // Simple font size calculation
            let font_size = 20; // Fixed size for simplicity
            let hfont = CreateFontW(font_size, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 2, 0, to_wide("Segoe UI").as_ptr());
            SelectObject(hdc, hfont as *mut winapi::ctypes::c_void);

            DrawTextW(hdc, buf.as_mut_ptr(), -1, &mut rect, DT_CENTER | DT_WORDBREAK | DT_VCENTER);

            DeleteObject(hfont as *mut winapi::ctypes::c_void);
            EndPaint(hwnd, &mut ps);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}