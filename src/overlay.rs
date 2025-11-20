use winapi::shared::windef::{POINT, RECT, HWND, SIZE};
use winapi::shared::minwindef::{WPARAM, LPARAM, LRESULT};
use winapi::um::winuser::{
    GetSystemMetrics, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, CreateWindowExW, RegisterClassW, WNDCLASSW, UnregisterClassW,
    SetLayeredWindowAttributes, LWA_ALPHA, LWA_COLORKEY, LoadCursorW, IDC_CROSS, IDC_HAND, FillRect,
    FrameRect, InvalidateRect, SetCapture, ReleaseCapture, GetCursorPos, PostMessageW,
    WM_CLOSE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_LBUTTONUP, WM_PAINT,
    WM_DESTROY, VK_ESCAPE, BeginPaint, EndPaint, PAINTSTRUCT, GetMessageW,
    TranslateMessage, DispatchMessageW, MSG, DefWindowProcW, PostQuitMessage,
    WS_EX_LAYERED, WS_EX_TOPMOST, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE, DrawTextW,
    GetClientRect, GetWindowTextLengthW, GetWindowTextW,
    DT_WORDBREAK, SetTimer, KillTimer, WM_TIMER, GetDC, ReleaseDC, DT_CALCRECT,
    SetCursor, WM_SETCURSOR, TrackMouseEvent, TRACKMOUSEEVENT, TME_LEAVE, WM_MOUSELEAVE,
    GetWindowRect, ShowWindow, SW_SHOW, IsWindow // <--- Quan trọng: Thêm IsWindow
};
use winapi::um::wingdi::{
    CreateCompatibleDC, CreateCompatibleBitmap, SelectObject, DeleteObject, DeleteDC,
    BitBlt, SRCCOPY, CreateSolidBrush, SetTextColor, CreateFontW, SetBkMode,
    CreateRoundRectRgn, AddFontMemResourceEx, FrameRgn, FillRgn,
    GetTextExtentPoint32W, SetTextJustification, TextOutW
};
use winapi::um::winuser::MoveWindow;
use std::sync::Mutex;
use winapi::um::libloaderapi::GetModuleHandleW;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::collections::HashMap;
use std::sync::{OnceLock, Once};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config;

// --- Struct & Globals ---
#[derive(Clone, Copy)]
struct Particle {
    x: f32, y: f32, vx: f32, vy: f32, size: i32, color: u32,
}

static ANIMATION_MAP: OnceLock<Mutex<HashMap<usize, Vec<Particle>>>> = OnceLock::new();
static mut START_POS: POINT = POINT { x: 0, y: 0 };
static mut CURR_POS: POINT = POINT { x: 0, y: 0 };
static mut IS_DRAGGING: bool = false;
static OVERLAY_LIST: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static HOVER_MAP: OnceLock<Mutex<HashMap<usize, bool>>> = OnceLock::new();

// --- Helpers ---
fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

fn simple_rng(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
    (*seed / 65536) % 32768
}

fn get_random_f32(seed: &mut u32, min: f32, max: f32) -> f32 {
    let r = simple_rng(seed) as f32 / 32768.0;
    min + r * (max - min)
}

struct TextLine {
    text: Vec<u16>,
    width: i32,
    spaces: i32,
}

unsafe fn break_text_into_lines(hdc: winapi::shared::windef::HDC, text: &[u16], max_width: i32) -> Vec<TextLine> {
    let mut lines = Vec::new();
    let mut words = Vec::new();
    let mut current_word = Vec::new();
    for &c in text {
        if c == 32 {
            if !current_word.is_empty() { words.push(current_word.clone()); current_word.clear(); }
        } else { current_word.push(c); }
    }
    if !current_word.is_empty() { words.push(current_word); }
    if words.is_empty() { return lines; }

    let mut current_line = words[0].clone();
    let mut size: SIZE = std::mem::zeroed();
    GetTextExtentPoint32W(hdc, current_line.as_ptr(), current_line.len() as i32, &mut size);
    let mut current_width = size.cx;

    for i in 1..words.len() {
        let word = &words[i];
        let mut test_line = current_line.clone();
        test_line.push(32); 
        test_line.extend_from_slice(word);

        GetTextExtentPoint32W(hdc, test_line.as_ptr(), test_line.len() as i32, &mut size);
        
        if size.cx > max_width {
            let spaces = current_line.iter().filter(|&&c| c == 32).count() as i32;
            lines.push(TextLine { text: current_line, width: current_width, spaces });
            current_line = word.clone();
            GetTextExtentPoint32W(hdc, current_line.as_ptr(), current_line.len() as i32, &mut size);
            current_width = size.cx;
        } else {
            current_line = test_line;
            current_width = size.cx;
        }
    }
    if !current_line.is_empty() {
        let spaces = current_line.iter().filter(|&&c| c == 32).count() as i32;
        lines.push(TextLine { text: current_line, width: current_width, spaces });
    }
    lines
}

// ========================================================
// SELECTION OVERLAY
// ========================================================

pub fn show_selection_overlay() {
    unsafe {
        IS_DRAGGING = false;
        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = to_wide("SnippingOverlay");
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(selection_wnd_proc);
        wc.hInstance = instance;
        wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_CROSS);
        wc.lpszClassName = class_name.as_ptr();
        wc.hbrBackground = CreateSolidBrush(0x00FF00FF); 
        RegisterClassW(&wc);

        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);

        let hwnd = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW, class_name.as_ptr(), to_wide("Snipping").as_ptr(), WS_POPUP, x, y, w, h, std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null_mut());
        SetLayeredWindowAttributes(hwnd, 0x00FF00FF, 100, LWA_ALPHA | LWA_COLORKEY);
        ShowWindow(hwnd, SW_SHOW);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) != 0 {
            TranslateMessage(&msg); DispatchMessageW(&msg);
            if msg.message == WM_CLOSE { break; }
        }
        UnregisterClassW(class_name.as_ptr(), instance);
    }
}

unsafe extern "system" fn selection_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_KEYDOWN => { if wparam == VK_ESCAPE as usize { PostMessageW(hwnd, WM_CLOSE, 0, 0); } 0 }
        WM_LBUTTONDOWN => {
            IS_DRAGGING = true; GetCursorPos(std::ptr::addr_of_mut!(START_POS)); CURR_POS = START_POS;
            SetCapture(hwnd); InvalidateRect(hwnd, std::ptr::null(), 0); 0
        }
        WM_MOUSEMOVE => {
            if IS_DRAGGING { GetCursorPos(std::ptr::addr_of_mut!(CURR_POS)); InvalidateRect(hwnd, std::ptr::null(), 0); } 0
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
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mem_dc = CreateCompatibleDC(hdc);
            let w = GetSystemMetrics(SM_CXVIRTUALSCREEN); let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            let mem_bm = CreateCompatibleBitmap(hdc, w, h); SelectObject(mem_dc, mem_bm as *mut winapi::ctypes::c_void);
            let bg_brush = CreateSolidBrush(0x00000000); FillRect(mem_dc, &RECT{left:0,top:0,right:w,bottom:h}, bg_brush); DeleteObject(bg_brush as *mut winapi::ctypes::c_void);
            if IS_DRAGGING {
                let r = RECT { left: (START_POS.x.min(CURR_POS.x)) - GetSystemMetrics(SM_XVIRTUALSCREEN), top: (START_POS.y.min(CURR_POS.y)) - GetSystemMetrics(SM_YVIRTUALSCREEN), right: (START_POS.x.max(CURR_POS.x)) - GetSystemMetrics(SM_XVIRTUALSCREEN), bottom: (START_POS.y.max(CURR_POS.y)) - GetSystemMetrics(SM_YVIRTUALSCREEN) };
                let k_br = CreateSolidBrush(0x00FF00FF); FillRect(mem_dc, &r, k_br); DeleteObject(k_br as *mut winapi::ctypes::c_void);
                let b_br = CreateSolidBrush(0x00FFFFFF); FrameRect(mem_dc, &r, b_br); DeleteObject(b_br as *mut winapi::ctypes::c_void);
            }
            BitBlt(hdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
            DeleteObject(mem_bm as *mut winapi::ctypes::c_void); DeleteDC(mem_dc); EndPaint(hwnd, &mut ps); 0
        }
        WM_DESTROY => { PostQuitMessage(0); 0 }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn process_region(region: config::Region) {
    let mut config = config::Config::load(); config.fixed_regions.clear(); config.fixed_regions.push(region); config.save().unwrap();
}

// ========================================================
// RESULT WINDOW
// ========================================================

static REGISTER_RESULT_CLASS: Once = Once::new();

pub fn show_result_window(target_rect: RECT, text: String, duration_ms: u32) {
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
        let hfont = CreateFontW(20, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 2, 0, to_wide("Roboto").as_ptr());
        SelectObject(hdc_screen, hfont as *mut winapi::ctypes::c_void);
        
        let padding = 10; 
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
        let mut y = target_rect.top - height - 10; 
        if y < 0 { y = target_rect.top + 10; }

        // --- [FIX DEADLOCK] ---
        // 1. Copy danh sách HWND cần di chuyển ra ngoài
        // 2. Lọc bỏ các HWND đã chết (IsWindow == 0)
        // 3. Nhả khóa mutex NGAY LẬP TỨC để tránh deadlock với luồng WM_DESTROY của cửa sổ cũ
        let valid_hwnds: Vec<HWND> = {
            let mut list = OVERLAY_LIST.lock().unwrap();
            // Xóa cửa sổ chết khỏi danh sách toàn cục luôn cho sạch
            list.retain(|&h| IsWindow(h as HWND) != 0);
            // Tạo bản copy để dùng
            list.iter().map(|&h| h as HWND).collect()
        };

        // Bây giờ mới thực hiện MoveWindow mà không giữ khóa
        for hwnd in valid_hwnds {
            let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            if GetWindowRect(hwnd, &mut rect) != 0 {
                MoveWindow(hwnd, rect.left, rect.top - height - 10, rect.right - rect.left, rect.bottom - rect.top, 1);
            }
        }
        // ----------------------

        let font_data = include_bytes!("roboto.ttf");
        let _ = AddFontMemResourceEx(font_data.as_ptr() as *mut winapi::ctypes::c_void, font_data.len() as u32, std::ptr::null_mut(), &mut 0);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            class_name.as_ptr(), to_wide(&text).as_ptr(), WS_POPUP | WS_VISIBLE,
            x, y, width, height, std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null_mut()
        );

        if !hwnd.is_null() {
            {
                let mut list = OVERLAY_LIST.lock().unwrap();
                list.push(hwnd as usize);
            }
            SetLayeredWindowAttributes(hwnd, 0x00FF00FF, 230, LWA_ALPHA | LWA_COLORKEY);
            SetTimer(hwnd, 1, duration_ms, None);

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) != 0 {
                TranslateMessage(&msg); DispatchMessageW(&msg);
                if msg.message == WM_CLOSE { break; }
            }
        }
    }
}

unsafe extern "system" fn result_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rect: RECT = std::mem::zeroed(); GetClientRect(hwnd, &mut rect);
            let mem_dc = CreateCompatibleDC(hdc);
            let mem_bm = CreateCompatibleBitmap(hdc, rect.right, rect.bottom); SelectObject(mem_dc, mem_bm as *mut winapi::ctypes::c_void);
            let k_br = CreateSolidBrush(0x00FF00FF); FillRect(mem_dc, &rect, k_br); DeleteObject(k_br as *mut winapi::ctypes::c_void);

            let particles_opt = {
                let map_mutex = ANIMATION_MAP.get_or_init(|| Mutex::new(HashMap::new()));
                let map = map_mutex.lock().unwrap();
                map.get(&(hwnd as usize)).cloned()
            };

            if let Some(particles) = particles_opt {
                for p in particles {
                    let p_rect = RECT { left: p.x as i32, top: p.y as i32, right: p.x as i32 + p.size, bottom: p.y as i32 + p.size };
                    let br = CreateSolidBrush(p.color); FillRect(mem_dc, &p_rect, br); DeleteObject(br as *mut winapi::ctypes::c_void);
                }
            } else {
                let hrgn = CreateRoundRectRgn(0, 0, rect.right, rect.bottom, 8, 8);
                let bg_br = CreateSolidBrush(0x00000000); FillRgn(mem_dc, hrgn, bg_br); DeleteObject(bg_br as *mut winapi::ctypes::c_void);
                {
                    let map_mutex = HOVER_MAP.get_or_init(|| Mutex::new(HashMap::new()));
                    let map = map_mutex.lock().unwrap();
                    if *map.get(&(hwnd as usize)).unwrap_or(&false) {
                        let g_br = CreateSolidBrush(0x0000FF00); FrameRgn(mem_dc, hrgn, g_br, 2, 2); DeleteObject(g_br as *mut winapi::ctypes::c_void);
                    }
                }
                SetBkMode(mem_dc, 1); SetTextColor(mem_dc, 0x00FFFFFF);
                let hfont = CreateFontW(20, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 2, 0, to_wide("Roboto").as_ptr());
                let old_font = SelectObject(mem_dc, hfont as *mut winapi::ctypes::c_void);

                let len = GetWindowTextLengthW(hwnd) + 1; let mut buf = vec![0u16; len as usize]; GetWindowTextW(hwnd, buf.as_mut_ptr(), len);
                let v_len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                
                // Justified Text
                let lines = break_text_into_lines(mem_dc, &buf[0..v_len], (rect.right - rect.left) - 20);
                let mut sz: SIZE = std::mem::zeroed(); GetTextExtentPoint32W(mem_dc, to_wide("A").as_ptr(), 1, &mut sz);
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
                SelectObject(mem_dc, old_font); DeleteObject(hfont as *mut winapi::ctypes::c_void); DeleteObject(hrgn as *mut winapi::ctypes::c_void);
            }
            BitBlt(hdc, 0, 0, rect.right, rect.bottom, mem_dc, 0, 0, SRCCOPY);
            DeleteObject(mem_bm as *mut winapi::ctypes::c_void); DeleteDC(mem_dc); EndPaint(hwnd, &mut ps); 0
        }
        WM_LBUTTONUP => {
             let mut r = RECT {left:0,top:0,right:0,bottom:0}; GetClientRect(hwnd, &mut r);
             let mut p = Vec::new(); let mut s = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos();
             for _ in 0..150 {
                 let c = if simple_rng(&mut s)%100 < 80 { 0 } else if simple_rng(&mut s)%100 < 90 { 0x0000FF00 } else { 0x00FFFFFF };
                 p.push(Particle{x: get_random_f32(&mut s,0.0,r.right as f32), y: get_random_f32(&mut s,0.0,r.bottom as f32), vx: get_random_f32(&mut s,-8.0,8.0), vy: get_random_f32(&mut s,-8.0,5.0), size: (simple_rng(&mut s)%5+2) as i32, color: c});
             }
             { let map_mutex = ANIMATION_MAP.get_or_init(|| Mutex::new(HashMap::new())); map_mutex.lock().unwrap().insert(hwnd as usize, p); }
             KillTimer(hwnd, 1); SetTimer(hwnd, 3, 16, None); 0
        }
        WM_TIMER => {
            if wparam == 1 { KillTimer(hwnd, 1); PostMessageW(hwnd, WM_CLOSE, 0, 0); }
            else if wparam == 3 {
                let mut close = false;
                {
                    let map_mutex = ANIMATION_MAP.get_or_init(|| Mutex::new(HashMap::new()));
                    let mut map = map_mutex.lock().unwrap();
                    if let Some(p) = map.get_mut(&(hwnd as usize)) {
                        let mut cnt = 0; let mut r = RECT{left:0,top:0,right:0,bottom:0}; GetClientRect(hwnd, &mut r);
                        for i in p.iter_mut() { i.x+=i.vx; i.y+=i.vy; i.vy+=0.5; if i.y < (r.bottom+100) as f32 { cnt+=1; } }
                        if cnt == 0 { close = true; }
                    } else { close = true; }
                }
                if close { KillTimer(hwnd, 3); PostMessageW(hwnd, WM_CLOSE, 0, 0); } else { InvalidateRect(hwnd, std::ptr::null(), 0); }
            } 0
        }
        WM_DESTROY => {
            { let mut map = HOVER_MAP.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap(); map.remove(&(hwnd as usize)); }
            { let mut map = ANIMATION_MAP.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap(); map.remove(&(hwnd as usize)); }
            // Quan trọng: Dọn dẹp OVERLAY_LIST ở đây
            { let mut list = OVERLAY_LIST.lock().unwrap(); list.retain(|&h| h != hwnd as usize); }
            PostQuitMessage(0); 0
        }
        WM_KEYDOWN => { if wparam == VK_ESCAPE as usize { PostMessageW(hwnd, WM_CLOSE, 0, 0); } 0 }
        WM_SETCURSOR => { SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_HAND)); 1 }
        WM_MOUSEMOVE => {
             let anim = { ANIMATION_MAP.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap().contains_key(&(hwnd as usize)) };
             if !anim {
                 let map_mutex = HOVER_MAP.get_or_init(|| Mutex::new(HashMap::new()));
                 let mut map = map_mutex.lock().unwrap();
                 if !*map.get(&(hwnd as usize)).unwrap_or(&false) {
                     map.insert(hwnd as usize, true);
                     let mut t = TRACKMOUSEEVENT{cbSize:std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags:TME_LEAVE, hwndTrack:hwnd, dwHoverTime:0};
                     unsafe { TrackMouseEvent(&mut t); InvalidateRect(hwnd, std::ptr::null(), 0); }
                 }
             } 0
        }
        WM_MOUSELEAVE => {
             let anim = { ANIMATION_MAP.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap().contains_key(&(hwnd as usize)) };
             if !anim {
                 HOVER_MAP.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap().insert(hwnd as usize, false);
                 unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
             } 0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}