use image::{DynamicImage, ImageFormat, imageops::FilterType, RgbaImage};
use screenshots::Screen;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

// Biến toàn cục để lưu lại "kích thước mũi tên đúng nhất"
static CACHED_TEMPLATE: Mutex<Option<RgbaImage>> = Mutex::new(None);

// Bộ đếm để hạn chế tần suất quét kỹ
static SCAN_COUNTER: AtomicU8 = AtomicU8::new(0);

/// Tìm màn hình chứa điểm (x, y)
fn find_screen_containing(x: i32, y: i32, screens: &[Screen]) -> Option<&Screen> {
    screens.iter().find(|s| {
        let info = s.display_info;
        x >= info.x
        && x < info.x + info.width as i32
        && y >= info.y
        && y < info.y + info.height as i32
    })
}

pub fn capture_image(region: &crate::config::Region) -> Result<Vec<u8>, anyhow::Error> {
    let screens = Screen::all()?;
    if screens.is_empty() { return Err(anyhow::anyhow!("No screens found")); }

    let screen = find_screen_containing(region.x, region.y, &screens)
        .unwrap_or(&screens[0]);

    let relative_x = region.x - screen.display_info.x;
    let relative_y = region.y - screen.display_info.y;

    let image = screen.capture_area(relative_x, relative_y, region.width, region.height)?;
    let img = DynamicImage::ImageRgba8(image);
    let mut buffer = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buffer), ImageFormat::Png)?;
    Ok(buffer)
}

pub fn is_template_present(region: &crate::config::Region, template_bytes: &[u8]) -> bool {
    let screens = match Screen::all() {
        Ok(s) => s,
        Err(_) => return false,
    };
    if screens.is_empty() { return false; }

    let screen = find_screen_containing(region.x, region.y, &screens)
        .unwrap_or(&screens[0]);

    let relative_x = region.x - screen.display_info.x;
    let relative_y = region.y - screen.display_info.y;

    let captured_image = match screen.capture_area(relative_x, relative_y, region.width, region.height) {
        Ok(img) => DynamicImage::ImageRgba8(img),
        Err(_) => return false,
    };

    let haystack = captured_image.to_rgba8();

    let template_original = match image::load_from_memory(template_bytes) {
        Ok(img) => img.to_rgba8(),
        Err(_) => return false,
    };

    // --- CHIẾN LƯỢC QUÉT ---

    // 1. Kiểm tra Cache trước
    {
        let cached = CACHED_TEMPLATE.lock().unwrap();
        if let Some(ref cached_img) = *cached {
            if check_match_at_scale(&haystack, cached_img) {
                SCAN_COUNTER.store(0, Ordering::Relaxed);
                return true;
            }
        }
    }

    // 2. Kiểm tra ảnh gốc
    if check_match_at_scale(&haystack, &template_original) {
        let mut cached = CACHED_TEMPLATE.lock().unwrap();
        *cached = Some(template_original);
        SCAN_COUNTER.store(0, Ordering::Relaxed);
        return true;
    }

    // 3. Quét đa tỉ lệ (Chạy mỗi 5 frame)
    let counter = SCAN_COUNTER.fetch_add(1, Ordering::Relaxed);
    if counter % 5 != 0 {
        return false;
    }

    // Mở rộng dải scale một chút để dễ bắt hơn, nhưng vẫn tránh mức 0.5 (quá nhỏ)
    let scales = [
        1.0,
        0.95, 1.05,
        0.9, 1.1,
        0.8, 1.2,
        0.75, 1.25,
        0.7, 1.3
    ];

    for scale in scales {
        let (orig_w, orig_h) = template_original.dimensions();
        let new_w = (orig_w as f32 * scale) as u32;
        let new_h = (orig_h as f32 * scale) as u32;

        if new_w < 5 || new_h < 5 { continue; }

        let template_resized = image::imageops::resize(
            &template_original,
            new_w,
            new_h,
            FilterType::Lanczos3
        );

        if check_match_at_scale(&haystack, &template_resized) {
            let mut cached = CACHED_TEMPLATE.lock().unwrap();
            *cached = Some(template_resized);
            SCAN_COUNTER.store(0, Ordering::Relaxed);
            return true;
        }
    }

    false
}

fn check_match_at_scale(haystack: &RgbaImage, needle: &RgbaImage) -> bool {
    let (w_h, h_h) = haystack.dimensions();
    let (w_n, h_n) = needle.dimensions();

    if w_h < w_n || h_h < h_n { return false; }

    // --- CẤU HÌNH ĐỘ DỄ TÍNH ---

    // Color Tolerance: Tăng lên 80 (Rất cao)
    // Cho phép mũi tên màu trắng bị mờ xuống màu xám đậm vẫn nhận ra.
    let color_tolerance = 80;

    // Match Threshold: Giảm xuống 0.70 (70%)
    // Chỉ cần khớp 70% diện tích là được.
    let match_threshold = 0.70;

    let limit_x = w_h - w_n;
    let limit_y = h_h - h_n;

    // Quét qua haystack (Nhảy bước 2 pixel để tìm nhanh)
    for y in (0..=limit_y).step_by(2) {
        for x in (0..=limit_x).step_by(2) {
            if fuzzy_match(haystack, needle, x, y, color_tolerance, match_threshold) {
                return true;
            }
        }
    }
    false
}

fn fuzzy_match(haystack: &RgbaImage, needle: &RgbaImage, sx: u32, sy: u32, tol: i16, threshold: f32) -> bool {
    let (w, h) = needle.dimensions();
    let mut total_checked = 0;
    let mut matched_count = 0;

    // Quét từng pixel của mũi tên mẫu
    for y in 0..h {
        for x in 0..w {
            let p_n = needle.get_pixel(x, y);

            // Chỉ kiểm tra những điểm thực sự là mũi tên (Alpha > 50)
            if p_n[3] < 50 { continue; }

            total_checked += 1;
            let p_h = haystack.get_pixel(sx + x, sy + y);

            if pixel_diff(p_h, p_n) <= tol {
                matched_count += 1;
            }
        }
    }

    if total_checked == 0 { return true; }

    // Tính tỉ lệ khớp
    (matched_count as f32 / total_checked as f32) >= threshold
}

fn pixel_diff(p1: &image::Rgba<u8>, p2: &image::Rgba<u8>) -> i16 {
    let r = (p1[0] as i16 - p2[0] as i16).abs();
    let g = (p1[1] as i16 - p2[1] as i16).abs();
    let b = (p1[2] as i16 - p2[2] as i16).abs();

    // Dùng trung bình cộng thay vì max để dễ tính hơn với các điểm nhiễu
    (r + g + b) / 3
}