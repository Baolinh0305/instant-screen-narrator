#![windows_subsystem = "windows"]

mod config;
mod capture;
mod translation;
mod tts;
mod overlay;
mod key_utils;
mod ui;

use crate::overlay::{show_result_window, show_result_window_internal};
use crate::ui::UiRenderer; 
use eframe::egui;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{self, Receiver, Sender};
use winapi::shared::windef::{RECT, HWND};
use winapi::shared::minwindef::UINT;
use winapi::um::winuser::*;
use winapi::um::libloaderapi::GetModuleHandleW;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use image::{self, GenericImageView};
use std::fs;
use webbrowser;
use arboard::Clipboard;
use std::sync::Arc;
use std::time::Duration;
use rand;

use tray_icon::{TrayIconBuilder, TrayIcon, TrayIconEvent, MouseButton};
use tray_icon::menu::{Menu, MenuItem, MenuEvent};

const DEFAULT_ARROW: &[u8] = include_bytes!("arrow.png");

// Constants
const APP_NAME: &str = "Instant Screen Narrator";
const THREAD_SLEEP_MS: u64 = 500;
const REPAINT_INTERVAL_MS: u64 = 250;
const SELECT_COOLDOWN_MS: u64 = 1000;
const BINDING_SLEEP_MS: u64 = 200;
const VK_MIN: i32 = 8;
const VK_MAX: i32 = 255;
const KEY_STATE_MASK: u16 = 0x8000;
const TIMER_INTERVAL_MS: u32 = 500;
const MISS_COUNTER_THRESHOLD: i32 = 5;
const SUCCESS_DISPLAY_DURATION_SECS: f32 = 1.0;
const CONFIG_SYNC_INTERVAL_SECS: u64 = 1;
const TTS_SPEED_MIN: f32 = 0.5;
const TTS_SPEED_MAX: f32 = 2.0;
const ARROW_CHECK_INTERVAL_MIN: f32 = 0.02;
const ARROW_CHECK_INTERVAL_MAX: f32 = 0.2;
const DEFAULT_ARROW_CHECK_INTERVAL: f32 = 0.02;
const FONT_SIZE_MIN: u32 = 10;
const FONT_SIZE_MAX: u32 = 72;
const PIXELS_PER_POINT: f32 = 1.2;

// WWM Region Ratios (16:9)
/* WWM Region Ratios (16:9) - Updated based on user coordinates
Region 1: Normal Text (423, 925) -> (1496, 1037)
W = 1496 - 423 = 1073
H = 1037 - 925 = 112 */
const WWM_TEXT_REGION_X_RATIO: f32 = 423.0 / 1920.0;
const WWM_TEXT_REGION_Y_RATIO: f32 = 925.0 / 1080.0;
const WWM_TEXT_REGION_W_RATIO: f32 = 1073.0 / 1920.0;
const WWM_TEXT_REGION_H_RATIO: f32 = 112.0 / 1080.0;

/* Region 2: With Name (423, 868) -> (1496, 1037)
W = 1496 - 423 = 1073
H = 1037 - 868 = 169 */
const WWM_NAME_REGION_X_RATIO: f32 = 423.0 / 1920.0;
const WWM_NAME_REGION_Y_RATIO: f32 = 868.0 / 1080.0;
const WWM_NAME_REGION_W_RATIO: f32 = 1073.0 / 1920.0;
const WWM_NAME_REGION_H_RATIO: f32 = 169.0 / 1080.0;

/* Region 3: Arrow (937, 1049) -> (979, 1079)
W = 979 - 937 = 42
H = 1079 - 1049 = 30 */
const WWM_ARROW_REGION_X_RATIO: f32 = 937.0 / 1920.0;
const WWM_ARROW_REGION_Y_RATIO: f32 = 1049.0 / 1080.0;
const WWM_ARROW_REGION_W_RATIO: f32 = 42.0 / 1920.0;
const WWM_ARROW_REGION_H_RATIO: f32 = 30.0 / 1080.0;

// Padding (Giữ nguyên hoặc chỉnh về 0 nếu muốn chính xác tuyệt đối theo tọa độ bạn đưa)
const WWM_REGION_PADDING: i32 = 0;
const WWM_REGION_EXTRA_WIDTH: u32 = 0;
const WWM_REGION_EXTRA_HEIGHT: u32 = 0;

static LAST_SELECT: AtomicU64 = AtomicU64::new(0);
static OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);
static LISTENING_PAUSED: AtomicBool = AtomicBool::new(false);
static AUTO_TRANSLATE_ENABLED: AtomicBool = AtomicBool::new(false);
static GROQ_REMAINING: AtomicI32 = AtomicI32::new(-1);
static HOTKEYS_NEED_UPDATE: AtomicBool = AtomicBool::new(false);
static IS_BINDING_MODE: AtomicBool = AtomicBool::new(false);

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

fn force_show_window_at_position() {
    unsafe {
        let window_name = to_wide(APP_NAME);
        let hwnd = FindWindowW(std::ptr::null(), window_name.as_ptr());
        
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
            // ĐÃ XÓA ĐOẠN SetWindowPos GÂY LỖI KÉO DÀI CỬA SỔ
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum BindingTarget {
    Translate,
    Select,
    Instant,
    Auto,
    ToggleAuto,
    AuxSelect(usize),
    AuxTranslate(usize),
}

enum AppSignal {
    Show,
}

// Cải tiến ReaderSignal để hỗ trợ Pre-load và Index Tracking
enum ReaderSignal {
    PlaybackFinished(usize),      // SỬA: Trả về index của câu vừa đọc xong
    DownloadFinished(Vec<u8>, usize), // Tải xong câu (data, index)
    Error,
}

pub struct MainApp {
    pub ui_state: ui::UiState,
    pub config_state: ui::ConfigState,
    pub hotkey_state: ui::HotkeyState,
    pub wwm_state: ui::WwmState,

    pub binding_target: Option<BindingTarget>,
    pub is_paused: bool,
    pub listener_spawned: bool,
    pub arrow_texture: Option<egui::TextureHandle>,
    pub last_config_sync: std::time::Instant,

    pub reader_rx: Receiver<ReaderSignal>,
    pub reader_tx: Sender<ReaderSignal>,
    
    // --- State cho Reader ---
    pub next_audio_buffer: Option<(Vec<u8>, usize)>, // SỬA: Lưu thêm index để kiểm tra
    pub is_downloading_next: bool,          
    pub is_playing_audio: bool,             

    _tray_icon: TrayIcon,
    rx_signal: Receiver<AppSignal>,
}

impl MainApp {
    fn new(cc: &eframe::CreationContext<'_>, tray_icon: TrayIcon, rx: Receiver<AppSignal>) -> Self {
        let mut config = config::Config::load();
        if config.groq_api_keys.is_empty() {
            config.groq_api_keys.push(String::new());
        }

        overlay::set_font_size(config.overlay_font_size);

        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(THREAD_SLEEP_MS));
            force_show_window_at_position();
        });

        let ctx_clone = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(REPAINT_INTERVAL_MS));
                ctx_clone.request_repaint();
            }
        });

        let (r_tx, r_rx) = std::sync::mpsc::channel();

        let mut app = Self {
            ui_state: ui::UiState::new(),
            config_state: ui::ConfigState::new(config.clone()),
            hotkey_state: ui::HotkeyState::new(&config),
            wwm_state: ui::WwmState::new(),
            binding_target: None,
            is_paused: false,
            listener_spawned: false,
            arrow_texture: None,
            last_config_sync: std::time::Instant::now(),

            reader_rx: r_rx,
            reader_tx: r_tx,
            next_audio_buffer: None,
            is_downloading_next: false,
            is_playing_audio: false,

            _tray_icon: tray_icon,
            rx_signal: rx,
        };

        app.start_service();
        app.listener_spawned = true;
        LISTENING_PAUSED.store(false, Ordering::Relaxed);

        app
    }

    fn configure_style(&self, ctx: &egui::Context) {
        if self.config_state.config.is_dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(6.0, 6.0);

        // --- THAY ĐỔI Ở ĐÂY: Chỉnh margin dưới về 0 ---
        style.spacing.window_margin = egui::Margin {
            left: 8.0,
            right: 8.0,
            top: 8.0,
            bottom: 0.0, // Để sát cạnh dưới
        };

        style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(4.0);
        style.visuals.widgets.inactive.rounding = egui::Rounding::same(4.0);
        style.visuals.widgets.hovered.rounding = egui::Rounding::same(4.0);
        style.visuals.widgets.active.rounding = egui::Rounding::same(4.0);

        let border_color = egui::Color32::from_rgb(100, 149, 237);
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border_color);
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, border_color);
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(2.0, border_color);
        ctx.set_style(style);
    }

    fn copy_to_clipboard(text: &str) {
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(text);
        }
    }

    async fn translate_regions(
        mut config: config::Config,
        regions: Vec<config::Region>,
        tx: Sender<(String, bool, f32, bool, u64)>,
        should_copy: bool,
    ) {
        // 1. Chuẩn bị Prompt
        let mut final_prompt = config.current_prompt.clone();
        
        // Nếu bật "Copy bản gốc", ta ép buộc LLM trả về format dạng: <Original> ||||| <Translation>
        // Điều này cho phép ta lấy được text gốc để copy và text dịch để đọc chỉ trong 1 request.
        let use_split_mode = should_copy && config.copy_original;
        
        if use_split_mode {
            final_prompt = format!("{}\n\nSPECIAL INSTRUCTION: You must output the result in two parts. Part 1 is the raw original text extracted from the image. Part 2 is the result of the prompt above. Separate them with the delimiter '|||||'. Do not add any other text. Format: [Original Text] ||||| [Result Text]", final_prompt);
        }

        let mut final_text_to_show = String::new(); // Dùng để hiển thị/đọc
        let mut final_text_to_copy = String::new(); // Dùng để copy (nếu split mode)
        
        overlay::set_font_size(config.overlay_font_size);

        for region in &regions {
            let image_bytes = capture::capture_image(region).unwrap_or_default();
            if !image_bytes.is_empty() {
                let mut attempts = 0;
                let max_attempts = if config.selected_api == "groq" { config.groq_api_keys.len() } else { 1 };
                let mut success = false;

                while attempts < max_attempts {
                    let api_key = if config.selected_api == "gemini" {
                        config.gemini_api_key.clone()
                    } else {
                        config.get_current_groq_key()
                    };

                    if api_key.is_empty() {
                        final_text_to_show.push_str("(Chưa nhập Key) ");
                        break;
                    }

                    // Gọi API với prompt đã chỉnh sửa
                    match translation::translate_from_image(&config.selected_api, &api_key, &final_prompt, &image_bytes).await {
                        Ok(result) => {
                            // Xử lý kết quả trả về
                            if use_split_mode {
                                let parts: Vec<&str> = result.text.split("|||||").collect();
                                if parts.len() >= 2 {
                                    final_text_to_copy.push_str(parts[0].trim());
                                    final_text_to_show.push_str(parts[1].trim());
                                } else {
                                    // Fallback nếu LLM không tuân thủ format (hiếm khi xảy ra)
                                    final_text_to_copy.push_str(&result.text);
                                    final_text_to_show.push_str(&result.text);
                                }
                                final_text_to_copy.push(' ');
                            } else {
                                final_text_to_show.push_str(&result.text);
                            }

                            if let Some(rem) = result.remaining_requests {
                                GROQ_REMAINING.store(rem, Ordering::Relaxed);
                            }
                            success = true;
                            break;
                        },
                        Err(translation::TranslationError::RateLimitExceeded) => {
                            if config.selected_api == "groq" && config.groq_api_keys.len() > 1 {
                                config.active_groq_index = (config.active_groq_index + 1) % config.groq_api_keys.len();
                                attempts += 1;
                                continue;
                            } else {
                                final_text_to_show.push_str("(Hết lượt Request & hết Key dự phòng) ");
                                break;
                            }
                        },
                        Err(translation::TranslationError::Other(e)) => {
                            let error_msg = format!("Lỗi: {} ", e);
                            final_text_to_show.push_str(&error_msg);
                            break;
                        }
                    }
                }
                if !success && final_text_to_show.is_empty() { final_text_to_show.push_str("... "); }
                final_text_to_show.push(' ');
            }
        }

        let cleaned_show = final_text_to_show.trim().to_string();
        let cleaned_copy = final_text_to_copy.trim().to_string();

        if !cleaned_show.is_empty() {
            if should_copy {
                if config.copy_original && !cleaned_copy.is_empty() {
                    Self::copy_to_clipboard(&cleaned_copy);
                } else {
                    Self::copy_to_clipboard(&cleaned_show);
                }
            }

            let req_id = rand::random::<u64>();
            let _ = tx.send((cleaned_show.clone(), config.split_tts, config.speed, config.use_tts, req_id));
            if config.show_overlay {
                if let Some(region) = regions.first() {
                    let rect = RECT { left: region.x, top: region.y, right: region.x + region.width as i32, bottom: region.y + region.height as i32 };
                    let duration_ms = (cleaned_show.chars().count() as f32 / 10.0 * 1000.0) as u32;
                    let text_final = cleaned_show.clone();
                    let req_id_clone = req_id;
                    std::thread::spawn(move || {
                        // Nếu không update được (do không phải mode auto/không có loading), thì hiện cửa sổ mới
                        if !overlay::update_loading_window(text_final.clone()) {
                              overlay::show_result_window_internal(rect, text_final, duration_ms, false, req_id_clone);
                        }
                    });
                }
            }
        }
    }

    // Helper: Tải audio cho một câu cụ thể (chạy trong thread)
    fn spawn_download(&self, text: String, index: usize) {
        let tx = self.reader_tx.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                match crate::tts::download_audio(text).await {
                    Ok(bytes) => { let _ = tx.send(ReaderSignal::DownloadFinished(bytes, index)); },
                    Err(_) => { let _ = tx.send(ReaderSignal::Error); }
                }
            });
        });
    }

    // Helper: Phát audio từ bytes (chạy trong thread)
    fn spawn_playback(&self, bytes: Vec<u8>, speed: f32, index: usize) { // SỬA: Nhận thêm index
        let tx = self.reader_tx.clone();
        std::thread::spawn(move || {
            if let Err(_) = crate::tts::play_audio_data(bytes, speed) {
                 let _ = tx.send(ReaderSignal::Error);
            }
            let _ = tx.send(ReaderSignal::PlaybackFinished(index)); // SỬA: Trả về index
        });
    }

    fn start_service(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel::<(String, bool, f32, bool, u64)>();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            while let Ok((text, split_tts, speed, use_tts, req_id)) = rx.recv() {
                rt.block_on(async { if let Err(_e) = tts::speak(&text, split_tts, speed, use_tts, req_id).await {} });
            }
        });

        let tx_clone = tx.clone();
        let tx_auto = tx.clone();

        // Thread Auto Translate
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let load_arrow = || -> Vec<u8> {
                let custom_path = config::Config::get_custom_arrow_path();
                if custom_path.exists() { if let Ok(b) = fs::read(custom_path) { return b; } }
                DEFAULT_ARROW.to_vec()
            };
            let arrow_bytes = load_arrow();
            let mut last_found_state = false;
            let mut miss_counter = 0;

            loop {
                let config = config::Config::load();
                let check_interval = config.arrow_check_interval;
                let enabled = AUTO_TRANSLATE_ENABLED.load(Ordering::Relaxed);
                
                if !enabled || arrow_bytes.is_empty() {
                    std::thread::sleep(std::time::Duration::from_millis(THREAD_SLEEP_MS));
                    continue;
                }

                if let Some(arrow_region) = &config.arrow_region {
                    let found = capture::is_template_present(arrow_region, &arrow_bytes);

                    // --- THÊM DÒNG NÀY ĐỂ CẬP NHẬT TRẠNG THÁI DEBUG ---
                    crate::overlay::ARROW_DEBUG_STATE.store(found, Ordering::Relaxed);
                    // --------------------------------------------------

                    if found {
                        miss_counter = 0;
                        if !last_found_state {
                            // --- SỬA Ở ĐÂY: Hiện Loading Overlay ngay lập tức ---
                            if config.show_overlay {
                                if let Some(target_region) = config.fixed_regions.first() {
                                    let rect = RECT {
                                        left: target_region.x,
                                        top: target_region.y,
                                        right: target_region.x + target_region.width as i32,
                                        bottom: target_region.y + target_region.height as i32
                                    };
                                    // Hiện cửa sổ loading "..."
                                    std::thread::spawn(move || { overlay::show_loading_window(rect); });
                                }
                            }
                            // ----------------------------------------------------

                            let tx_inner = tx_auto.clone();
                            let should_copy = config.auto_copy && !config.copy_instant_only;
                            rt.block_on(async { Self::translate_regions(config.clone(), config.fixed_regions.clone(), tx_inner, should_copy).await; });
                            last_found_state = true;
                        }
                    } else {
                        if last_found_state {
                            miss_counter += 1;
                            if miss_counter > MISS_COUNTER_THRESHOLD { last_found_state = false; miss_counter = 0; }
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs_f32(check_interval)); 
            }
        });

        // --- WINDOWS HOTKEY LISTENER THREAD ---
        std::thread::spawn(move || {
            unsafe {
                let instance = GetModuleHandleW(std::ptr::null());
                let class_name = to_wide("HotkeyListener");
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(DefWindowProcW),
                    hInstance: instance,
                    lpszClassName: class_name.as_ptr(),
                    ..std::mem::zeroed()
                };
                RegisterClassW(&wc);
                let hwnd = CreateWindowExW(0, class_name.as_ptr(), to_wide("Listener").as_ptr(), 0, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null_mut());

                let register_keys = |hwnd: HWND| {
                    let cfg = config::Config::load();
                    // Reset all
                    for i in 1..500 { UnregisterHotKey(hwnd, i); }

                    let k1 = crate::key_utils::get_vk_from_name(&cfg.hotkey_translate);
                    let k2 = crate::key_utils::get_vk_from_name(&cfg.hotkey_select);
                    let k3 = crate::key_utils::get_vk_from_name(&cfg.hotkey_instant);
                    let k4 = crate::key_utils::get_vk_from_name(&cfg.hotkey_auto);
                    let k5 = crate::key_utils::get_vk_from_name(&cfg.hotkey_toggle_auto);
                    if k1 > 0 { RegisterHotKey(hwnd, 1, 0, k1 as UINT); }
                    if k2 > 0 { RegisterHotKey(hwnd, 2, 0, k2 as UINT); }
                    if k3 > 0 { RegisterHotKey(hwnd, 3, 0, k3 as UINT); }
                    if k4 > 0 { RegisterHotKey(hwnd, 4, 0, k4 as UINT); }
                    if k5 > 0 { RegisterHotKey(hwnd, 5, 0, k5 as UINT); }

                    // --- AUX REGIONS KEYS ---
                    for (i, aux) in cfg.aux_regions.iter().enumerate() {
                        let k_sel = crate::key_utils::get_vk_from_name(&aux.hotkey_select);
                        let k_trans = crate::key_utils::get_vk_from_name(&aux.hotkey_translate);
                        if k_sel > 0 { RegisterHotKey(hwnd, 100 + i as i32, 0, k_sel as UINT); }
                        if k_trans > 0 { RegisterHotKey(hwnd, 200 + i as i32, 0, k_trans as UINT); }
                    }
                };

                register_keys(hwnd);
                SetTimer(hwnd, 1, TIMER_INTERVAL_MS, None);

                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) != 0 {
                    if msg.message == WM_TIMER {
                        if HOTKEYS_NEED_UPDATE.load(Ordering::Relaxed) {
                            register_keys(hwnd);
                            HOTKEYS_NEED_UPDATE.store(false, Ordering::Relaxed);
                        }
                    } else if msg.message == WM_HOTKEY {
                        if !LISTENING_PAUSED.load(Ordering::Relaxed) {
                            let id = msg.wParam as i32;
                            let config = config::Config::load();
                            
                            // --- MAIN KEYS ---
                            if id == 1 { // Translate
                                let tx = tx_clone.clone();
                                let rt = tokio::runtime::Runtime::new().unwrap();
                                let should_copy = config.auto_copy && !config.copy_instant_only;
                                std::thread::spawn(move || { rt.block_on(async { Self::translate_regions(config.clone(), config.fixed_regions.clone(), tx, should_copy).await; }); });
                            } else if id == 2 { // Select
                                if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
                                    if now - LAST_SELECT.load(Ordering::Relaxed) > SELECT_COOLDOWN_MS {
                                        LAST_SELECT.store(now, Ordering::Relaxed); OVERLAY_ACTIVE.store(true, Ordering::Relaxed);
                                        overlay::set_selection_mode(0); std::thread::spawn(|| { overlay::show_selection_overlay(); OVERLAY_ACTIVE.store(false, Ordering::Relaxed); });
                                    }
                                }
                            } else if id == 3 { // Instant
                                if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                                    OVERLAY_ACTIVE.store(true, Ordering::Relaxed); let tx = tx_clone.clone();
                                    overlay::set_selection_mode(2);
                                    std::thread::spawn(move || {
                                        overlay::show_selection_overlay(); OVERLAY_ACTIVE.store(false, Ordering::Relaxed);
                                        let config = config::Config::load();
                                        if let Some(region) = config.instant_region.clone() {
                                            let rt = tokio::runtime::Runtime::new().unwrap();
                                            let should_copy = config.auto_copy;
                                            rt.block_on(async { Self::translate_regions(config, vec![region], tx, should_copy).await; });
                                        }
                                    });
                                }
                            } else if id == 4 { // Auto
                                if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
                                    if now - LAST_SELECT.load(Ordering::Relaxed) > SELECT_COOLDOWN_MS {
                                        LAST_SELECT.store(now, Ordering::Relaxed); OVERLAY_ACTIVE.store(true, Ordering::Relaxed);
                                        overlay::set_selection_mode(1); std::thread::spawn(|| { overlay::show_selection_overlay(); OVERLAY_ACTIVE.store(false, Ordering::Relaxed); });
                                    }
                                }
                            } else if id == 5 { // Toggle Auto
                                let current_state = AUTO_TRANSLATE_ENABLED.load(Ordering::Relaxed);
                                let new_state = !current_state;
                                AUTO_TRANSLATE_ENABLED.store(new_state, Ordering::Relaxed);
                                // Gọi hàm thông báo
                                show_toggle_notification(new_state);
                            }
                            // --- AUX REGIONS KEYS ---
                            else if id >= 100 && id < 200 { // Select Aux
                                let idx = (id - 100) as usize;
                                if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                                    OVERLAY_ACTIVE.store(true, Ordering::Relaxed);
                                    overlay::set_selection_mode((100 + idx) as u8);
                                    std::thread::spawn(|| { overlay::show_selection_overlay(); OVERLAY_ACTIVE.store(false, Ordering::Relaxed); });
                                }
                            } else if id >= 200 && id < 300 { // Translate Aux
                                let idx = (id - 200) as usize;
                                if idx < config.aux_regions.len() {
                                    if let Some(region) = &config.aux_regions[idx].region {
                                        let tx = tx_clone.clone();
                                        let rt = tokio::runtime::Runtime::new().unwrap();
                                        let should_copy = config.auto_copy && !config.copy_instant_only;
                                        let reg_clone = region.clone();
                                        std::thread::spawn(move || { rt.block_on(async { Self::translate_regions(config.clone(), vec![reg_clone], tx, should_copy).await; }); });
                                    }
                                }
                            }
                        }
                    }
                    TranslateMessage(&msg); DispatchMessageW(&msg);
                }
            }
        });
    }
}

impl eframe::App for MainApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(signal) = self.rx_signal.try_recv() {
            match signal {
                AppSignal::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                }
            }
        }

        // --- READER LOGIC (QUEUEING & PREFETCH) ---
        // SỬA: Kiểm tra buffer có đúng là buffer của câu hiện tại không
        let current_idx = self.ui_state.reader.current_index;
        let has_buffer_for_current = self.next_audio_buffer.as_ref().map_or(false, |(_, i)| *i == current_idx);

        if self.ui_state.reader.is_open && self.ui_state.reader.is_playing && !self.is_playing_audio && !self.ui_state.reader.chunks.is_empty() {
             if has_buffer_for_current {
                 // Có buffer ĐÚNG của câu hiện tại -> Phát ngay
                 if let Some((bytes, _)) = self.next_audio_buffer.take() {
                     let speed = self.config_state.config.speed;
                     self.spawn_playback(bytes, speed, current_idx);
                     self.is_playing_audio = true;
                     self.is_downloading_next = false;
                     // Trigger pre-fetch câu tiếp theo
                     if current_idx + 1 < self.ui_state.reader.chunks.len() {
                         let next_text = self.ui_state.reader.chunks[current_idx + 1].clone();
                         self.spawn_download(next_text, current_idx + 1);
                         self.is_downloading_next = true;
                     }
                 }
             } else if !self.is_downloading_next {
                 // Buffer rỗng hoặc sai index (do seek hoặc pause lâu) -> Tải câu hiện tại
                 // Chỉ tải nếu chưa có tiến trình tải nào đang chạy
                 if current_idx < self.ui_state.reader.chunks.len() {
                     let text = self.ui_state.reader.chunks[current_idx].clone();
                     self.spawn_download(text, current_idx);
                     self.is_downloading_next = true;
                 }
             }
        }

        // 2. Xử lý signals từ thread âm thanh
        while let Ok(sig) = self.reader_rx.try_recv() {
            match sig {
                ReaderSignal::DownloadFinished(bytes, index) => {
                    // Nếu dữ liệu tải về là của câu hiện tại (đang cần phát)
                    if index == self.ui_state.reader.current_index && !self.is_playing_audio {
                        // Phát ngay
                        let speed = self.config_state.config.speed;
                        self.spawn_playback(bytes, speed, index);
                        self.is_playing_audio = true;
                        self.is_downloading_next = false;
                        // Tải trước câu tiếp theo
                        if self.ui_state.reader.current_index + 1 < self.ui_state.reader.chunks.len() {
                             let next_text = self.ui_state.reader.chunks[self.ui_state.reader.current_index + 1].clone();
                             self.spawn_download(next_text, self.ui_state.reader.current_index + 1);
                             self.is_downloading_next = true;
                        }
                    } 
                    // Nếu dữ liệu tải về là của câu tiếp theo (Pre-fetch)
                    else if index == self.ui_state.reader.current_index + 1 {
                        self.next_audio_buffer = Some((bytes, index));
                        self.is_downloading_next = false;
                    }
                    // Nếu index không khớp (do người dùng click nhảy câu khác), ta bỏ qua dữ liệu này
                },
                ReaderSignal::PlaybackFinished(finished_index) => {
                    self.is_playing_audio = false;

                    // SỬA: Chỉ tự động chuyển câu nếu câu vừa đọc xong TRÙNG với câu hiện tại
                    // Nếu không trùng (do người dùng click câu khác), thì không được nhảy số.
                    if self.ui_state.reader.is_playing && finished_index == self.ui_state.reader.current_index {
                         self.ui_state.reader.current_index += 1;
                         
                         if self.ui_state.reader.current_index >= self.ui_state.reader.chunks.len() {
                             self.ui_state.reader.is_playing = false;
                             self.ui_state.reader.current_index = 0;
                             self.next_audio_buffer = None;
                         } 
                         // Reset cờ downloading để vòng lặp biết đường xử lý
                         self.is_downloading_next = false; 
                    }
                },
                ReaderSignal::Error => {
                    self.ui_state.reader.is_playing = false;
                    self.is_playing_audio = false;
                    self.is_downloading_next = false;
                }
            }
        }

        // -------------------------

        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert("roboto".to_owned(), egui::FontData::from_static(include_bytes!("roboto.ttf")));
        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "roboto".to_owned());
        fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap().push("roboto".to_owned());
        ctx.set_fonts(fonts);
        ctx.set_pixels_per_point(PIXELS_PER_POINT);

        self.configure_style(ctx);

        if self.binding_target.is_some() {
            self.check_key_binding();
            ctx.request_repaint();
        }

        // SYNC UI: Đảm bảo nút "Bật Tự Động Dịch" hiển thị đúng theo biến Global
        // Nếu dùng phím tắt để tắt/bật thì nút trên giao diện phải đổi màu theo
        self.wwm_state.auto_translate_active = AUTO_TRANSLATE_ENABLED.load(Ordering::Relaxed);

        if self.last_config_sync.elapsed() > Duration::from_secs(CONFIG_SYNC_INTERVAL_SECS) {
            self.sync_config_from_file();
            self.last_config_sync = std::time::Instant::now();
        }

        if self.arrow_texture.is_none() {
              let custom_path = config::Config::get_custom_arrow_path();
              if custom_path.exists() { if let Ok(b) = fs::read(custom_path) { self.load_texture(ctx, &b, true); } }
              else { self.load_texture(ctx, DEFAULT_ARROW, true); }
        }

        // --- LAYOUT CHIA 2 CỘT ---
        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_header(ui);
            
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.columns(2, |columns| {
                    // Cột trái
                    columns[0].vertical(|ui| {
                        self.render_api_section(ui);
                        ui.add_space(10.0);
                        
                        self.render_prompt_section(ui);
                        ui.add_space(10.0);
                        
                        self.render_settings_section(ui);
                    });

                    // Cột phải
                    columns[1].vertical(|ui| {
                        self.render_hotkeys_section(ui);
                        ui.add_space(10.0);
                        
                        self.render_aux_regions_section(ui);
                        ui.add_space(10.0);
                        
                        self.render_wwm_section(ctx, ui);
                    });
                });
            });
        });

        self.render_reader_window(ctx);

        if self.ui_state.show_popup {
            let mut open = true;
            let title = if self.ui_state.popup_text == "gemini" { "Hướng dẫn Gemini" } else { "Hướng dẫn Groq" };
            egui::Window::new(title).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                if self.ui_state.popup_text == "gemini" {
                    ui.heading("Gemini API");
                    ui.horizontal(|ui| { ui.label("1. Vào:"); ui.hyperlink("https://aistudio.google.com/api-keys"); });
                    ui.label("2. Đăng nhập Google -> Create API key"); ui.label("3. Copy key và dán vào tool");
                } else if self.ui_state.popup_text == "groq" {
                    ui.heading("Groq API (Nhanh)");
                    ui.horizontal(|ui| { ui.label("1. Vào:"); ui.hyperlink("https://console.groq.com/keys"); });
                    ui.label("2. Đăng nhập -> Create API Key"); ui.label("3. Copy key và dán vào tool");
                }
                ui.separator();
                ui.vertical_centered(|ui| { if ui.button("Đã hiểu").clicked() { self.ui_state.show_popup = false; } });
            });
            if !open { self.ui_state.show_popup = false; }
        }

        if self.ui_state.show_reset_confirm {
            let mut open = true;
            egui::Window::new("⚠️ Xác nhận Reset").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).open(&mut open).show(ctx, |ui| {
                ui.label("Bạn có muốn giữ lại API Key không?");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("✅ Có (Giữ Key)").clicked() {
                        let saved_gemini = self.config_state.gemini_api_key.clone();
                        let saved_groq = self.config_state.config.groq_api_keys.clone();
                        self.config_state.config = config::Config::default();
                        self.config_state.config.gemini_api_key = saved_gemini;
                        self.config_state.config.groq_api_keys = saved_groq;

                        self.config_state.gemini_api_key = self.config_state.config.gemini_api_key.clone();
                        self.config_state.current_prompt = self.config_state.config.current_prompt.clone();
                        self.config_state.editing_prompt_index = None;
                        self.hotkey_state.hotkey_translate = self.config_state.config.hotkey_translate.clone();
                        self.hotkey_state.hotkey_select = self.config_state.config.hotkey_select.clone();
                        self.hotkey_state.hotkey_instant = self.config_state.config.hotkey_instant.clone();
                        self.hotkey_state.hotkey_auto = self.config_state.config.hotkey_auto.clone();
                        self.config_state.selected_api = self.config_state.config.selected_api.clone();
                        self.config_state.use_tts = self.config_state.config.use_tts;

                        overlay::set_font_size(self.config_state.config.overlay_font_size);
                        HOTKEYS_NEED_UPDATE.store(true, Ordering::Relaxed);
                        let _ = self.config_state.config.save();
                        self.ui_state.show_reset_confirm = false;
                    }
                    if ui.button("❌ Không (Xóa sạch)").clicked() {
                        self.config_state.config = config::Config::default();
                        self.config_state.gemini_api_key = String::new();
                        self.config_state.config.groq_api_keys = vec![String::new()];
                        self.config_state.current_prompt = self.config_state.config.current_prompt.clone();
                        self.config_state.editing_prompt_index = None;
                        self.hotkey_state.hotkey_translate = self.config_state.config.hotkey_translate.clone();
                        self.hotkey_state.hotkey_select = self.config_state.config.hotkey_select.clone();
                        self.hotkey_state.hotkey_instant = self.config_state.config.hotkey_instant.clone();
                        self.hotkey_state.hotkey_auto = self.config_state.config.hotkey_auto.clone();
                        self.config_state.selected_api = self.config_state.config.selected_api.clone();
                        self.config_state.use_tts = self.config_state.config.use_tts;

                        overlay::set_font_size(self.config_state.config.overlay_font_size);
                        HOTKEYS_NEED_UPDATE.store(true, Ordering::Relaxed);
                        let _ = self.config_state.config.save();
                        self.ui_state.show_reset_confirm = false;
                    }
                    if ui.button("🔙 Hủy").clicked() {
                        self.ui_state.show_reset_confirm = false;
                    }
                });
            });
            if !open { self.ui_state.show_reset_confirm = false; }
        }

        if self.ui_state.show_arrow_window {
            let mut open = true;
            egui::Window::new("Mũi tên hiện tại").open(&mut open).collapsible(false).resizable(true).default_size(egui::vec2(300.0, 300.0)).show(ctx, |ui| {
                  ui.label("Đây là hình ảnh mũi tên đang được dùng để nhận diện:");
                  ui.separator();
                if let Some(texture) = &self.arrow_texture {
                    let s = texture.size_vec2() * 2.0;
                    ui.centered_and_justified(|ui| { ui.image((texture.id(), s)); });
                } else { ui.label("Không tìm thấy arrow.png"); }
            });
            if !open { self.ui_state.show_arrow_window = false; }
        }

        if self.ui_state.show_arrow_help {
            let mut open = true;
            egui::Window::new("Giải thích Mũi tên").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label("Đây là hình tam giác hiện ra ở dưới hội thoại mỗi khi nó hiện đầy đủ.");
                ui.label("Phần mềm sẽ dựa vào mũi tên đó để nhận biết khi nào câu thoại đã chạy xong và tiến hành tự động dịch.");
                ui.add_space(10.0);
                if ui.button("Đã hiểu").clicked() { self.ui_state.show_arrow_help = false; }
            });
            if !open { self.ui_state.show_arrow_help = false; }
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    let mut options = eframe::NativeOptions::default();
    options.viewport.transparent = Some(false);

    // --- THAY ĐỔI Ở ĐÂY ---
    // Cũ: options.viewport.inner_size = Some(egui::vec2(900.0, 950.0));
    options.viewport.inner_size = Some(egui::vec2(850.0, 790.0));
    // ---------------------

    options.viewport.position = Some(egui::pos2(100.0, 100.0));

    let icon_bytes = include_bytes!("icon2.ico");
    if let Ok(icon_image) = image::load_from_memory(icon_bytes) {
        let icon_rgba = icon_image.to_rgba8();
        let icon_width = icon_image.width() as u32;
        let icon_height = icon_image.height() as u32;
        let icon_data = egui::IconData {
            rgba: icon_rgba.into_raw(),
            width: icon_width,
            height: icon_height,
        };
        options.viewport.icon = Some(Arc::new(icon_data.into()));
    }

    let tray_menu = Menu::new();
    let item_show = MenuItem::with_id("show", "Hiện ứng dụng / Cài đặt", true, None);
    let item_quit = MenuItem::with_id("quit", "Thoát hoàn toàn", true, None);
    let _ = tray_menu.append(&item_show);
    let _ = tray_menu.append(&item_quit);

    let (tx, rx) = mpsc::channel();

    let tray_icon_obj = if let Ok(image) = image::load_from_memory(icon_bytes) {
        let rgba = image.to_rgba8();
        let (width, height) = image.dimensions();
        if let Ok(icon) = tray_icon::Icon::from_rgba(rgba.into_raw(), width, height) {
             TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip(APP_NAME)
            .with_icon(icon)
            .build()
            .ok()
        } else { None }
    } else { None };
    
    let tray_icon = tray_icon_obj.expect("Failed to create Tray Icon!");

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            let ctx_clone = cc.egui_ctx.clone();
            let tx_clone = tx.clone();

            std::thread::spawn(move || {
                while let Ok(event) = TrayIconEvent::receiver().recv() {
                    if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                        force_show_window_at_position();
                        let _ = tx_clone.send(AppSignal::Show);
                        ctx_clone.request_repaint();
                    }
                }
            });

            let ctx_clone2 = cc.egui_ctx.clone();
            let tx_clone2 = tx.clone();

            std::thread::spawn(move || {
                while let Ok(event) = MenuEvent::receiver().recv() {
                    if event.id.as_ref() == "show" {
                        force_show_window_at_position();
                        let _ = tx_clone2.send(AppSignal::Show);
                        ctx_clone2.request_repaint();
                    } else if event.id.as_ref() == "quit" {
                        std::process::exit(0);
                    }
                }
            });

            Box::new(MainApp::new(cc, tray_icon, rx))
        }),
    )
}

// --- THÊM HÀM NÀY VÀO CUỐI FILE ---
pub fn show_toggle_notification(enabled: bool) {
    let text = if enabled { "Đã bật tự động dịch" } else { "Đã tắt tự động dịch" };
    let req_id = rand::random::<u64>();

    // 1. Phát âm thanh (TTS)
    let text_audio = text.to_string();
    let req_id_tts = req_id;
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let _ = crate::tts::speak(&text_audio, false, 1.2, true, req_id_tts).await;
        });
    });

    // 2. Hiện thông báo trên màn hình
    let text_visual = text.to_string();
    let req_id_overlay = req_id;
    std::thread::spawn(move || {
        unsafe {
            let screen_w = winapi::um::winuser::GetSystemMetrics(winapi::um::winuser::SM_CXSCREEN);
            let width = 400;
            let height = 100;
            let left = (screen_w / 2) - (width / 2);
            let top = 100;
            let rect = winapi::shared::windef::RECT {
                left,
                top,
                right: left + width,
                bottom: top + height
            };
            crate::overlay::show_result_window_internal(rect, text_visual, 2000, false, req_id_overlay);
        }
    });
}