use reqwest;
use rodio::{Decoder, OutputStream, Sink, Source};
use std::io::Cursor;
use regex::Regex;
use urlencoding;
use futures::future::join_all;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use winapi::um::winuser::{PostMessageW, WM_CLOSE};
use winapi::shared::windef::HWND;

// Map lưu trạng thái dừng của từng ID (Request ID -> Token dừng)
static STOP_TOKENS: OnceLock<Mutex<HashMap<u64, Arc<AtomicBool>>>> = OnceLock::new();
// Map lưu HWND của cửa sổ Overlay tương ứng với ID (Request ID -> HWND)
static WINDOW_HANDLES: OnceLock<Mutex<HashMap<u64, usize>>> = OnceLock::new();

pub fn register_window(id: u64, hwnd: usize) {
    let map = WINDOW_HANDLES.get_or_init(|| Mutex::new(HashMap::new()));
    map.lock().unwrap().insert(id, hwnd);
}

pub fn unregister_window(id: u64) {
    if let Some(map) = WINDOW_HANDLES.get() {
        map.lock().unwrap().remove(&id);
    }
}

pub fn stop_id(id: u64) {
    if let Some(map) = STOP_TOKENS.get() {
        if let Some(token) = map.lock().unwrap().get(&id) {
            token.store(true, Ordering::Relaxed);
        }
    }
}

pub fn stop_all() {
    if let Some(map) = STOP_TOKENS.get() {
        for token in map.lock().unwrap().values() {
            token.store(true, Ordering::Relaxed);
        }
    }
}

// Giữ lại hàm stop() cũ để tương thích
pub fn stop() {
    stop_all();
}

pub async fn speak(text: &str, split: bool, speed: f32, use_tts: bool, req_id: u64) -> Result<(), anyhow::Error> {
    if use_tts {
        // Tạo token dừng riêng cho request này
        let stop_token = Arc::new(AtomicBool::new(false));
        {
            let map = STOP_TOKENS.get_or_init(|| Mutex::new(HashMap::new()));
            map.lock().unwrap().insert(req_id, stop_token.clone());
        }

        let res = speak_gtts(text, split, speed, stop_token.clone()).await;

        // Xóa token sau khi chạy xong
        if let Some(map) = STOP_TOKENS.get() {
            map.lock().unwrap().remove(&req_id);
        }

        // Nếu chạy xong mà không bị dừng đột ngột (do người dùng click), gửi lệnh đóng cửa sổ
        if !stop_token.load(Ordering::Relaxed) {
             if let Some(map) = WINDOW_HANDLES.get() {
                 if let Some(&hwnd_ptr) = map.lock().unwrap().get(&req_id) {
                     unsafe {
                         // Gửi lệnh đóng cửa sổ
                         PostMessageW(hwnd_ptr as HWND, WM_CLOSE, 0, 0);
                     }
                 }
             }
        }
        res
    } else {
        Ok(())
    }
}

async fn speak_gtts(text: &str, split: bool, speed: f32, stop_token: Arc<AtomicBool>) -> Result<(), anyhow::Error> {
    let parts: Vec<String> = if split {
        let re = Regex::new(r"[,.]")?;
        re.split(text).map(|s| s.to_string()).collect()
    } else {
        vec![text.to_string()]
    };

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
        if stop_token.load(Ordering::Relaxed) { break; }

        if let Ok(Ok(bytes)) = result {
            let sink = Sink::try_new(&stream_handle)?;
            let cursor = Cursor::new(bytes);
            let source = Decoder::new_mp3(cursor)?;
            let sped_source = source.speed(speed);

            sink.append(sped_source);

            while !sink.empty() {
                if stop_token.load(Ordering::Relaxed) {
                    sink.stop();
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
    Ok(())
}

// Giữ lại các hàm download_audio/play_audio_data cho ReaderWindow (không đổi)
pub async fn download_audio(text: String) -> Result<Vec<u8>, anyhow::Error> {
    if text.trim().is_empty() { return Ok(Vec::new()); }
    let url = format!("https://translate.google.com/translate_tts?ie=UTF-8&q={}&tl=vi&client=tw-ob", urlencoding::encode(&text.trim()));
    let response = reqwest::get(&url).await?;
    let bytes = response.bytes().await?;
    Ok(bytes.to_vec())
}

pub fn play_audio_data(data: Vec<u8>, speed: f32) -> Result<(), anyhow::Error> {
    // Logic cũ cho reader
    if data.is_empty() { return Ok(()); }
    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;
    let cursor = Cursor::new(data);
    let source = Decoder::new_mp3(cursor)?;
    sink.append(source.speed(speed));
    sink.sleep_until_end();
    Ok(())
}