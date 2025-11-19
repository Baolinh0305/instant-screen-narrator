use winapi::shared::windef::{POINT, RECT, HWND};
use winapi::shared::minwindef::{WPARAM, LPARAM, LRESULT};
use winapi::um::winuser::{GetSystemMetrics, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, CreateWindowExW, RegisterClassW, WNDCLASSW, UnregisterClassW, SetLayeredWindowAttributes, LWA_ALPHA, LoadCursorW, IDC_CROSS, FillRect, FrameRect, InvalidateRect, SetCapture, ReleaseCapture, GetCursorPos, PostMessageW, WM_CLOSE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_LBUTTONUP, WM_PAINT, WM_DESTROY, VK_ESCAPE, BeginPaint, EndPaint, PAINTSTRUCT, GetMessageW, TranslateMessage, DispatchMessageW, MSG, DefWindowProcW, PostQuitMessage, WS_EX_LAYERED, WS_EX_TOPMOST, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE, DrawTextW, DT_CENTER, DT_VCENTER, DT_SINGLELINE};
use winapi::um::wingdi::{CreateCompatibleDC, CreateCompatibleBitmap, SelectObject, DeleteObject, DeleteDC, BitBlt, SRCCOPY, CreateSolidBrush, SetTextColor};
use winapi::um::libloaderapi::GetModuleHandleW;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use tokio::runtime::Runtime;

use crate::config;
use crate::translation;
use crate::tts;
use crate::capture;

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

static mut START_POS: POINT = POINT { x: 0, y: 0 };
static mut CURR_POS: POINT = POINT { x: 0, y: 0 };
static mut IS_DRAGGING: bool = false;
static mut MODE: bool = false;

pub fn show_selection_overlay() {
    unsafe {
        MODE = std::fs::metadata("overlay.txt").is_ok();
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
        WM_LBUTTONDOWN => {
            if MODE {
                let _ = std::fs::remove_file("overlay.txt");
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
                0
            } else {
                IS_DRAGGING = true;
                GetCursorPos(std::ptr::addr_of_mut!(START_POS));
                CURR_POS = START_POS;
                SetCapture(hwnd);
                InvalidateRect(hwnd, std::ptr::null(), 0);
                0
            }
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

            if MODE {
                if let Ok(text) = std::fs::read_to_string("overlay.txt") {
                    let text_wide = to_wide(&text);
                    let mut rect = RECT { left: 100, top: 100, right: 800, bottom: 300 };
                    let bg_brush = CreateSolidBrush(0x00FFFFFF);
                    FillRect(mem_dc, &rect, bg_brush);
                    DeleteObject(bg_brush as *mut winapi::ctypes::c_void);
                    SetTextColor(mem_dc, 0x00000000);
                    DrawTextW(mem_dc, text_wide.as_ptr(), -1, &mut rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                }
            } else if IS_DRAGGING {
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