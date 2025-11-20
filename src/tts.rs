use reqwest;
use rodio::{Decoder, OutputStream, Sink, Source};
use std::io::Cursor;
use regex::Regex;
use urlencoding;
use futures::future::join_all;

pub async fn speak(text: &str, split: bool, speed: f32) -> Result<(), anyhow::Error> {
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
        let bytes = result??;
        let sink = Sink::try_new(&stream_handle)?;
        let cursor = Cursor::new(bytes);
        let source = Decoder::new_mp3(cursor)?;
        let sped_source = source.speed(speed);
        sink.append(sped_source);
        sink.sleep_until_end();
    }
    Ok(())
}