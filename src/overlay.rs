use winapi::shared::windef::{POINT, RECT, HWND, SIZE, HBITMAP};
use winapi::shared::minwindef::{WPARAM, LPARAM, LRESULT, TRUE};
use winapi::um::winuser::{
    GetSystemMetrics, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, CreateWindowExW, RegisterClassW, WNDCLASSW, UnregisterClassW,
    SetLayeredWindowAttributes, LWA_ALPHA, LWA_COLORKEY, LoadCursorW, IDC_HAND, IDC_ARROW, FillRect,
    FrameRect, InvalidateRect, SetCapture, ReleaseCapture, GetCursorPos, PostMessageW,
    WM_CLOSE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_LBUTTONUP, WM_PAINT,
    WM_DESTROY, VK_ESCAPE, BeginPaint, EndPaint, PAINTSTRUCT, GetMessageW,
    TranslateMessage, DispatchMessageW, MSG, DefWindowProcW, PostQuitMessage,
    WS_EX_LAYERED, WS_EX_TOPMOST, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE, DrawTextW,
    GetClientRect, GetWindowTextLengthW, GetWindowTextW,
    DT_WORDBREAK, SetTimer, KillTimer, WM_TIMER, GetDC, ReleaseDC, DT_CALCRECT,
    SetCursor, WM_SETCURSOR, TrackMouseEvent, TRACKMOUSEEVENT, TME_LEAVE, WM_MOUSELEAVE,
    GetWindowRect, ShowWindow, SW_SHOW, IsWindow,
    WS_EX_NOACTIVATE, UpdateWindow, WS_EX_TRANSPARENT, FindWindowW, SetWindowTextW
};
use winapi::um::wingdi::{
    CreateCompatibleDC, CreateCompatibleBitmap, SelectObject, DeleteObject, DeleteDC,
    BitBlt, SRCCOPY, CreateSolidBrush, SetTextColor, CreateFontW, SetBkMode,
    CreateRoundRectRgn, AddFontMemResourceEx, FrameRgn, FillRgn,
    GetTextExtentPoint32W, SetTextJustification, TextOutW,
    MoveToEx, LineTo, CreatePen, PS_SOLID, TRANSPARENT, PS_INSIDEFRAME,
    PatBlt, SRCAND
};
use winapi::um::winuser::MoveWindow;
use std::sync::Mutex;
use winapi::um::libloaderapi::GetModuleHandleW;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::collections::HashMap;
use std::sync::{OnceLock, Once, atomic::{AtomicU8, AtomicBool, AtomicI32, AtomicUsize, Ordering}};
use std::time::{SystemTime, UNIX_EPOCH};
use rand::Rng;

use crate::config;
use crate::tts;

#[derive(Clone, Copy)]
struct Particle { x: f32, y: f32, vx: f32, vy: f32, life: f32, color: u32 }

static ANIMATION_MAP: OnceLock<Mutex<HashMap<usize, Vec<Particle>>>> = OnceLock::new();
static mut START_POS: POINT = POINT { x: 0, y: 0 };
static mut CURR_POS: POINT = POINT { x: 0, y: 0 };
static mut IS_DRAGGING: bool = false;
static OVERLAY_LIST: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static HOVER_MAP: OnceLock<Mutex<HashMap<usize, bool>>> = OnceLock::new();

// Thêm Map để lưu ID request của từng cửa sổ
static WINDOW_REQ_IDS: OnceLock<Mutex<HashMap<usize, u64>>> = OnceLock::new();

static SELECTION_MODE: AtomicU8 = AtomicU8::new(0);
static DEBUG_ACTIVE: AtomicBool = AtomicBool::new(false);
static CURRENT_FONT_SIZE: AtomicI32 = AtomicI32::new(24);

pub static ARROW_DEBUG_STATE: AtomicBool = AtomicBool::new(false);

// --- Biến lưu handle của cửa sổ đang Loading "..." ---
static PENDING_LOADING_HWND: AtomicUsize = AtomicUsize::new(0);

static mut FROZEN_BITMAP: HBITMAP = std::ptr::null_mut();

pub fn set_selection_mode(mode: u8) {
    SELECTION_MODE.store(mode, Ordering::Relaxed);
}

pub fn set_font_size(size: i32) {
    CURRENT_FONT_SIZE.store(size, Ordering::Relaxed);
}

pub fn is_debug_active() -> bool {
    DEBUG_ACTIVE.load(Ordering::Relaxed)
}

fn to_wide(s: &str) -> Vec<u16> { OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect() }
fn simple_rng(seed: &mut u32) -> u32 { *seed = seed.wrapping_mul(1103515245).wrapping_add(12345); (*seed / 65536) % 32768 }
fn get_random_f32(seed: &mut u32, min: f32, max: f32) -> f32 { let r = simple_rng(seed) as f32 / 32768.0; min + r * (max - min) }

struct TextLine { text: Vec<u16>, width: i32, spaces: i32 }

unsafe fn break_text_into_lines(hdc: winapi::shared::windef::HDC, text: &[u16], max_width: i32) -> Vec<TextLine> {
    let mut lines = Vec::new(); let mut words = Vec::new(); let mut current_word = Vec::new();
    for &c in text { if c == 32 { if !current_word.is_empty() { words.push(current_word.clone()); current_word.clear(); } } else { current_word.push(c); } }
    if !current_word.is_empty() { words.push(current_word); }
    if words.is_empty() { return lines; }
    let mut current_line = words[0].clone(); let mut size: SIZE = std::mem::zeroed();
    GetTextExtentPoint32W(hdc, current_line.as_ptr(), current_line.len() as i32, &mut size);
    let mut current_width = size.cx;
    for i in 1..words.len() {
        let word = &words[i]; let mut test_line = current_line.clone(); test_line.push(32); test_line.extend_from_slice(word);
        GetTextExtentPoint32W(hdc, test_line.as_ptr(), test_line.len() as i32, &mut size);
        if size.cx > max_width {
            let spaces = current_line.iter().filter(|&&c| c == 32).count() as i32;
            lines.push(TextLine { text: current_line, width: current_width, spaces });
            current_line = word.clone(); GetTextExtentPoint32W(hdc, current_line.as_ptr(), current_line.len() as i32, &mut size); current_width = size.cx;
        } else { current_line = test_line; current_width = size.cx; }
    }
    if !current_line.is_empty() { let spaces = current_line.iter().filter(|&&c| c == 32).count() as i32; lines.push(TextLine { text: current_line, width: current_width, spaces }); }
    lines
}

pub fn toggle_debug_overlay() {
    let current = DEBUG_ACTIVE.load(Ordering::Relaxed);
    if current {
        DEBUG_ACTIVE.store(false, Ordering::Relaxed);
    } else {
        DEBUG_ACTIVE.store(true, Ordering::Relaxed);
        std::thread::spawn(|| {
            unsafe {
                let instance = GetModuleHandleW(std::ptr::null());
                let class_name = to_wide("DebugOverlay");
                let mut wc: WNDCLASSW = std::mem::zeroed();
                wc.lpfnWndProc = Some(debug_wnd_proc);
                wc.hInstance = instance;
                wc.lpszClassName = class_name.as_ptr();
                wc.hbrBackground = CreateSolidBrush(0x00000000);
                RegisterClassW(&wc);

                let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
                let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);

                let hwnd = CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT,
                    class_name.as_ptr(), to_wide("DebugRects").as_ptr(), WS_POPUP | WS_VISIBLE,
                    x, y, w, h, std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null_mut()
                );

                SetLayeredWindowAttributes(hwnd, 0x00000000, 0, LWA_COLORKEY);
                SetTimer(hwnd, 1, 100, None);

                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) != 0 {
                    TranslateMessage(&msg); DispatchMessageW(&msg);
                    if msg.message == WM_CLOSE { break; }
                }
                UnregisterClassW(class_name.as_ptr(), instance);
            }
        });
    }
}

unsafe extern "system" fn debug_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            if !DEBUG_ACTIVE.load(Ordering::Relaxed) {
                KillTimer(hwnd, 1);
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            } else {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut r: RECT = std::mem::zeroed(); GetClientRect(hwnd, &mut r);

            let bg_brush = CreateSolidBrush(0x00000000);
            FillRect(hdc, &r, bg_brush);
            DeleteObject(bg_brush as *mut winapi::ctypes::c_void);

            SetBkMode(hdc, TRANSPARENT as i32);

            let debug_txt = to_wide("DEBUG MODE: ON");
            SetTextColor(hdc, 0x0000FF00);
            TextOutW(hdc, 10, 10, debug_txt.as_ptr(), debug_txt.len() as i32);

            let is_found = ARROW_DEBUG_STATE.load(Ordering::Relaxed);
            let status_text = if is_found { "MŨI TÊN: TÌM THẤY (FOUND)" } else { "MŨI TÊN: KHÔNG THẤY (MISSING)" };
            let wide_status = to_wide(status_text);
            let color = if is_found { 0x0000FF00 } else { 0x000000FF };
            SetTextColor(hdc, color);
            TextOutW(hdc, 10, 40, wide_status.as_ptr(), wide_status.len() as i32);

            let cfg = config::Config::load();
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);

            let null_brush = winapi::um::wingdi::GetStockObject(winapi::um::wingdi::NULL_BRUSH as i32);
            let old_brush = SelectObject(hdc, null_brush);

            if !cfg.fixed_regions.is_empty() {
                let red_pen = CreatePen(PS_SOLID.try_into().unwrap(), 2, 0x000000FF);
                let old_pen = SelectObject(hdc, red_pen as *mut winapi::ctypes::c_void);
                for region in &cfg.fixed_regions {
                    let left = region.x - vx;
                    let top = region.y - vy;
                    let right = left + region.width as i32;
                    let bottom = top + region.height as i32;
                    winapi::um::wingdi::Rectangle(hdc, left, top, right, bottom);
                    let txt = to_wide("Vùng Dịch");
                    SetTextColor(hdc, 0x000000FF);
                    TextOutW(hdc, left, top - 20, txt.as_ptr(), txt.len() as i32);
                }
                SelectObject(hdc, old_pen); DeleteObject(red_pen as *mut winapi::ctypes::c_void);
            }

            if let Some(arrow) = &cfg.arrow_region {
                let yellow_pen = CreatePen(PS_SOLID.try_into().unwrap(), 2, 0x0000FFFF);
                let old_pen = SelectObject(hdc, yellow_pen as *mut winapi::ctypes::c_void);
                let left = arrow.x - vx;
                let top = arrow.y - vy;
                let right = left + arrow.width as i32;
                let bottom = top + arrow.height as i32;
                winapi::um::wingdi::Rectangle(hdc, left, top, right, bottom);
                let txt = to_wide("Mũi Tên");
                SetTextColor(hdc, 0x0000FFFF);
                TextOutW(hdc, left, top - 20, txt.as_ptr(), txt.len() as i32);
                SelectObject(hdc, old_pen); DeleteObject(yellow_pen as *mut winapi::ctypes::c_void);
            }

            if let Some(instant) = &cfg.instant_region {
                let blue_pen = CreatePen(PS_SOLID.try_into().unwrap(), 2, 0x00FF0000);
                let old_pen = SelectObject(hdc, blue_pen as *mut winapi::ctypes::c_void);
                let left = instant.x - vx;
                let top = instant.y - vy;
                let right = left + instant.width as i32;
                let bottom = top + instant.height as i32;
                winapi::um::wingdi::Rectangle(hdc, left, top, right, bottom);
                let txt = to_wide("Dịch Nhanh");
                SetTextColor(hdc, 0x00FF0000);
                TextOutW(hdc, left, top - 20, txt.as_ptr(), txt.len() as i32);
                SelectObject(hdc, old_pen); DeleteObject(blue_pen as *mut winapi::ctypes::c_void);
            }

            SelectObject(hdc, old_brush);
            EndPaint(hwnd, &mut ps); 0
        }
        WM_DESTROY => { PostQuitMessage(0); 0 }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn show_selection_overlay() {
    unsafe {
        IS_DRAGGING = false;
        let config = config::Config::load();

        FROZEN_BITMAP = std::ptr::null_mut();
        if config.freeze_screen {
            let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);

            let hdc_screen = GetDC(std::ptr::null_mut());
            let hdc_mem = CreateCompatibleDC(hdc_screen);
            let hbm = CreateCompatibleBitmap(hdc_screen, w, h);
            SelectObject(hdc_mem, hbm as *mut _);
            BitBlt(hdc_mem, 0, 0, w, h, hdc_screen, x, y, SRCCOPY);

            FROZEN_BITMAP = hbm;

            DeleteDC(hdc_mem);
            ReleaseDC(std::ptr::null_mut(), hdc_screen);
        }

        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = to_wide("SnippingOverlay");
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(selection_wnd_proc);
        wc.hInstance = instance;
        wc.lpszClassName = class_name.as_ptr();
        wc.hbrBackground = CreateSolidBrush(0x00FF00FF);
        RegisterClassW(&wc);

        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);

        let ex_style = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW;
        let hwnd = CreateWindowExW(ex_style, class_name.as_ptr(), to_wide("Snipping").as_ptr(), WS_POPUP, x, y, w, h, std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null_mut());

        if config.freeze_screen {
             SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA);
        } else {
             SetLayeredWindowAttributes(hwnd, 0x00FF00FF, 100, LWA_ALPHA | LWA_COLORKEY);
        }

        GetCursorPos(std::ptr::addr_of_mut!(CURR_POS));
        ShowWindow(hwnd, SW_SHOW);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) != 0 {
            TranslateMessage(&msg); DispatchMessageW(&msg);
            if msg.message == WM_CLOSE { break; }
        }

        if !FROZEN_BITMAP.is_null() {
            DeleteObject(FROZEN_BITMAP as *mut _);
            FROZEN_BITMAP = std::ptr::null_mut();
        }

        UnregisterClassW(class_name.as_ptr(), instance);
    }
}

unsafe extern "system" fn selection_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_KEYDOWN => { if wparam == VK_ESCAPE as usize { PostMessageW(hwnd, WM_CLOSE, 0, 0); } 0 }
        WM_LBUTTONDOWN => {
            IS_DRAGGING = true; GetCursorPos(std::ptr::addr_of_mut!(START_POS)); CURR_POS = START_POS;
            SetCapture(hwnd); InvalidateRect(hwnd, std::ptr::null(), 0);
            UpdateWindow(hwnd);
            0
        }
        WM_MOUSEMOVE => {
            GetCursorPos(std::ptr::addr_of_mut!(CURR_POS));
            InvalidateRect(hwnd, std::ptr::null(), 0);
            UpdateWindow(hwnd);
            0
        }
        WM_LBUTTONUP => {
            if IS_DRAGGING {
                IS_DRAGGING = false; ReleaseCapture();
                let rect = RECT { left: START_POS.x.min(CURR_POS.x), top: START_POS.y.min(CURR_POS.y), right: START_POS.x.max(CURR_POS.x), bottom: START_POS.y.max(CURR_POS.y) };
                if (rect.right - rect.left) > 10 && (rect.bottom - rect.top) > 10 {
                    let region = config::Region { x: rect.left, y: rect.top, width: (rect.right - rect.left) as u32, height: (rect.bottom - rect.top) as u32 };
                    let hwnd_usize = hwnd as usize;
                    std::thread::spawn(move || { process_region(region); unsafe { PostMessageW(hwnd_usize as HWND, WM_CLOSE, 0, 0); } });
                } else { PostMessageW(hwnd, WM_CLOSE, 0, 0); }
            } 0
        }
        WM_SETCURSOR => {
            let mode = SELECTION_MODE.load(Ordering::Relaxed);
            if mode == 2 {
                SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_ARROW));
            } else {
                SetCursor(std::ptr::null_mut());
            }
            1
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mem_dc = CreateCompatibleDC(hdc);
            let w = GetSystemMetrics(SM_CXVIRTUALSCREEN); let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN); let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);

            let mem_bm = CreateCompatibleBitmap(hdc, w, h); SelectObject(mem_dc, mem_bm as *mut winapi::ctypes::c_void);
            let mode = SELECTION_MODE.load(Ordering::Relaxed);

            if !FROZEN_BITMAP.is_null() {
                // === FREEZE MODE ===
                let hdc_src = CreateCompatibleDC(hdc);
                SelectObject(hdc_src, FROZEN_BITMAP as *mut _);

                // 1. Draw source
                BitBlt(mem_dc, 0, 0, w, h, hdc_src, 0, 0, SRCCOPY);

                // 2. Dim screen
                let dim_brush = CreateSolidBrush(0x00404040);
                let old_brush = SelectObject(mem_dc, dim_brush as *mut _);
                PatBlt(mem_dc, 0, 0, w, h, SRCAND);
                SelectObject(mem_dc, old_brush);
                DeleteObject(dim_brush as *mut _);

                // 3. Border
                let pen_screen_border = CreatePen(PS_INSIDEFRAME as i32, 6, 0x00FF0000); // Blue (BGR)
                let old_pen_border = SelectObject(mem_dc, pen_screen_border as *mut _);
                let null_brush = winapi::um::wingdi::GetStockObject(winapi::um::wingdi::NULL_BRUSH as i32);
                let old_br_border = SelectObject(mem_dc, null_brush);
                winapi::um::wingdi::Rectangle(mem_dc, 0, 0, w, h);
                SelectObject(mem_dc, old_br_border);
                SelectObject(mem_dc, old_pen_border);
                DeleteObject(pen_screen_border as *mut _);

                // 4. Dragging rect
                if IS_DRAGGING {
                     let r = RECT {
                        left: (START_POS.x.min(CURR_POS.x)) - vx,
                        top: (START_POS.y.min(CURR_POS.y)) - vy,
                        right: (START_POS.x.max(CURR_POS.x)) - vx,
                        bottom: (START_POS.y.max(CURR_POS.y)) - vy
                    };

                    // Copy bright part from src
                    BitBlt(mem_dc, r.left, r.top, r.right - r.left, r.bottom - r.top,
                           hdc_src, r.left, r.top, SRCCOPY);

                    let color = if mode == 1 { 0x00FFFF00 } else if mode == 2 { 0x000080FF } else { 0x00FF00FF };
                    let pen_border = CreatePen(PS_SOLID.try_into().unwrap(), 2, color);
                    let old_pen = SelectObject(mem_dc, pen_border as *mut _);
                    let old_br = SelectObject(mem_dc, null_brush);

                    winapi::um::wingdi::Rectangle(mem_dc, r.left, r.top, r.right, r.bottom);

                    SelectObject(mem_dc, old_br);
                    SelectObject(mem_dc, old_pen);
                    DeleteObject(pen_border as *mut _);
                }

                DeleteDC(hdc_src);

            } else {
                // === NORMAL MODE ===
                let bg_brush = CreateSolidBrush(0x00000000);
                FillRect(mem_dc, &RECT{left:0,top:0,right:w,bottom:h}, bg_brush);
                DeleteObject(bg_brush as *mut winapi::ctypes::c_void);

                if IS_DRAGGING {
                    let r = RECT {
                        left: (START_POS.x.min(CURR_POS.x)) - vx,
                        top: (START_POS.y.min(CURR_POS.y)) - vy,
                        right: (START_POS.x.max(CURR_POS.x)) - vx,
                        bottom: (START_POS.y.max(CURR_POS.y)) - vy
                    };

                    let color = if mode == 1 { 0x00FFFF00 } else if mode == 2 { 0x000080FF } else { 0x00FF00FF };
                    let k_br = CreateSolidBrush(color);
                    FillRect(mem_dc, &r, k_br);
                    DeleteObject(k_br as *mut winapi::ctypes::c_void);

                    let b_br = CreateSolidBrush(0x00FFFFFF); FrameRect(mem_dc, &r, b_br); DeleteObject(b_br as *mut winapi::ctypes::c_void);
                }
            }

            if mode != 2 {
                let cx = CURR_POS.x - vx;
                let cy = CURR_POS.y - vy;
                let pen = CreatePen(PS_SOLID.try_into().unwrap(), 2, 0x000000FF);
                let old_pen = SelectObject(mem_dc, pen as *mut winapi::ctypes::c_void);
                MoveToEx(mem_dc, 0, cy, std::ptr::null_mut()); LineTo(mem_dc, w, cy);
                MoveToEx(mem_dc, cx, 0, std::ptr::null_mut()); LineTo(mem_dc, cx, h);
                SelectObject(mem_dc, old_pen); DeleteObject(pen as *mut winapi::ctypes::c_void);
            }

            BitBlt(hdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
            DeleteObject(mem_bm as *mut winapi::ctypes::c_void); DeleteDC(mem_dc); EndPaint(hwnd, &mut ps); 0
        }
        WM_DESTROY => { PostQuitMessage(0); 0 }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn process_region(region: config::Region) {
    let mut config = config::Config::load();
    let mode = SELECTION_MODE.load(Ordering::Relaxed);

    if mode == 0 {
        config.fixed_regions.clear();
        config.fixed_regions.push(region);
    } else if mode == 1 {
        config.arrow_region = Some(region);
    } else if mode == 2 {
        config.instant_region = Some(region);
    } else if mode >= 100 {
        let idx = (mode - 100) as usize;
        if idx < config.aux_regions.len() {
            config.aux_regions[idx].region = Some(region);
        }
    }
    config.save().unwrap();
}

pub fn show_highlight(rect: RECT) {
    std::thread::spawn(move || {
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let class_name = to_wide("HighlightOverlay");
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(highlight_wnd_proc);
            wc.hInstance = instance;
            wc.lpszClassName = class_name.as_ptr();
            wc.hbrBackground = CreateSolidBrush(0x0000FF00);
            RegisterClassW(&wc);

            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class_name.as_ptr(), to_wide("Highlight").as_ptr(), WS_POPUP | WS_VISIBLE,
                rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top,
                std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null_mut()
            );

            SetLayeredWindowAttributes(hwnd, 0, 100, 0x00000002);
            SetTimer(hwnd, 999, 3000, None);

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) != 0 {
                TranslateMessage(&msg); DispatchMessageW(&msg);
                if msg.message == WM_CLOSE { break; }
            }
            UnregisterClassW(class_name.as_ptr(), instance);
        }
    });
}

static REGISTER_RESULT_CLASS: Once = Once::new();
pub fn show_loading_window(rect: RECT) {
    show_result_window_internal(rect, "...".to_string(), 20000, true, 0);
}

// --- HÀM MỚI: Cập nhật nội dung cửa sổ "Loading" ---
pub fn update_loading_window(text: String) -> bool { // Thêm -> bool
    let hwnd_val = PENDING_LOADING_HWND.load(Ordering::Relaxed);
    if hwnd_val == 0 { return false; } // Trả về false
    let hwnd = hwnd_val as HWND;
    if unsafe { IsWindow(hwnd) } == 0 { return false; } // Trả về false

    unsafe {
        // Reset thời gian tắt (vì đã có kết quả)
        SetTimer(hwnd, 1, 20000, None); // Ví dụ: 20s để đọc

        // Tính lại chiều cao mới
        let hdc = GetDC(hwnd);
        let font_size = CURRENT_FONT_SIZE.load(Ordering::Relaxed);
        let hfont = CreateFontW(font_size, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 2, 0, to_wide("Roboto").as_ptr());
        SelectObject(hdc, hfont as *mut winapi::ctypes::c_void);

        let mut rect: RECT = std::mem::zeroed();
        GetWindowRect(hwnd, &mut rect);
        let width = rect.right - rect.left;
        let padding = 10;
        let max_text_width = width - padding * 2;

        let mut text_rect = RECT { left: 0, top: 0, right: max_text_width, bottom: 0 };
        let wide_text = to_wide(&text);
        DrawTextW(hdc, wide_text.as_ptr(), -1, &mut text_rect, DT_CALCRECT | DT_WORDBREAK);
        let new_height = (text_rect.bottom - text_rect.top) + padding * 2;

        DeleteObject(hfont as *mut winapi::ctypes::c_void);
        ReleaseDC(hwnd, hdc);

        // Cập nhật text & Resize
        SetWindowTextW(hwnd, wide_text.as_ptr());
        MoveWindow(hwnd, rect.left, rect.top, width, new_height, 1);
        InvalidateRect(hwnd, std::ptr::null(), TRUE);

        // Reset cờ chờ
        PENDING_LOADING_HWND.store(0, Ordering::Relaxed);
    }
    true // Trả về true
}

// --- HÀM CHUNG ---
pub fn show_result_window_internal(target_rect: RECT, text: String, duration_ms: u32, is_loading: bool, req_id: u64) {
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = to_wide("TranslationResult");
        REGISTER_RESULT_CLASS.call_once(|| {
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(result_wnd_proc);
            wc.hInstance = instance;
            wc.lpszClassName = class_name.as_ptr();
            wc.hbrBackground = CreateSolidBrush(0x00FF00FF);
            wc.style = winapi::um::winuser::CS_HREDRAW | winapi::um::winuser::CS_VREDRAW;
            RegisterClassW(&wc);
        });

        let region_width = (target_rect.right - target_rect.left).abs();
        let hdc_screen = GetDC(std::ptr::null_mut());

        let font_size = CURRENT_FONT_SIZE.load(Ordering::Relaxed);
        let hfont = CreateFontW(font_size, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 2, 0, to_wide("Roboto").as_ptr());

        SelectObject(hdc_screen, hfont as *mut winapi::ctypes::c_void);
        let padding = 10; let max_text_width = region_width - padding * 2;
        let mut text_rect = RECT { left: 0, top: 0, right: max_text_width, bottom: 0 };
        let wide_text = to_wide(&text);
        DrawTextW(hdc_screen, wide_text.as_ptr(), -1, &mut text_rect, DT_CALCRECT | DT_WORDBREAK);
        let text_height = text_rect.bottom - text_rect.top;
        DeleteObject(hfont as *mut winapi::ctypes::c_void);
        ReleaseDC(std::ptr::null_mut(), hdc_screen);

        let height = text_height + padding * 2;
        let x = target_rect.left;
        let width = region_width as i32;
        let mut y = target_rect.top - height - 10;
        if y < 0 { y = target_rect.top + 10; } // Fallback nếu sát mép trên

        // --- PUSH UP LOGIC (ĐẨY CÁC CỬA SỔ CŨ LÊN) ---
        let valid_hwnds: Vec<HWND> = {
            let mut list = OVERLAY_LIST.lock().unwrap();
            list.retain(|&h| IsWindow(h as HWND) != 0);
            list.iter().map(|&h| h as HWND).collect()
        };
        for hwnd in valid_hwnds {
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            if GetWindowRect(hwnd, &mut r) != 0 {
                // Đẩy lên đúng bằng chiều cao cửa sổ mới + khoảng cách
                MoveWindow(hwnd, r.left, r.top - height - 10, r.right - r.left, r.bottom - r.top, 1);
            }
        }
        // ---------------------------------------------

        let font_data = include_bytes!("roboto.ttf");
        let _ = AddFontMemResourceEx(font_data.as_ptr() as *mut winapi::ctypes::c_void, font_data.len() as u32, std::ptr::null_mut(), &mut 0);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name.as_ptr(), to_wide(&text).as_ptr(), WS_POPUP | WS_VISIBLE, x, y, width, height, std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null_mut());

        if !hwnd.is_null() {
            {
                let mut list = OVERLAY_LIST.lock().unwrap();
                list.push(hwnd as usize);
            }

            // --- LƯU ID REQUEST VÀO MAP ---
            {
                let map = WINDOW_REQ_IDS.get_or_init(|| Mutex::new(HashMap::new()));
                map.lock().unwrap().insert(hwnd as usize, req_id);
            }
            // Đăng ký với TTS để TTS có thể gửi lệnh đóng khi đọc xong
            crate::tts::register_window(req_id, hwnd as usize);

            // Nếu là cửa sổ Loading, lưu lại Handle để cập nhật sau
            if is_loading {
                PENDING_LOADING_HWND.store(hwnd as usize, Ordering::Relaxed);
            }

            SetLayeredWindowAttributes(hwnd, 0x00FF00FF, 200, LWA_ALPHA | LWA_COLORKEY);

            // Tăng duration fallback lên (ví dụ +5s) để ưu tiên việc đóng bằng TTS
            // Nếu TTS bị lỗi thì timer này sẽ đóng cửa sổ sau cùng
            SetTimer(hwnd, 1, duration_ms + 5000, None);

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) != 0 {
                TranslateMessage(&msg); DispatchMessageW(&msg);
                if msg.message == WM_CLOSE { break; }
            }
        }
    }
}

pub fn show_result_window(target_rect: RECT, text: String, duration_ms: u32, req_id: u64) {
    show_result_window_internal(target_rect, text, duration_ms, false, req_id);
}

unsafe extern "system" fn highlight_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut r: RECT = std::mem::zeroed(); GetClientRect(hwnd, &mut r);

            let fill_brush = CreateSolidBrush(0x0000FF00);
            FillRect(hdc, &r, fill_brush);
            DeleteObject(fill_brush as *mut winapi::ctypes::c_void);

            let border_brush = CreateSolidBrush(0x000000FF);
            FrameRect(hdc, &r, border_brush);
            let mut r2 = r; r2.left += 1; r2.top += 1; r2.right -= 1; r2.bottom -= 1;
            FrameRect(hdc, &r2, border_brush);
            let mut r3 = r2; r3.left += 1; r3.top += 1; r3.right -= 1; r3.bottom -= 1;
            FrameRect(hdc, &r3, border_brush);

            DeleteObject(border_brush as *mut winapi::ctypes::c_void);
            EndPaint(hwnd, &mut ps); 0
        }
        WM_TIMER => { if wparam == 999 { KillTimer(hwnd, 999); PostMessageW(hwnd, WM_CLOSE, 0, 0); } 0 }
        WM_DESTROY => { PostQuitMessage(0); 0 }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}


unsafe extern "system" fn result_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rect: RECT = std::mem::zeroed(); GetClientRect(hwnd, &mut rect);

            // Kiểm tra xem cửa sổ có đang trong trạng thái "vỡ" (animation) không
            let is_animating = {
                let map = ANIMATION_MAP.get_or_init(|| Mutex::new(HashMap::new()));
                map.lock().unwrap().contains_key(&(hwnd as usize))
            };

            let mem_dc = CreateCompatibleDC(hdc);
            let mem_bm = CreateCompatibleBitmap(hdc, rect.right, rect.bottom); SelectObject(mem_dc, mem_bm as *mut winapi::ctypes::c_void);

            // Xóa nền (Transparent)
            let bg_brush = CreateSolidBrush(0x00FF00FF); // Màu Key
            FillRect(mem_dc, &rect, bg_brush);
            DeleteObject(bg_brush as *mut winapi::ctypes::c_void);

            if is_animating {
                // --- VẼ HIỆU ỨNG HẠT ---
                let map = ANIMATION_MAP.get().unwrap().lock().unwrap();
                if let Some(particles) = map.get(&(hwnd as usize)) {
                    for p in particles {
                        if p.life > 0.0 {
                            // Vẽ hạt (hình vuông nhỏ hoặc pixel)
                            let size = 3;
                            let p_rect = RECT {
                                left: p.x as i32, top: p.y as i32,
                                right: p.x as i32 + size, bottom: p.y as i32 + size
                            };
                            let p_brush = CreateSolidBrush(p.color);
                            FillRect(mem_dc, &p_rect, p_brush);
                            DeleteObject(p_brush as *mut winapi::ctypes::c_void);
                        }
                    }
                }
            } else {
                // --- VẼ TEXT BÌNH THƯỜNG ---
                let hrgn = CreateRoundRectRgn(0, 0, rect.right, rect.bottom, 8, 8);
                let bg_br = CreateSolidBrush(0x00000000); // Đen mờ
                FillRgn(mem_dc, hrgn, bg_br);
                DeleteObject(bg_br as *mut winapi::ctypes::c_void);

                // Viền xanh khi hover
                {
                    let map_mutex = HOVER_MAP.get_or_init(|| Mutex::new(HashMap::new()));
                    let map = map_mutex.lock().unwrap();
                    if *map.get(&(hwnd as usize)).unwrap_or(&false) {
                        let g_br = CreateSolidBrush(0x0000FF00);
                        FrameRgn(mem_dc, hrgn, g_br, 2, 2);
                        DeleteObject(g_br as *mut winapi::ctypes::c_void);
                    }
                }
                DeleteObject(hrgn as *mut winapi::ctypes::c_void);

                SetBkMode(mem_dc, 1); SetTextColor(mem_dc, 0x00FFFFFF);
                let font_size = CURRENT_FONT_SIZE.load(Ordering::Relaxed);
                let hfont = CreateFontW(font_size, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 2, 0, to_wide("Roboto").as_ptr());
                let old_font = SelectObject(mem_dc, hfont as *mut winapi::ctypes::c_void);

                // Lấy text và vẽ
                let len = GetWindowTextLengthW(hwnd) + 1;
                let mut buf = vec![0u16; len as usize]; GetWindowTextW(hwnd, buf.as_mut_ptr(), len);
                let v_len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                let lines = break_text_into_lines(mem_dc, &buf[0..v_len], (rect.right - rect.left) - 20);

                // Tính toán vị trí vẽ text
                let mut sz: SIZE = std::mem::zeroed();
                GetTextExtentPoint32W(mem_dc, to_wide("A").as_ptr(), 1, &mut sz);
                let start_y = (rect.bottom - rect.top - (sz.cy * lines.len() as i32)) / 2;

                for (i, line) in lines.iter().enumerate() {
                    let y = start_y + (i as i32 * sz.cy);
                    if i < lines.len() - 1 && line.spaces > 0 {
                        SetTextJustification(mem_dc, ((rect.right - rect.left) - 20) - line.width, line.spaces as i32);
                        TextOutW(mem_dc, 10, y, line.text.as_ptr(), line.text.len() as i32);
                        SetTextJustification(mem_dc, 0, 0);
                    } else {
                        TextOutW(mem_dc, (rect.right - rect.left - line.width) / 2, y, line.text.as_ptr(), line.text.len() as i32);
                    }
                }
                SelectObject(mem_dc, old_font);
                DeleteObject(hfont as *mut winapi::ctypes::c_void);
            }

            BitBlt(hdc, 0, 0, rect.right, rect.bottom, mem_dc, 0, 0, SRCCOPY); DeleteObject(mem_bm as *mut winapi::ctypes::c_void); DeleteDC(mem_dc); EndPaint(hwnd, &mut ps); 0
        }
        WM_LBUTTONUP => {
            // 1. Tắt tiếng của RIÊNG khung này
            let req_id_map = WINDOW_REQ_IDS.get_or_init(|| Mutex::new(HashMap::new()));
            if let Some(&req_id) = req_id_map.lock().unwrap().get(&(hwnd as usize)) {
                crate::tts::stop_id(req_id); // Chỉ stop ID này
            }

            // 2. Khởi tạo hiệu ứng "vỡ" (Particles)
            let mut particles = Vec::new();
            let mut rect: RECT = std::mem::zeroed(); GetClientRect(hwnd, &mut rect);
            let w = rect.right - rect.left;
            let h = rect.bottom - rect.top;

            // Tạo khoảng 50 hạt ngẫu nhiên
            let mut rng = rand::thread_rng();
            for _ in 0..50 {
                particles.push(Particle {
                    x: rng.gen_range(0.0..w as f32),
                    y: rng.gen_range(0.0..h as f32),
                    vx: rng.gen_range(-5.0..5.0),
                    vy: rng.gen_range(-5.0..5.0),
                    life: 1.0,
                    color: 0x00FFFFFF, // Màu trắng
                });
            }

            {
                let map = ANIMATION_MAP.get_or_init(|| Mutex::new(HashMap::new()));
                map.lock().unwrap().insert(hwnd as usize, particles);
            }

            // 3. Bắt đầu Timer animation (ID 3)
            SetTimer(hwnd, 3, 16, None); // ~60fps
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_TIMER => {
            if wparam == 1 {
                // Timer đóng cửa sổ (Failsafe nếu TTS không gửi lệnh đóng)
                // Ta để thời gian dài ra chút để ưu tiên việc đóng bằng TTS
                KillTimer(hwnd, 1);
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            } else if wparam == 3 {
                // Timer Animation
                let mut should_close = false;
                {
                    let map = ANIMATION_MAP.get_or_init(|| Mutex::new(HashMap::new()));
                    let mut map_lock = map.lock().unwrap();
                    if let Some(particles) = map_lock.get_mut(&(hwnd as usize)) {
                        let mut alive_count = 0;
                        for p in particles.iter_mut() {
                            p.x += p.vx;
                            p.y += p.vy;
                            p.vy += 0.8; // Trọng lực
                            p.life -= 0.05; // Mờ dần
                            if p.life > 0.0 { alive_count += 1; }
                        }
                        if alive_count == 0 { should_close = true; }
                    } else {
                         should_close = true;
                    }
                }

                if should_close {
                    KillTimer(hwnd, 3);
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                } else {
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }
            0
        }
        WM_DESTROY => {
            // Dọn dẹp Map
            let hwnd_u = hwnd as usize;
            { let mut map = HOVER_MAP.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap(); map.remove(&hwnd_u); }
            { let mut map = ANIMATION_MAP.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap(); map.remove(&hwnd_u); }

            // Xóa khỏi TTS Map
            let mut req_id_map = WINDOW_REQ_IDS.get_or_init(|| Mutex::new(HashMap::new()));
            if let Some(req_id) = req_id_map.lock().unwrap().remove(&hwnd_u) {
                crate::tts::unregister_window(req_id);
            }

            { let mut list = OVERLAY_LIST.lock().unwrap(); list.retain(|&h| h != hwnd_u); }
            PostQuitMessage(0); 0
        }
        WM_KEYDOWN => { if wparam == VK_ESCAPE as usize { PostMessageW(hwnd, WM_CLOSE, 0, 0); } 0 }
        WM_SETCURSOR => { SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_HAND)); 1 }
        WM_MOUSEMOVE => { let anim = { ANIMATION_MAP.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap().contains_key(&(hwnd as usize)) }; if !anim { let map_mutex = HOVER_MAP.get_or_init(|| Mutex::new(HashMap::new())); let mut map = map_mutex.lock().unwrap(); if !*map.get(&(hwnd as usize)).unwrap_or(&false) { map.insert(hwnd as usize, true); let mut t = TRACKMOUSEEVENT{cbSize:std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags:TME_LEAVE, hwndTrack:hwnd, dwHoverTime:0}; unsafe { TrackMouseEvent(&mut t); InvalidateRect(hwnd, std::ptr::null(), 0); } } } 0 }
        WM_MOUSELEAVE => { let anim = { ANIMATION_MAP.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap().contains_key(&(hwnd as usize)) }; if !anim { HOVER_MAP.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap().insert(hwnd as usize, false); unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); } } 0 }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}