use winapi::shared::windef::{POINT, RECT, HWND};
use winapi::shared::minwindef::{WPARAM, LPARAM, LRESULT};
use winapi::um::winuser::{GetSystemMetrics, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, CreateWindowExW, RegisterClassW, WNDCLASSW, UnregisterClassW, SetLayeredWindowAttributes, LWA_ALPHA, LoadCursorW, IDC_CROSS, IDC_HAND, FillRect, FrameRect, InvalidateRect, SetCapture, ReleaseCapture, GetCursorPos, PostMessageW, WM_CLOSE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_LBUTTONUP, WM_PAINT, WM_DESTROY, VK_ESCAPE, BeginPaint, EndPaint, PAINTSTRUCT, GetMessageW, TranslateMessage, DispatchMessageW, MSG, DefWindowProcW, PostQuitMessage, WS_EX_LAYERED, WS_EX_TOPMOST, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE, DrawTextW, DT_CENTER, DT_VCENTER, GetClientRect, GetWindowTextLengthW, GetWindowTextW, DT_WORDBREAK, SetTimer, KillTimer, WM_TIMER, GetDC, ReleaseDC, DT_CALCRECT, SetCursor, WM_SETCURSOR, TrackMouseEvent, TRACKMOUSEEVENT, TME_LEAVE, WM_MOUSELEAVE, GetWindowRect, RedrawWindow, RDW_INVALIDATE, RDW_UPDATENOW};
use winapi::um::wingdi::{CreateCompatibleDC, CreateCompatibleBitmap, SelectObject, DeleteObject, DeleteDC, BitBlt, SRCCOPY, CreateSolidBrush, SetTextColor, CreateFontW, SetBkMode, CreateRoundRectRgn, AddFontMemResourceEx, FrameRgn, FillRgn};
use winapi::um::winuser::{MoveWindow};
use std::sync::Mutex;
use winapi::um::libloaderapi::GetModuleHandleW;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::config;

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

static mut START_POS: POINT = POINT { x: 0, y: 0 };
static mut CURR_POS: POINT = POINT { x: 0, y: 0 };
static mut IS_DRAGGING: bool = false;

static OVERLAY_LIST: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static HOVER_MAP: OnceLock<Mutex<HashMap<usize, bool>>> = OnceLock::new();

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
            // Remove from list
            {
                let mut list = OVERLAY_LIST.lock().unwrap();
                list.retain(|&h| h != hwnd as usize);
            }
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
            {
                let map_mutex = HOVER_MAP.get_or_init(|| Mutex::new(HashMap::new()));
                let mut map = map_mutex.lock().unwrap();
                map.remove(&(hwnd as usize));
            }
            unsafe { KillTimer(hwnd, 2); }
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

        let region_width = (target_rect.right - target_rect.left).abs();

        // Calculate text size
        let hdc_screen = GetDC(std::ptr::null_mut());
        let font_size = 20;
        let hfont = CreateFontW(font_size, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 2, 0, to_wide("Roboto").as_ptr());
        SelectObject(hdc_screen, hfont as *mut winapi::ctypes::c_void);
        let padding = 4;
        let max_text_width = region_width - padding * 2;
        let mut text_rect = RECT { left: 0, top: 0, right: max_text_width, bottom: 0 };
        let wide_text = to_wide(&text);
        DrawTextW(hdc_screen, wide_text.as_ptr(), -1, &mut text_rect, DT_CALCRECT | DT_WORDBREAK);
        let text_height = text_rect.bottom - text_rect.top;
        DeleteObject(hfont as *mut winapi::ctypes::c_void);
        ReleaseDC(std::ptr::null_mut(), hdc_screen);

        let height = text_height + padding * 2;

        let x = target_rect.left;
        let width = region_width as i32;
        let mut y = target_rect.top - height - 10; // A little above
        if y < 0 {
            y = target_rect.top + 10; // Below if above is off-screen
        }

        // Push existing overlays up
        {
            let list = OVERLAY_LIST.lock().unwrap();
            for &hwnd_usize in list.iter() {
                let hwnd = hwnd_usize as HWND;
                let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                unsafe { GetWindowRect(hwnd, &mut rect); }
                unsafe { MoveWindow(hwnd, rect.left, rect.top - height - 10, rect.right - rect.left, rect.bottom - rect.top, 1); }
            }
        }

        // Load Roboto font
        let font_data = include_bytes!("roboto.ttf");
        let _font_handle = AddFontMemResourceEx(font_data.as_ptr() as *mut winapi::ctypes::c_void, font_data.len() as u32, std::ptr::null_mut(), &mut 0);

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

        // Add to list
        {
            let mut list = OVERLAY_LIST.lock().unwrap();
            list.push(hwnd as usize);
        }

        SetLayeredWindowAttributes(hwnd, 0, 220, LWA_ALPHA);
        SetTimer(hwnd, 1, duration_ms, None);
        // Removed Timer 2 (polling) in favor of event-driven tracking

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
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_HAND));
            1
        }
        WM_MOUSEMOVE => {
            let map_mutex = HOVER_MAP.get_or_init(|| Mutex::new(HashMap::new()));
            let mut map = map_mutex.lock().unwrap();
            
            // If we aren't already marked as hovering, start tracking
            if !*map.get(&(hwnd as usize)).unwrap_or(&false) {
                map.insert(hwnd as usize, true);
                
                // Request notification when mouse leaves the window
                let mut tme: TRACKMOUSEEVENT = std::mem::zeroed();
                tme.cbSize = std::mem::size_of::<TRACKMOUSEEVENT>() as u32;
                tme.dwFlags = TME_LEAVE;
                tme.hwndTrack = hwnd;
                tme.dwHoverTime = 0;
                unsafe { TrackMouseEvent(&mut tme); }
                
                // Invalidate only this window to redraw with green border
                unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
            }
            0
        }
        WM_MOUSELEAVE => {
            let map_mutex = HOVER_MAP.get_or_init(|| Mutex::new(HashMap::new()));
            let mut map = map_mutex.lock().unwrap();
            
            // Mark as not hovering
            map.insert(hwnd as usize, false);
            
            // Invalidate to redraw without border
            unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
            0
        }
        WM_TIMER => {
            // Timer 1 is for auto-close
            if wparam == 1 {
                KillTimer(hwnd, 1);
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rect: RECT = unsafe { std::mem::zeroed() };
            GetClientRect(hwnd, &mut rect);

            // Double Buffering to prevent flicker
            let mem_dc = CreateCompatibleDC(hdc);
            let mem_bitmap = CreateCompatibleBitmap(hdc, rect.right, rect.bottom);
            SelectObject(mem_dc, mem_bitmap as *mut winapi::ctypes::c_void);

            // 1. Clear background (transparent/black)
            let clear_brush = CreateSolidBrush(0x00000000);
            FillRect(mem_dc, &rect, clear_brush);
            DeleteObject(clear_brush as *mut winapi::ctypes::c_void);

            // 2. Draw the main rounded rectangle background (Black)
            let hrgn = CreateRoundRectRgn(0, 0, rect.right, rect.bottom, 8, 8);
            let bg_brush = CreateSolidBrush(0x00000000); // Black background
            FillRgn(mem_dc, hrgn, bg_brush);
            DeleteObject(bg_brush as *mut winapi::ctypes::c_void);

            // 3. Check Hover State and Draw Green Border
            {
                let map_mutex = HOVER_MAP.get_or_init(|| Mutex::new(HashMap::new()));
                let map = map_mutex.lock().unwrap();
                if *map.get(&(hwnd as usize)).unwrap_or(&false) {
                    let green_brush = CreateSolidBrush(0x0000FF00); // 0x00bbggrr - Green
                    // Frame the region (draws border)
                    FrameRgn(mem_dc, hrgn, green_brush, 2, 2);
                    DeleteObject(green_brush as *mut winapi::ctypes::c_void);
                }
            }

            // 4. Draw Text
            SetBkMode(mem_dc, 1); // Transparent background for text
            SetTextColor(mem_dc, 0x00FFFFFF); // White text

            let text_len = GetWindowTextLengthW(hwnd) + 1;
            let mut buf = vec![0u16; text_len as usize];
            GetWindowTextW(hwnd, buf.as_mut_ptr(), text_len);

            let font_size = 20;
            let hfont = CreateFontW(font_size, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 2, 0, to_wide("Segoe UI").as_ptr());
            let old_font = SelectObject(mem_dc, hfont as *mut winapi::ctypes::c_void);

            let mut draw_rect = rect;
            DrawTextW(mem_dc, buf.as_mut_ptr(), -1, &mut draw_rect, DT_CENTER | DT_WORDBREAK | DT_VCENTER);

            // 5. Copy memory buffer to screen
            BitBlt(hdc, 0, 0, rect.right, rect.bottom, mem_dc, 0, 0, SRCCOPY);

            // Cleanup
            SelectObject(mem_dc, old_font);
            DeleteObject(hfont as *mut winapi::ctypes::c_void);
            DeleteObject(hrgn as *mut winapi::ctypes::c_void);
            DeleteObject(mem_bitmap as *mut winapi::ctypes::c_void);
            DeleteDC(mem_dc);

            EndPaint(hwnd, &mut ps);
            0
        }
        WM_DESTROY => {
            // Clean up map entry
            let map_mutex = HOVER_MAP.get_or_init(|| Mutex::new(HashMap::new()));
            let mut map = map_mutex.lock().unwrap();
            map.remove(&(hwnd as usize));
            
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}