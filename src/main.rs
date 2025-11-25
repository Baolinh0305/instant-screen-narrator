#![windows_subsystem = "windows"]

mod config;
mod capture;
mod translation;
mod tts;
mod overlay;
mod key_utils;

use crate::overlay::show_result_window;
use eframe::egui;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::atomic::AtomicU64;
use std::sync::{mpsc::{self, Receiver, Sender}, Mutex};
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
use std::sync::OnceLock;

// --- IMPORT TRAY ICON ---
use tray_icon::{TrayIconBuilder, TrayIcon, TrayIconEvent, MouseButton};
use tray_icon::menu::{Menu, MenuItem, MenuEvent};

const DEFAULT_ARROW: &[u8] = include_bytes!("arrow.png");

static LAST_SELECT: AtomicU64 = AtomicU64::new(0);
static OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);
static LISTENING_PAUSED: AtomicBool = AtomicBool::new(true);
static AUTO_TRANSLATE_ENABLED: AtomicBool = AtomicBool::new(false);
static GROQ_REMAINING: AtomicI32 = AtomicI32::new(-1);
static HOTKEYS_NEED_UPDATE: AtomicBool = AtomicBool::new(false);

static IS_BINDING_MODE: AtomicBool = AtomicBool::new(false);
static LAST_BOUND_KEY: OnceLock<Mutex<String>> = OnceLock::new();

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

fn force_show_window_at_position() {
    unsafe {
        let window_name = to_wide("Instant Screen Narrator");
        let hwnd = FindWindowW(std::ptr::null(), window_name.as_ptr());
        
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);

            let screen_height = GetSystemMetrics(SM_CYSCREEN);
            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd, &mut rect) != 0 {
                let current_width = rect.right - rect.left;
                SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, current_width, screen_height, 0x0040); 
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum BindingTarget {
    Translate,
    Select,
    Instant,
    Auto,
    // --- MỚI: Binding cho vùng phụ ---
    AuxSelect(usize),
    AuxTranslate(usize),
}

enum AppSignal {
    Show,
}

struct MainApp {
    config: config::Config,
    gemini_api_key: String,
    current_prompt: String,
    editing_prompt_index: Option<usize>,

    binding_target: Option<BindingTarget>,

    hotkey_translate: String,
    hotkey_select: String,
    hotkey_instant: String,
    hotkey_auto: String,

    selected_api: String,
    started: bool,
    listener_spawned: bool,
    show_popup: bool,
    popup_text: String,
    use_tts: bool,

    show_arrow_window: bool,
    show_arrow_help: bool,
    arrow_texture: Option<egui::TextureHandle>,

    wwm_success_timer: Option<std::time::Instant>,
    wwm_name_success_timer: Option<std::time::Instant>,
    arrow_wwm_success_timer: Option<std::time::Instant>,
    auto_translate_active: bool,
    show_password: bool,
    last_config_sync: std::time::Instant,
    
    show_reset_confirm: bool,

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
            std::thread::sleep(Duration::from_millis(500));
            force_show_window_at_position();
        });

        let ctx_clone = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(250));
                ctx_clone.request_repaint();
            }
        });

        Self {
            config: config.clone(),
            gemini_api_key: config.gemini_api_key,
            current_prompt: config.current_prompt,
            editing_prompt_index: None,
            binding_target: None,

            hotkey_translate: config.hotkey_translate.clone(),
            hotkey_select: config.hotkey_select.clone(),
            hotkey_instant: config.hotkey_instant.clone(),
            hotkey_auto: config.hotkey_auto.clone(),

            selected_api: config.selected_api,
            started: false,
            listener_spawned: false,
            show_popup: false,
            popup_text: String::new(),
            use_tts: config.use_tts,
            show_arrow_window: false,
            show_arrow_help: false,
            arrow_texture: None,
            wwm_success_timer: None,
            wwm_name_success_timer: None,
            arrow_wwm_success_timer: None,
            auto_translate_active: false,
            show_password: false,
            last_config_sync: std::time::Instant::now(),
            
            show_reset_confirm: false,

            _tray_icon: tray_icon,
            rx_signal: rx,
        }
    }

    fn configure_style(&self, ctx: &egui::Context) {
        if self.config.is_dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(6.0, 6.0);
        style.spacing.window_margin = egui::Margin::same(8.0);

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

    fn sync_config_from_file(&mut self) {
        let new_config = config::Config::load();
        self.config.fixed_regions = new_config.fixed_regions;
        self.config.arrow_region = new_config.arrow_region;
        self.config.instant_region = new_config.instant_region;
        // Sync cả vùng phụ
        self.config.aux_regions = new_config.aux_regions;
    }

    fn check_key_binding(&mut self) {
        if let Some(target) = self.binding_target {
            unsafe {
                for vk in 8..255 {
                    if (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 {
                        if vk == VK_LBUTTON || vk == VK_RBUTTON || vk == VK_MBUTTON { continue; }

                        let key_name = crate::key_utils::get_name_from_vk(vk);

                        match target {
                            BindingTarget::Translate => { self.hotkey_translate = key_name.clone(); self.config.hotkey_translate = key_name; }
                            BindingTarget::Select => { self.hotkey_select = key_name.clone(); self.config.hotkey_select = key_name; }
                            BindingTarget::Instant => { self.hotkey_instant = key_name.clone(); self.config.hotkey_instant = key_name; }
                            BindingTarget::Auto => { self.hotkey_auto = key_name.clone(); self.config.hotkey_auto = key_name; }
                            // --- XỬ LÝ BINDING VÙNG PHỤ ---
                            BindingTarget::AuxSelect(idx) => {
                                if idx < self.config.aux_regions.len() {
                                    self.config.aux_regions[idx].hotkey_select = key_name;
                                }
                            }
                            BindingTarget::AuxTranslate(idx) => {
                                if idx < self.config.aux_regions.len() {
                                    self.config.aux_regions[idx].hotkey_translate = key_name;
                                }
                            }
                        }
                        self.config.save().unwrap();
                        HOTKEYS_NEED_UPDATE.store(true, Ordering::Relaxed);

                        self.binding_target = None;
                        IS_BINDING_MODE.store(false, Ordering::Relaxed);
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        break;
                    }
                }
            }
        }
    }

    async fn translate_regions(
        mut config: config::Config,
        regions: Vec<config::Region>,
        tx: Sender<(String, bool, f32, bool)>,
        should_copy: bool,
    ) {
        let prompt = config.current_prompt.clone();
        let mut final_text = String::new();
        
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
                        final_text.push_str("(Chưa nhập Key) ");
                        break;
                    }

                    match translation::translate_from_image(&config.selected_api, &api_key, &prompt, &image_bytes).await {
                        Ok(result) => {
                            final_text.push_str(&result.text);
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
                                final_text.push_str("(Hết lượt Request & hết Key dự phòng) ");
                                break;
                            }
                        },
                        Err(translation::TranslationError::Other(e)) => {
                            let error_msg = format!("Lỗi: {} ", e);
                            final_text.push_str(&error_msg);
                            break; 
                        }
                    }
                }
                if !success && final_text.is_empty() { final_text.push_str("... "); }
                final_text.push(' ');
            }
        }

        let cleaned_text = final_text.trim().to_string();
        if !cleaned_text.is_empty() {
            if should_copy {
                Self::copy_to_clipboard(&cleaned_text);
            }

            let _ = tx.send((cleaned_text.clone(), config.split_tts, config.speed, config.use_tts));
            if config.show_overlay {
                if let Some(region) = regions.first() {
                    let rect = RECT { left: region.x, top: region.y, right: region.x + region.width as i32, bottom: region.y + region.height as i32 };
                    let duration_ms = (cleaned_text.chars().count() as f32 / 10.0 * 1000.0) as u32;
                    std::thread::spawn(move || { show_result_window(rect, cleaned_text, duration_ms); });
                }
            }
        }
    }

    fn load_texture(&mut self, ctx: &egui::Context, bytes: &[u8], is_arrow: bool) {
        if let Ok(image) = image::load_from_memory(bytes) {
            let size = [image.width() as usize, image.height() as usize];
            let image_buffer = image.to_rgba8();
            let pixels = image_buffer.into_raw();
            let image_data = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
            let name = if is_arrow { "arrow_img" } else { "area_img" };
            let texture = ctx.load_texture(name, image_data, egui::TextureOptions::default());
            if is_arrow { self.arrow_texture = Some(texture); } 
        }
    }

    fn start_service(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel::<(String, bool, f32, bool)>();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            while let Ok((text, split_tts, speed, use_tts)) = rx.recv() {
                rt.block_on(async { if let Err(_e) = tts::speak(&text, split_tts, speed, use_tts).await {} });
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
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }

                if let Some(arrow_region) = &config.arrow_region {
                    let found = capture::is_template_present(arrow_region, &arrow_bytes);
                    if found {
                        miss_counter = 0; 
                        if !last_found_state {
                            let tx_inner = tx_auto.clone();
                            let should_copy = config.auto_copy && !config.copy_instant_only;
                            rt.block_on(async { Self::translate_regions(config.clone(), config.fixed_regions.clone(), tx_inner, should_copy).await; });
                            last_found_state = true;
                        }
                    } else {
                        if last_found_state {
                            miss_counter += 1;
                            if miss_counter > 5 { last_found_state = false; miss_counter = 0; }
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
                    // Reset toàn bộ
                    for i in 1..500 { UnregisterHotKey(hwnd, i); }

                    let k1 = crate::key_utils::get_vk_from_name(&cfg.hotkey_translate);
                    let k2 = crate::key_utils::get_vk_from_name(&cfg.hotkey_select);
                    let k3 = crate::key_utils::get_vk_from_name(&cfg.hotkey_instant);
                    let k4 = crate::key_utils::get_vk_from_name(&cfg.hotkey_auto);
                    if k1 > 0 { RegisterHotKey(hwnd, 1, 0, k1 as UINT); }
                    if k2 > 0 { RegisterHotKey(hwnd, 2, 0, k2 as UINT); }
                    if k3 > 0 { RegisterHotKey(hwnd, 3, 0, k3 as UINT); }
                    if k4 > 0 { RegisterHotKey(hwnd, 4, 0, k4 as UINT); }

                    // --- ĐĂNG KÝ PHÍM TẮT CHO VÙNG PHỤ ---
                    // Quy tắc ID:
                    // 100 + index: Chọn vùng
                    // 200 + index: Dịch vùng
                    for (i, aux) in cfg.aux_regions.iter().enumerate() {
                        let k_sel = crate::key_utils::get_vk_from_name(&aux.hotkey_select);
                        let k_trans = crate::key_utils::get_vk_from_name(&aux.hotkey_translate);
                        if k_sel > 0 { RegisterHotKey(hwnd, 100 + i as i32, 0, k_sel as UINT); }
                        if k_trans > 0 { RegisterHotKey(hwnd, 200 + i as i32, 0, k_trans as UINT); }
                    }
                };

                register_keys(hwnd);
                SetTimer(hwnd, 1, 500, None);

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
                            
                            // --- XỬ LÝ PHÍM CHÍNH ---
                            if id == 1 { // Translate
                                let tx = tx_clone.clone();
                                let rt = tokio::runtime::Runtime::new().unwrap();
                                let should_copy = config.auto_copy && !config.copy_instant_only;
                                std::thread::spawn(move || { rt.block_on(async { Self::translate_regions(config.clone(), config.fixed_regions.clone(), tx, should_copy).await; }); });
                            } else if id == 2 { // Select
                                if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
                                    if now - LAST_SELECT.load(Ordering::Relaxed) > 1000 {
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
                                    if now - LAST_SELECT.load(Ordering::Relaxed) > 1000 {
                                        LAST_SELECT.store(now, Ordering::Relaxed); OVERLAY_ACTIVE.store(true, Ordering::Relaxed);
                                        overlay::set_selection_mode(1); std::thread::spawn(|| { overlay::show_selection_overlay(); OVERLAY_ACTIVE.store(false, Ordering::Relaxed); });
                                    }
                                }
                            } 
                            // --- XỬ LÝ VÙNG PHỤ ---
                            else if id >= 100 && id < 200 { // Select Aux
                                let idx = (id - 100) as usize;
                                if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                                    OVERLAY_ACTIVE.store(true, Ordering::Relaxed);
                                    // Gửi ID vùng phụ vào selection mode (100 + idx)
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

        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert("roboto".to_owned(), egui::FontData::from_static(include_bytes!("roboto.ttf")));
        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "roboto".to_owned());
        fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap().push("roboto".to_owned());
        ctx.set_fonts(fonts);
        ctx.set_pixels_per_point(1.2);

        self.configure_style(ctx);

        if self.binding_target.is_some() {
            self.check_key_binding();
            ctx.request_repaint();
        }

        if self.last_config_sync.elapsed() > Duration::from_secs(1) {
            self.sync_config_from_file();
            self.last_config_sync = std::time::Instant::now();
        }

        if self.arrow_texture.is_none() {
             let custom_path = config::Config::get_custom_arrow_path();
             if custom_path.exists() { if let Ok(b) = fs::read(custom_path) { self.load_texture(ctx, &b, true); } } 
             else { self.load_texture(ctx, DEFAULT_ARROW, true); }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                        ui.heading(egui::RichText::new("Instant Screen Narrator").strong().size(24.0));
                        ui.add_space(10.0);
                        if ui.button(egui::RichText::new("👤 Tạo bởi: Baolinh0305").small()).clicked() {
                             let _ = webbrowser::open("https://github.com/Baolinh0305/instant-screen-narrator/releases");
                        }
                        ui.add_space(10.0);
                        
                        ui.add_enabled_ui(!self.started, |ui| {
                            if ui.button(egui::RichText::new("🔄 Reset").small().color(egui::Color32::RED)).clicked() {
                                self.show_reset_confirm = true;
                            }
                            let theme_text = if self.config.is_dark_mode { "🌗 Theme: Tối" } else { "🌗 Theme: Sáng" };
                            if ui.button(egui::RichText::new(theme_text).small()).clicked() {
                                self.config.is_dark_mode = !self.config.is_dark_mode;
                                self.config.save().unwrap();
                            }
                        });
                    });
                });
                ui.add_space(10.0);
            });

            egui::ScrollArea::vertical().show(ui, |ui| {
                // KHỐI API
                egui::CollapsingHeader::new(egui::RichText::new("🌐 Cấu hình API").strong()).default_open(true).show(ui, |ui| {
                    egui::Grid::new("api_grid").num_columns(2).spacing([20.0, 15.0]).striped(true).show(ui, |ui| {
                        ui.label("Dịch vụ:");
                        ui.horizontal(|ui| {
                            ui.add_enabled_ui(!self.started, |ui| {
                                egui::ComboBox::from_id_source("api_selector")
                                    .selected_text(if self.selected_api == "gemini" { "Gemini (Không nên dùng)" } else { "Groq (Nhanh)" })
                                    .width(200.0)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut self.selected_api, "gemini".to_string(), "Gemini (Không nên dùng)");
                                        ui.selectable_value(&mut self.selected_api, "groq".to_string(), "Groq (Meta Llama)");
                                    });
                            });

                            if self.selected_api == "groq" {
                                let remaining = GROQ_REMAINING.load(Ordering::Relaxed);
                                if remaining >= 0 {
                                    ui.colored_label(egui::Color32::GREEN, format!("(Còn lại: {} req)", remaining));
                                } else {
                                    ui.colored_label(egui::Color32::GRAY, "(Còn lại: ?)");
                                }
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let help_text = if self.selected_api == "gemini" { "❓ Hướng dẫn (Gemini)" } else { "❓ Hướng dẫn (Groq)" };
                                if ui.add(egui::Button::new(egui::RichText::new(help_text).small())).clicked() {
                                    self.show_popup = true;
                                    self.popup_text = self.selected_api.clone();
                                }
                            });
                        });
                        ui.end_row();

                        ui.label("API Key(s):");
                        ui.vertical(|ui| {
                             if self.selected_api == "gemini" {
                                 ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.gemini_api_key).password(!self.show_password).desired_width(400.0));
                             } else {
                                 let mut keys_to_remove = Vec::new();
                                 let show_pass = self.show_password;
                                 let started = self.started;
                                 let keys_count = self.config.groq_api_keys.len();

                                 for (i, key) in self.config.groq_api_keys.iter_mut().enumerate() {
                                     ui.horizontal(|ui| {
                                         ui.label(format!("#{}", i + 1));
                                         ui.add_enabled(!started, egui::TextEdit::singleline(key).password(!show_pass).desired_width(350.0));
                                         if !started && keys_count > 1 {
                                             if ui.button("🗑").clicked() { keys_to_remove.push(i); }
                                         }
                                     });
                                 }
                                 for i in keys_to_remove.iter().rev() {
                                     self.config.groq_api_keys.remove(*i);
                                 }

                                 if !self.started {
                                     if ui.button("➕ Thêm Key dự phòng").clicked() {
                                         self.config.groq_api_keys.push(String::new());
                                     }
                                 }
                             }

                             if ui.button(if self.show_password { "🙈 Ẩn Key" } else { "👁 Hiện Key" }).clicked() {
                                 self.show_password = !self.show_password;
                             }
                        });
                        ui.end_row();
                    });
                });
                ui.add_space(5.0);

                // KHỐI PROMPT
                egui::CollapsingHeader::new(egui::RichText::new("📝 Cấu hình Dịch (Prompt)").strong()).default_open(true).show(ui, |ui| {
                    ui.add_enabled_ui(!self.started, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("🗡️ Kiếm hiệp").clicked() { 
                                self.current_prompt = config::Config::get_wuxia_prompt(); 
                                self.editing_prompt_index = None;
                            }
                            if ui.button("🌍 Thông thường").clicked() { 
                                self.current_prompt = config::Config::get_normal_prompt(); 
                                self.editing_prompt_index = None;
                            }

                            let mut to_select = None;
                            for (i, _) in self.config.saved_prompts.iter().enumerate() {
                                let btn_label = format!("Mẫu {}", i + 1);
                                let is_selected = self.editing_prompt_index == Some(i);
                                if ui.add(egui::Button::new(btn_label).selected(is_selected)).clicked() {
                                    to_select = Some(i);
                                }
                            }
                            
                            if let Some(i) = to_select {
                                self.editing_prompt_index = Some(i);
                                self.current_prompt = self.config.saved_prompts[i].content.clone();
                            }

                            if ui.button("➕").clicked() {
                                self.config.saved_prompts.push(config::CustomPrompt {
                                    content: String::new(),
                                });
                                self.editing_prompt_index = Some(self.config.saved_prompts.len() - 1);
                                self.current_prompt = String::new();
                                self.config.save().unwrap();
                            }
                        });
                    });
                    ui.add_space(5.0);
                    
                    if let Some(idx) = self.editing_prompt_index {
                        if idx < self.config.saved_prompts.len() {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("Đang sửa: Mẫu {}", idx + 1)).italics());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.add_enabled(!self.started, egui::Button::new(egui::RichText::new("🗑").color(egui::Color32::RED))).clicked() {
                                        self.config.saved_prompts.remove(idx);
                                        self.editing_prompt_index = None;
                                        self.current_prompt = config::Config::get_normal_prompt();
                                        self.config.save().unwrap();
                                    }
                                });
                            });
                        }
                    }

                    if ui.add_enabled(!self.started, egui::TextEdit::multiline(&mut self.current_prompt).desired_rows(4).desired_width(f32::INFINITY)).changed() {
                        if let Some(idx) = self.editing_prompt_index {
                            if idx < self.config.saved_prompts.len() {
                                self.config.saved_prompts[idx].content = self.current_prompt.clone();
                                self.config.save().unwrap();
                            }
                        }
                    }
                });
                ui.add_space(5.0);

                // KHỐI PHÍM TẮT CHUNG
                egui::CollapsingHeader::new(egui::RichText::new("⌨️ Phím tắt chung").strong()).default_open(true).show(ui, |ui| {
                     egui::Grid::new("hotkey_grid").num_columns(2).spacing([20.0, 10.0]).striped(true).show(ui, |ui| {
                        
                        let mut draw_bind_btn = |label: &str, target: BindingTarget, current_key: &str| {
                             ui.label(label);
                             let btn_text = if self.binding_target == Some(target) { "🛑 Đang chờ phím..." } else { current_key };
                             
                             let btn = if self.binding_target == Some(target) {
                                 egui::Button::new(egui::RichText::new(btn_text).color(egui::Color32::YELLOW))
                             } else {
                                 egui::Button::new(btn_text)
                             };

                             if ui.add_enabled(!self.started, btn).clicked() {
                                 if self.binding_target == Some(target) {
                                     self.binding_target = None;
                                     IS_BINDING_MODE.store(false, Ordering::Relaxed);
                                 } else {
                                     self.binding_target = Some(target);
                                     IS_BINDING_MODE.store(true, Ordering::Relaxed);
                                 }
                             }
                             ui.end_row();
                        };

                        draw_bind_btn("Dịch vùng đã chọn:", BindingTarget::Translate, &self.hotkey_translate);
                        draw_bind_btn("Chọn vùng dịch:", BindingTarget::Select, &self.hotkey_select);
                        draw_bind_btn("Chụp & Dịch ngay:", BindingTarget::Instant, &self.hotkey_instant);
                    });
                });
                ui.add_space(5.0);

                // --- KHỐI MỚI: QUẢN LÝ VÙNG DỊCH PHỤ ---
                egui::CollapsingHeader::new(egui::RichText::new("📑 Vùng dịch phụ (Multi-Region)").strong()).default_open(true).show(ui, |ui| {
                    ui.add_enabled_ui(!self.started, |ui| {
                        if ui.button("➕ Thêm vùng dịch mới").clicked() {
                            let new_id = self.config.aux_regions.len();
                            self.config.aux_regions.push(config::AuxRegion {
                                id: new_id,
                                name: format!("Vùng phụ #{}", new_id + 1),
                                region: None,
                                hotkey_select: "NONE".to_string(),
                                hotkey_translate: "NONE".to_string(),
                            });
                            self.config.save().unwrap();
                            HOTKEYS_NEED_UPDATE.store(true, Ordering::Relaxed);
                        }
                        
                        ui.add_space(5.0);
                        
                        let mut remove_idx = None;
                        for (i, aux) in self.config.aux_regions.iter_mut().enumerate() {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(&aux.name).strong());
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button("🗑 Xóa").clicked() { remove_idx = Some(i); }
                                    });
                                });
                                ui.horizontal(|ui| {
                                    // Bind Select Key
                                    ui.label("Chọn:");
                                    let btn_txt_sel = if self.binding_target == Some(BindingTarget::AuxSelect(i)) { "..." } else { &aux.hotkey_select };
                                    if ui.button(btn_txt_sel).clicked() {
                                        self.binding_target = Some(BindingTarget::AuxSelect(i));
                                        IS_BINDING_MODE.store(true, Ordering::Relaxed);
                                    }
                                    
                                    // Bind Translate Key
                                    ui.label("Dịch:");
                                    let btn_txt_trans = if self.binding_target == Some(BindingTarget::AuxTranslate(i)) { "..." } else { &aux.hotkey_translate };
                                    if ui.button(btn_txt_trans).clicked() {
                                        self.binding_target = Some(BindingTarget::AuxTranslate(i));
                                        IS_BINDING_MODE.store(true, Ordering::Relaxed);
                                    }
                                    
                                    if aux.region.is_some() {
                                        ui.label("✅ Đã có vùng");
                                    } else {
                                        ui.label("⚠️ Chưa chọn vùng");
                                    }
                                });
                            });
                            ui.add_space(2.0);
                        }
                        
                        if let Some(i) = remove_idx {
                            self.config.aux_regions.remove(i);
                            self.config.save().unwrap();
                            HOTKEYS_NEED_UPDATE.store(true, Ordering::Relaxed);
                        }
                    });
                });
                ui.add_space(5.0);

                // KHỐI CÀI ĐẶT
                egui::CollapsingHeader::new(egui::RichText::new("⚙️ Cài đặt hiển thị & Âm thanh").strong()).default_open(true).show(ui, |ui| {
                    egui::Grid::new("settings_grid").num_columns(2).spacing([20.0, 10.0]).show(ui, |ui| {
                        
                        ui.label("Overlay:");
                        ui.horizontal(|ui| {
                            ui.add_enabled(!self.started, egui::Checkbox::new(&mut self.config.show_overlay, "Hiện văn bản"));
                            ui.add_space(10.0);
                            ui.label("Cỡ chữ:");
                            if ui.add_enabled(!self.started && self.config.show_overlay, egui::Slider::new(&mut self.config.overlay_font_size, 10..=72).text("px")).changed() {
                                overlay::set_font_size(self.config.overlay_font_size);
                                self.config.save().unwrap();
                            }
                        });
                        ui.end_row();

                        ui.label("TTS (Đọc):");
                        ui.horizontal(|ui| {
                            ui.add_enabled(!self.started, egui::Checkbox::new(&mut self.use_tts, "Bật đọc"));
                            ui.add_space(10.0);
                            ui.label("Tốc độ:");
                            ui.add_enabled(!self.started && self.use_tts, egui::Slider::new(&mut self.config.speed, 0.5..=2.0).text("x"));
                        });
                        ui.end_row();
                        
                        ui.label("Tùy chọn khác:");
                        ui.vertical(|ui| {
                            ui.add_enabled(!self.started, egui::Checkbox::new(&mut self.config.freeze_screen, "Đóng băng khi chọn vùng"));
                            self.config.split_tts = true; 
                            ui.add_enabled(false, egui::Checkbox::new(&mut self.config.split_tts, "Split TTS (Luôn bật)"));
                        });
                        ui.end_row();

                        ui.label("Copy Text:");
                        ui.vertical(|ui| {
                            ui.add_enabled(!self.started, egui::Checkbox::new(&mut self.config.auto_copy, "Tự động Copy kết quả"));
                            if self.config.auto_copy {
                                ui.add_enabled(!self.started, egui::Checkbox::new(&mut self.config.copy_instant_only, "Chỉ áp dụng lên Dịch nhanh"));
                            }
                        });
                        ui.end_row();
                    });
                });
                ui.add_space(20.0);

                // === KHỐI WWM ===
                ui.vertical_centered(|ui| {
                    egui::CollapsingHeader::new(egui::RichText::new("🎮 Dịch Where Winds Meet").strong()).default_open(true).show(ui, |ui| {
                        ui.add_space(5.0);
                        
                        ui.horizontal(|ui| {
                            ui.label("Phím tắt chọn vùng Mũi tên:");
                            let btn_text = if self.binding_target == Some(BindingTarget::Auto) { "🛑 Chờ..." } else { &self.hotkey_auto };
                            let btn = if self.binding_target == Some(BindingTarget::Auto) { egui::Button::new(egui::RichText::new(btn_text).color(egui::Color32::YELLOW)) } else { egui::Button::new(btn_text) };

                            if ui.add_enabled(!self.started, btn).clicked() {
                                if self.binding_target == Some(BindingTarget::Auto) { self.binding_target = None; IS_BINDING_MODE.store(false, Ordering::Relaxed); } else { self.binding_target = Some(BindingTarget::Auto); IS_BINDING_MODE.store(true, Ordering::Relaxed); }
                            }
                            ui.add_space(10.0);
                            if ui.button("❓").clicked() { self.show_arrow_help = true; }
                        });
                        ui.add_space(5.0);

                        // Nút WWM Thường
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                                let mut wwm_text = "🎯 Tự động chọn vùng dịch WWM";
                                if let Some(time) = self.wwm_success_timer {
                                    if time.elapsed().as_secs_f32() < 1.0 { wwm_text = "✅ Đã chọn"; ctx.request_repaint(); } 
                                    else { self.wwm_success_timer = None; }
                                }
                                if ui.add_enabled(self.started, egui::Button::new(wwm_text)).clicked() {
                                    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32;
                                    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) } as f32;
                                    let r_x = 308.0 / 1920.0; let r_y = 919.0 / 1080.0;
                                    let r_w = 1313.0 / 1920.0; let r_h = 135.0 / 1080.0;
                                    
                                    let region = config::Region {
                                        x: (screen_w * r_x) as i32 - 5, 
                                        y: (screen_h * r_y) as i32 - 5,
                                        width: (screen_w * r_w) as u32 + 10, 
                                        height: (screen_h * r_h) as u32 + 10
                                    };
                                    
                                    self.config.fixed_regions.clear(); 
                                    self.config.fixed_regions.push(region.clone());
                                    self.current_prompt = config::Config::get_wuxia_prompt();
                                    self.config.current_prompt = self.current_prompt.clone();
                                    self.editing_prompt_index = None;
                                    self.config.save().unwrap();
                                    self.sync_config_from_file(); 
                                    self.wwm_success_timer = Some(std::time::Instant::now());
                                    overlay::show_highlight(RECT{left: region.x, top: region.y, right: region.x + region.width as i32, bottom: region.y + region.height as i32});
                                }
                                ui.label(egui::RichText::new("(16:9)").italics().color(egui::Color32::GRAY));
                            });
                        });

                        // Nút WWM Có Tên
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                                let mut wwm_name_text = "🎯 Tự chọn vùng dịch WWM (có tên người thoại)";
                                if let Some(time) = self.wwm_name_success_timer {
                                    if time.elapsed().as_secs_f32() < 1.0 { wwm_name_text = "✅ Đã chọn"; ctx.request_repaint(); } 
                                    else { self.wwm_name_success_timer = None; }
                                }
                                if ui.add_enabled(self.started, egui::Button::new(wwm_name_text)).clicked() {
                                    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32;
                                    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) } as f32;
                                    let r_x = 310.0 / 1920.0; let r_y = 868.0 / 1080.0;
                                    let r_w = 1312.0 / 1920.0; let r_h = 187.0 / 1080.0;
                                    let region = config::Region { x: (screen_w * r_x) as i32, y: (screen_h * r_y) as i32, width: (screen_w * r_w) as u32, height: (screen_h * r_h) as u32 };
                                    
                                    self.config.fixed_regions.clear(); 
                                    self.config.fixed_regions.push(region.clone());
                                    self.current_prompt = config::Config::get_wuxia_speaker_prompt();
                                    self.config.current_prompt = self.current_prompt.clone();
                                    self.editing_prompt_index = None;
                                    self.config.save().unwrap();
                                    self.sync_config_from_file(); 
                                    self.wwm_name_success_timer = Some(std::time::Instant::now());
                                    overlay::show_highlight(RECT{left: region.x, top: region.y, right: region.x + region.width as i32, bottom: region.y + region.height as i32});
                                }
                                ui.label(egui::RichText::new("(16:9)").italics().color(egui::Color32::GRAY));
                            });
                        });

                        // Nút Mũi tên
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                                let mut arrow_text = "🏹 Tự động chọn vùng Mũi tên WWM";
                                if let Some(time) = self.arrow_wwm_success_timer {
                                    if time.elapsed().as_secs_f32() < 1.0 { arrow_text = "✅ Đã chọn"; ctx.request_repaint(); }
                                    else { self.arrow_wwm_success_timer = None; }
                                }
                                if ui.add_enabled(self.started, egui::Button::new(arrow_text)).clicked() {
                                    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32;
                                    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) } as f32;
                                    let r_x = 930.0 / 1920.0; let r_y = 1042.0 / 1080.0;
                                    let r_w = 49.0 / 1920.0; let r_h = 36.0 / 1080.0;
                                    let region = config::Region { x: (screen_w * r_x) as i32, y: (screen_h * r_y) as i32, width: (screen_w * r_w) as u32, height: (screen_h * r_h) as u32 };
                                    self.config.arrow_region = Some(region.clone());
                                    self.config.save().unwrap();
                                    self.sync_config_from_file(); 
                                    self.arrow_wwm_success_timer = Some(std::time::Instant::now());
                                    overlay::show_highlight(RECT{left: region.x, top: region.y, right: region.x + region.width as i32, bottom: region.y + region.height as i32});
                                }
                                ui.label(egui::RichText::new("(16:9)").italics().color(egui::Color32::GRAY));
                                if ui.button("🖼️").clicked() { self.show_arrow_window = true; }
                            });
                        });
                        
                        if self.started {
                            ui.add_space(5.0);
                            ui.label(egui::RichText::new("⚡ Tốc độ nhận diện mũi tên").strong());
                            ui.add(egui::Slider::new(&mut self.config.arrow_check_interval, 0.02..=0.2).text("s"));
                            if (self.config.arrow_check_interval - 0.02).abs() < 0.001 {
                                ui.colored_label(egui::Color32::GREEN, "(Nên để mặc định: 0.02)");
                            } else {
                                ui.label(format!("(Quét mỗi {:.2} giây)", self.config.arrow_check_interval));
                            }
                            ui.add_space(5.0);

                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                                    let btn_text = if self.auto_translate_active { "🔄 ĐANG BẬT TỰ ĐỘNG DỊCH" } else { "🔄 Bật Tự Động Dịch" };
                                    let btn_color = if self.auto_translate_active { egui::Color32::DARK_GREEN } else { egui::Color32::from_rgb(60, 60, 60) };
                                    
                                    if ui.add(egui::Button::new(egui::RichText::new(btn_text).strong().color(egui::Color32::WHITE)).fill(btn_color).min_size(egui::vec2(200.0, 30.0))).clicked() {
                                        self.auto_translate_active = !self.auto_translate_active;
                                        AUTO_TRANSLATE_ENABLED.store(self.auto_translate_active, Ordering::Relaxed);
                                    }
                                });
                            });
                            
                            ui.separator();
                            
                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                                    if ui.button("🐞 Debug Overlay").clicked() {
                                        overlay::toggle_debug_overlay();
                                    }

                                    if ui.button("📂 Đổi ảnh mũi tên").clicked() {
                                        if let Some(path) = rfd::FileDialog::new().add_filter("Image", &["png"]).pick_file() {
                                            if let Ok(bytes) = fs::read(&path) {
                                                let dest = config::Config::get_custom_arrow_path();
                                                if let Ok(_) = fs::write(&dest, &bytes) {
                                                    self.load_texture(ctx, &bytes, true);
                                                }
                                            }
                                        }
                                    }
                                    let custom_path = config::Config::get_custom_arrow_path();
                                    if custom_path.exists() {
                                        if ui.button("❌ Reset Mặc định").clicked() {
                                            let _ = fs::remove_file(custom_path);
                                            self.load_texture(ctx, DEFAULT_ARROW, true);
                                        }
                                    }
                                });
                            });
                        } else {
                            ui.add_space(5.0);
                            ui.label(egui::RichText::new("⚠️ Hãy bấm 'BẮT ĐẦU' để dùng tính năng này").color(egui::Color32::RED));
                        }
                        ui.add_space(5.0);
                    });
                    
                    ui.add_space(10.0);

                    if !self.started {
                        let start_btn = egui::Button::new(egui::RichText::new("🚀 BẮT ĐẦU SỬ DỤNG").size(20.0).strong().color(egui::Color32::WHITE))
                            .min_size(egui::vec2(200.0, 50.0)).fill(egui::Color32::from_rgb(0, 120, 215)); 
                        if ui.add(start_btn).clicked() {
                            self.config.gemini_api_key = self.gemini_api_key.clone();
                            self.config.current_prompt = self.current_prompt.clone();
                            self.config.hotkey_translate = self.hotkey_translate.clone();
                            self.config.hotkey_select = self.hotkey_select.clone();
                            self.config.hotkey_instant = self.hotkey_instant.clone();
                            self.config.hotkey_auto = self.hotkey_auto.clone();
                            self.config.selected_api = self.selected_api.clone();
                            self.config.use_tts = self.use_tts;
                            self.config.save().unwrap();
                            self.started = true;
                            LISTENING_PAUSED.store(false, Ordering::Relaxed);
                            
                            if !self.listener_spawned { self.start_service(); self.listener_spawned = true; }
                        }
                    } else {
                        // KHI ĐÃ START: Hiện nút Dừng và nút Ẩn Tray
                        ui.horizontal(|ui| {
                            let stop_btn = egui::Button::new(egui::RichText::new("⏹ Dừng").size(20.0).strong().color(egui::Color32::WHITE))
                                .min_size(egui::vec2(150.0, 50.0)).fill(egui::Color32::from_rgb(200, 50, 50));
                            if ui.add(stop_btn).clicked() { 
                                self.started = false; 
                                self.auto_translate_active = false;
                                AUTO_TRANSLATE_ENABLED.store(false, Ordering::Relaxed);
                                LISTENING_PAUSED.store(true, Ordering::Relaxed); 
                            }

                            ui.add_space(10.0);

                            let tray_btn = egui::Button::new(egui::RichText::new("🔽 Ẩn vào Tray").size(16.0).strong())
                                .min_size(egui::vec2(120.0, 50.0));
                            if ui.add(tray_btn).clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                            }
                        });
                    }
                    ui.add_space(10.0);
                });
            });
        });

        if self.show_popup {
            let mut open = true;
            let title = if self.popup_text == "gemini" { "Hướng dẫn Gemini" } else { "Hướng dẫn Groq" };
            egui::Window::new(title).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                if self.popup_text == "gemini" {
                    ui.heading("Gemini API");
                    ui.horizontal(|ui| { ui.label("1. Vào:"); ui.hyperlink("https://aistudio.google.com/api-keys"); });
                    ui.label("2. Đăng nhập Google -> Create API key"); ui.label("3. Copy key và dán vào tool");
                } else if self.popup_text == "groq" {
                    ui.heading("Groq API (Nhanh)");
                    ui.horizontal(|ui| { ui.label("1. Vào:"); ui.hyperlink("https://console.groq.com/keys"); });
                    ui.label("2. Đăng nhập -> Create API Key"); ui.label("3. Copy key và dán vào tool");
                }
                ui.separator();
                ui.vertical_centered(|ui| { if ui.button("Đã hiểu").clicked() { self.show_popup = false; } });
            });
            if !open { self.show_popup = false; }
        }

        if self.show_reset_confirm {
            let mut open = true;
            egui::Window::new("⚠️ Xác nhận Reset").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).open(&mut open).show(ctx, |ui| {
                ui.label("Bạn có muốn giữ lại API Key không?");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("✅ Có (Giữ Key)").clicked() {
                        let saved_gemini = self.config.gemini_api_key.clone();
                        let saved_groq = self.config.groq_api_keys.clone();
                        self.config = config::Config::default();
                        self.config.gemini_api_key = saved_gemini;
                        self.config.groq_api_keys = saved_groq;
                        
                        self.gemini_api_key = self.config.gemini_api_key.clone();
                        self.current_prompt = self.config.current_prompt.clone();
                        self.editing_prompt_index = None;
                        self.hotkey_translate = self.config.hotkey_translate.clone();
                        self.hotkey_select = self.config.hotkey_select.clone();
                        self.hotkey_instant = self.config.hotkey_instant.clone();
                        self.hotkey_auto = self.config.hotkey_auto.clone();
                        self.selected_api = self.config.selected_api.clone();
                        self.use_tts = self.config.use_tts;
                        
                        overlay::set_font_size(self.config.overlay_font_size);
                        HOTKEYS_NEED_UPDATE.store(true, Ordering::Relaxed);
                        let _ = self.config.save();
                        self.show_reset_confirm = false;
                    }
                    if ui.button("❌ Không (Xóa sạch)").clicked() {
                        self.config = config::Config::default();
                        self.gemini_api_key = String::new();
                        self.config.groq_api_keys = vec![String::new()];
                        self.current_prompt = self.config.current_prompt.clone();
                        self.editing_prompt_index = None;
                        self.hotkey_translate = self.config.hotkey_translate.clone();
                        self.hotkey_select = self.config.hotkey_select.clone();
                        self.hotkey_instant = self.config.hotkey_instant.clone();
                        self.hotkey_auto = self.config.hotkey_auto.clone();
                        self.selected_api = self.config.selected_api.clone();
                        self.use_tts = self.config.use_tts;
                        
                        overlay::set_font_size(self.config.overlay_font_size);
                        HOTKEYS_NEED_UPDATE.store(true, Ordering::Relaxed);
                        let _ = self.config.save();
                        self.show_reset_confirm = false;
                    }
                    if ui.button("🔙 Hủy").clicked() {
                        self.show_reset_confirm = false;
                    }
                });
            });
            if !open { self.show_reset_confirm = false; }
        }

        if self.show_arrow_window {
            let mut open = true;
            egui::Window::new("Mũi tên hiện tại").open(&mut open).collapsible(false).resizable(true).default_size(egui::vec2(300.0, 300.0)).show(ctx, |ui| {
                 ui.label("Đây là hình ảnh mũi tên đang được dùng để nhận diện:");
                 ui.separator();
                if let Some(texture) = &self.arrow_texture {
                    let s = texture.size_vec2() * 2.0;
                    ui.centered_and_justified(|ui| { ui.image((texture.id(), s)); });
                } else { ui.label("Không tìm thấy arrow.png"); }
            });
            if !open { self.show_arrow_window = false; }
        }
        
        if self.show_arrow_help {
            let mut open = true;
            egui::Window::new("Giải thích Mũi tên").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label("Đây là hình tam giác hiện ra ở dưới hội thoại mỗi khi nó hiện đầy đủ.");
                ui.label("Phần mềm sẽ dựa vào mũi tên đó để nhận biết khi nào câu thoại đã chạy xong và tiến hành tự động dịch.");
                ui.add_space(10.0);
                if ui.button("Đã hiểu").clicked() { self.show_arrow_help = false; }
            });
            if !open { self.show_arrow_help = false; }
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    let mut options = eframe::NativeOptions::default();
    options.viewport.transparent = Some(false);
    options.viewport.inner_size = Some(egui::vec2(900.0, 950.0));
    options.viewport.taskbar = Some(true);
    options.viewport.position = Some(egui::pos2(0.0, 0.0));

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
            .with_tooltip("Instant Screen Narrator")
            .with_icon(icon)
            .build()
            .ok()
        } else { None }
    } else { None };
    
    let tray_icon = tray_icon_obj.expect("Failed to create Tray Icon!");

    eframe::run_native(
        "Instant Screen Narrator",
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