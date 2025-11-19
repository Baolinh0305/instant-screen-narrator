# Screen Translation Tool

A tool to select a screen region, OCR English text, translate to Vietnamese, and play the audio.

## Features
- Select screen region with GUI
- OCR using Tesseract
- Translate using Google Gemini API
- Text-to-speech in Vietnamese
- Hotkeys: `[` to translate, `]` to reselect region

## Setup

1. Install Python 3.x

2. Install dependencies:
   ```
   pip install -r requirements.txt
   ```

3. Copy Tesseract-OCR:
   - Copy `C:\Program Files\Tesseract-OCR` to the project folder as `Tesseract-OCR`

4. Configure settings:
   - Edit `config.txt` to set your API key, hotkeys, and prompt

5. Run the app:
   - For source: Double-click `run_translator.vbs`
   - For EXE: Double-click `run_exe.vbs` (requires copying Tesseract-OCR first)
   - Press `]` to select region
   - Press `[` to translate

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
- For fullscreen video games, the selection window may not overlay properly; try running the game in windowed mode