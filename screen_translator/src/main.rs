mod config;
mod capture;
mod translation;
mod tts;
mod overlay;

use eframe::egui;

use rdev::{listen, Event, EventType};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static LAST_SELECT: AtomicU64 = AtomicU64::new(0);
static OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);


struct MainApp {
    config: config::Config,
    gemini_api_key: String,
    groq_api_key: String,
    current_prompt: String,
    custom_prompt: String,
    hotkey_translate: String,
    hotkey_select: String,
    hotkey_instant: String,
    selected_api: String,
    started: bool,
    show_popup: bool,
    popup_text: String,
    in_custom_mode: bool,
}

impl Default for MainApp {
    fn default() -> Self {
        let config = config::Config::load();
        Self {
            config: config.clone(),
            gemini_api_key: config.gemini_api_key,
            groq_api_key: config.groq_api_key,
            current_prompt: config.current_prompt,
            custom_prompt: config.custom_prompt,
            hotkey_translate: config.hotkey_translate,
            hotkey_select: config.hotkey_select,
            hotkey_instant: config.hotkey_instant,
            selected_api: config.selected_api,
            started: false,
            show_popup: false,
            popup_text: String::new(),
            in_custom_mode: false,
        }
    }
}

impl eframe::App for MainApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert("roboto".to_owned(), egui::FontData::from_static(include_bytes!("roboto.ttf")));
        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "roboto".to_owned());
        fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap().push("roboto".to_owned());
        ctx.set_fonts(fonts);
        ctx.set_pixels_per_point(1.2);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Screen Translator");
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label("Chọn API để dịch:");
                ui.set_enabled(!self.started);
                ui.horizontal(|ui| {
                    egui::ComboBox::from_label("")
                        .selected_text(if self.selected_api == "gemini" { "Gemini" } else { "Groq" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.selected_api, "gemini".to_string(), "Gemini");
                            ui.selectable_value(&mut self.selected_api, "groq".to_string(), "Groq");
                        });
                    ui.label(if self.selected_api == "gemini" { "Chậm, để cho vui không nên chọn" } else { "Nhanh, mỗi api key dịch được 1000 lần/ngày, hết thì đổi api key" });
                });
                ui.set_enabled(true);
                ui.horizontal(|ui| {
                    if self.selected_api == "groq" {
                        if ui.button("Cách lấy groq api key").clicked() {
                            self.show_popup = true;
                            self.popup_text = "groq".to_string();
                        }
                    } else {
                        if ui.button("Cách lấy gemini api key").clicked() {
                            self.show_popup = true;
                            self.popup_text = "gemini".to_string();
                        }
                    }
                });
                if self.selected_api == "gemini" {
                    ui.horizontal(|ui| {
                        ui.label("Nhập Gemini api key:");
                        ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.gemini_api_key));
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.label("Nhập Groq api key:");
                        ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.groq_api_key));
                    });
                }
            });
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label("Prompt Dịch:");
                ui.horizontal(|ui| {
                    if ui.add_enabled(!self.started, egui::Button::new("Dịch kiểu kiếm hiệp")).clicked() {
                        self.current_prompt = config::Config::get_wuxia_prompt();
                        self.in_custom_mode = false;
                    }
                    if ui.add_enabled(!self.started, egui::Button::new("Dịch kiểu bình thường")).clicked() {
                        self.current_prompt = config::Config::get_normal_prompt();
                        self.in_custom_mode = false;
                    }
                    if ui.add_enabled(!self.started, egui::Button::new("Custom")).clicked() {
                        self.current_prompt = self.custom_prompt.clone();
                        self.in_custom_mode = true;
                    }
                });
                ui.add_enabled(!self.started, egui::TextEdit::multiline(&mut self.current_prompt).desired_rows(5));
                if self.in_custom_mode && ui.add_enabled(!self.started, egui::Button::new("Lưu")).clicked() {
                    self.custom_prompt = self.current_prompt.clone();
                    self.config.custom_prompt = self.custom_prompt.clone();
                    self.config.save().unwrap();
                }
            });
            ui.separator();
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label("Phím tắt: (hiện chỉ hỗ trợ phím đơn)");
                ui.horizontal(|ui| {
                    ui.label("Nút dịch:");
                    ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.hotkey_translate).desired_width(50.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Nút chọn vùng cần dịch:");
                    ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.hotkey_select).desired_width(50.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Nút chụp nhanh:");
                    ui.add_enabled(!self.started, egui::TextEdit::singleline(&mut self.hotkey_instant).desired_width(50.0));
                });
            });
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_enabled(!self.started, egui::Checkbox::new(&mut self.config.split_tts, "Split TTS"));
                    ui.label("Bắt buộc chọn, chia đoạn văn ra thành từng phần nhỏ để chuyển thành giọng nói rồi ghép lại");
                });
                ui.horizontal(|ui| {
                    ui.add_enabled(false, egui::Checkbox::new(&mut self.config.show_overlay, egui::RichText::new("Hiển thị văn bản dịch trên vùng dịch (chưa làm)").color(egui::Color32::GRAY)));
                });
                ui.add_enabled(!self.started, egui::Slider::new(&mut self.config.speed, 0.0..=2.0).text("Tốc độ đọc"));
            });
            if !self.started && ui.button("Bắt đầu").clicked() {
                // Save config
                self.config.gemini_api_key = self.gemini_api_key.clone();
                self.config.groq_api_key = self.groq_api_key.clone();
                self.config.current_prompt = self.current_prompt.clone();
                self.config.hotkey_translate = self.hotkey_translate.clone();
                self.config.hotkey_select = self.hotkey_select.clone();
                self.config.hotkey_instant = self.hotkey_instant.clone();
                self.config.selected_api = self.selected_api.clone();
                self.config.save().unwrap();

                self.start_service();
                self.started = true;
            }
            ui.colored_label(egui::Color32::RED, "Lưu ý: Muốn dịch game hay app fullscreen thì phải chạy app bằng quyền quản trị");
            ui.colored_label(egui::Color32::RED, "Sau khi nhấn Bắt đầu, muốn điều chỉnh thì tắt app mở lại");
        });

        if self.show_popup {
            let mut open = true;
            egui::Window::new("Hướng dẫn")
                .open(&mut open)
                .show(ctx, |ui| {
                    if self.popup_text == "gemini" {
                        ui.label("Để lấy Gemini API key:");
                        ui.hyperlink_to("1. Vào https://aistudio.google.com/api-keys", "https://aistudio.google.com/api-keys");
                        ui.label("2. Đăng nhập");
                        ui.label("3. Tạo API key");
                        ui.label("4. Sao chép và dán vào");
                    } else if self.popup_text == "groq" {
                        ui.label("Để lấy Groq API key:");
                        ui.hyperlink_to("1. Vào https://console.groq.com/keys", "https://console.groq.com/keys");
                        ui.label("2. Đăng nhập");
                        ui.label("3. Tạo API key");
                        ui.label("4. Sao chép và dán vào");
                    }
                    if ui.button("Đóng").clicked() {
                        self.show_popup = false;
                    }
                });
            if !open {
                self.show_popup = false;
            }
        }
    }
}


impl MainApp {

    fn start_service(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel::<(String, bool, f32)>();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            while let Ok((text, split_tts, speed)) = rx.recv() {
                rt.block_on(async {
                    if let Err(e) = tts::speak(&text, split_tts, speed).await {
                    }
                });
            }
        });
        std::thread::spawn(move || {
            let tx_clone = tx.clone();
            let callback = move |event: Event| {
                let config = config::Config::load();
                if let EventType::KeyPress(_) = event.event_type {
                    if event.name.as_ref() == Some(&config.hotkey_translate) {
                        let tx = tx_clone.clone();
                        std::thread::spawn(move || {
                            let config = config::Config::load();
                            let selected_api = config.selected_api.clone();
                            let gemini_key = config.gemini_api_key.clone();
                            let groq_key = config.groq_api_key.clone();
                            let prompt = config.current_prompt;
                            let regions = config.fixed_regions.clone();
                            let split_tts = config.split_tts;
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                let mut text = String::new();
                                let mut first = true;
                                for region in &regions {
                                    let image_bytes = capture::capture_image(region).unwrap_or_default();
                                    if !image_bytes.is_empty() {
                                        if first {
                                            let _ = std::fs::write("captured.png", &image_bytes);
                                            first = false;
                                        }
                                        let api_key = if selected_api == "gemini" { &gemini_key } else { &groq_key };
                                        if !api_key.is_empty() {
                                            match translation::translate_from_image(&selected_api, api_key, &prompt, &image_bytes).await {
                                                Ok(translated) => {
                                                    text.push_str(&translated);
                                                }
                                                Err(e) => {
                                                    text.push_str("(Translation error)");
                                                }
                                            }
                                        } else {
                                            text.push_str("(No API key set)");
                                        }
                                        text.push(' ');
                                    }
                                }
                                if !text.is_empty() {
                                    println!("{}", text);
                                    let _ = tx.send((text.clone(), split_tts, config.speed));
                                    if config.show_overlay {
                                        let _ = std::fs::write("overlay.txt", &text);
                                    }
                                }
                            });
                        });
                    } else if event.name.as_ref() == Some(&config.hotkey_select) {
                        if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
                            if now - LAST_SELECT.load(Ordering::Relaxed) > 1000 {
                                LAST_SELECT.store(now, Ordering::Relaxed);
                                let _ = std::fs::remove_file("overlay.txt");
                                OVERLAY_ACTIVE.store(true, Ordering::Relaxed);
                                std::thread::spawn(|| {
                                    overlay::show_selection_overlay();
                                    OVERLAY_ACTIVE.store(false, Ordering::Relaxed);
                                });
                            }
                        }
                    } else if event.name.as_ref() == Some(&config.hotkey_instant) {
                        if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                            OVERLAY_ACTIVE.store(true, Ordering::Relaxed);
                            std::thread::spawn(|| {
                                overlay::show_selection_overlay();
                                OVERLAY_ACTIVE.store(false, Ordering::Relaxed);
                                let config = config::Config::load();
                                if let Some(region) = config.fixed_regions.last() {
                                    let image_bytes = capture::capture_image(region).unwrap_or_default();
                                    if !image_bytes.is_empty() {
                                        let api_key = if config.selected_api == "gemini" { &config.gemini_api_key } else { &config.groq_api_key };
                                        if !api_key.is_empty() {
                                            let rt = tokio::runtime::Runtime::new().unwrap();
                                            rt.block_on(async {
                                                match translation::translate_from_image(&config.selected_api, api_key, &config.current_prompt, &image_bytes).await {
                                                    Ok(translated) => {
                                                        println!("{}", translated);
                                                        let _ = tts::speak(&translated, config.split_tts, config.speed).await;
                                                    }
                                                    Err(_) => {
                                                        println!("Translation error");
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                            });
                        }
                    }
                }
            };
            if let Err(error) = listen(callback) {
            }
        });
    }
}


fn main() -> Result<(), eframe::Error> {
    let mut options = eframe::NativeOptions::default();
    options.viewport.transparent = Some(false);
    options.viewport.inner_size = Some(egui::vec2(800.0, 700.0));
    options.viewport.taskbar = Some(true);
    eframe::run_native(
        "Screen Translator",
        options,
        Box::new(|_cc| Box::new(MainApp::default())),
    )
}
