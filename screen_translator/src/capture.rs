use image::{DynamicImage, ImageFormat};
use screenshots::Screen;

pub fn capture_image(region: &crate::config::Region) -> Result<Vec<u8>, anyhow::Error> {
    let screens = Screen::all()?;
    let screen = &screens[0];
    let image = screen.capture_area(region.x as i32, region.y as i32, region.width, region.height)?;
    let img = DynamicImage::ImageRgba8(image);
    let mut buffer = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buffer), ImageFormat::Png)?;
    Ok(buffer)
}