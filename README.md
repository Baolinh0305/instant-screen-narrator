# Screen Translation Tool

A tool to select a screen region, OCR English text, translate to Vietnamese, and play the audio.

## Features
- Select screen region with GUI
- OCR using Tesseract
- Translate using Google Gemini API and Groq API
- Text-to-speech in Vietnamese
- Hotkeys: `[` to translate, `]` to reselect region

## Files
- `screen_translator.py`: Main script
- `config.txt`: Configuration file
- `requirements.txt`: Python dependencies
- `run_translator.vbs`: Launcher (runs minimized)
- `run_translator.bat`: Alternative launcher
- `run_exe.vbs`: EXE launcher
- `dist/screen_translator.exe`: Compiled EXE
- `Tesseract-OCR/`: Portable Tesseract (copy from system)
- `README.md`: This file

## Notes
- Requires administrator privileges for hotkeys
- Audio plays in background
- Tested on Windows 11

## Vietnamese Version

# Công Cụ Dịch Màn Hình

Một công cụ để chọn vùng màn hình, OCR văn bản tiếng Anh, dịch sang tiếng Việt và phát âm thanh.

## Tính Năng
- Chọn vùng màn hình với GUI
- OCR sử dụng Tesseract
- Dịch sử dụng Google Gemini API và Groq API
- Chuyển văn bản thành giọng nói bằng tiếng Việt
- Phím nóng: `[` để dịch, `]` để chọn lại vùng

## Tệp
- `screen_translator.py`: Tập lệnh chính
- `config.txt`: Tệp cấu hình
- `requirements.txt`: Các phụ thuộc Python
- `run_translator.vbs`: Trình khởi chạy (chạy ẩn)
- `run_translator.bat`: Trình khởi chạy thay thế
- `run_exe.vbs`: Trình khởi chạy EXE
- `dist/screen_translator.exe`: EXE đã biên dịch
- `Tesseract-OCR/`: Tesseract di động (sao chép từ hệ thống)
- `README.md`: Tệp này

## Ghi Chú
- Yêu cầu quyền quản trị viên cho phím nóng
- Âm thanh phát ở nền
- Đã thử nghiệm trên Windows 11