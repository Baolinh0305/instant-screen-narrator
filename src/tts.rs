use reqwest;
use rodio::{Decoder, OutputStream, Sink, Source};
use std::io::Cursor;
use regex::Regex;
use urlencoding;
use futures::future::join_all;
use std::sync::atomic::{AtomicBool, Ordering};

// Biến cờ để báo hiệu dừng đọc
static STOP_SIGNAL: AtomicBool = AtomicBool::new(false);

// Hàm gọi từ bên ngoài để dừng đọc
pub fn stop() {
    STOP_SIGNAL.store(true, Ordering::Relaxed);
}

pub async fn speak(text: &str, split: bool, speed: f32, use_tts: bool) -> Result<(), anyhow::Error> {
    if use_tts {
        // Reset cờ dừng trước khi bắt đầu câu mới
        STOP_SIGNAL.store(false, Ordering::Relaxed);
        speak_gtts(text, split, speed).await
    } else {
        Ok(())
    }
}

async fn speak_gtts(text: &str, split: bool, speed: f32) -> Result<(), anyhow::Error> {
    let parts: Vec<String> = if split {
        let re = Regex::new(r"[,.]")?;
        re.split(text).map(|s| s.to_string()).collect()
    } else {
        vec![text.to_string()]
    };

    // Tải song song tất cả các đoạn audio
    let (_stream, stream_handle) = OutputStream::try_default()?;
    let handles: Vec<_> = parts.into_iter().filter(|p| !p.trim().is_empty()).map(|part| {
        tokio::spawn(async move {
            let url = format!(
                "https://translate.google.com/translate_tts?ie=UTF-8&q={}&tl=vi&client=tw-ob",
                urlencoding::encode(&part.trim())
            );
            let response = reqwest::get(&url).await?;
            response.bytes().await
        })
    }).collect();

    let results = join_all(handles).await;

    for result in results {
        // Kiểm tra cờ dừng trước khi phát đoạn tiếp theo
        if STOP_SIGNAL.load(Ordering::Relaxed) {
            break;
        }

        let bytes = result??;
        let sink = Sink::try_new(&stream_handle)?;
        let cursor = Cursor::new(bytes);
        let source = Decoder::new_mp3(cursor)?;
        let sped_source = source.speed(speed);
        
        sink.append(sped_source);
        
        // Vòng lặp chờ audio chạy xong, nhưng có kiểm tra cờ dừng liên tục
        while !sink.empty() {
            if STOP_SIGNAL.load(Ordering::Relaxed) {
                sink.stop(); // Dừng ngay lập tức
                return Ok(());
            }
            // Ngủ ngắn để không chiếm CPU
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    Ok(())
}

// 1. Chỉ tải dữ liệu
pub async fn download_audio(text: String) -> Result<Vec<u8>, anyhow::Error> {
    if text.trim().is_empty() { return Ok(Vec::new()); }
    let url = format!(
        "https://translate.google.com/translate_tts?ie=UTF-8&q={}&tl=vi&client=tw-ob",
        urlencoding::encode(&text.trim())
    );
    let response = reqwest::get(&url).await?;
    let bytes = response.bytes().await?;
    Ok(bytes.to_vec())
}

// 2. Phát dữ liệu (Blocking)
pub fn play_audio_data(data: Vec<u8>, speed: f32) -> Result<(), anyhow::Error> {
    if data.is_empty() { return Ok(()); }
    // Reset cờ trước khi phát
    STOP_SIGNAL.store(false, Ordering::Relaxed);

    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;
    let cursor = Cursor::new(data);
    let source = Decoder::new_mp3(cursor)?;
    let sped_source = source.speed(speed);
    
    sink.append(sped_source);
    
    while !sink.empty() {
        if STOP_SIGNAL.load(Ordering::Relaxed) {
            sink.stop();
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}