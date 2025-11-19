import tkinter as tk
from tkinter import messagebox
import pyautogui
import pytesseract
from PIL import Image, ImageTk
from gtts import gTTS
import keyboard
import subprocess
import os
import threading
from playsound import playsound
import time

# Load config
config = {}
with open('config.txt', 'r') as f:
    for line in f:
        if '=' in line:
            key, value = line.split('=', 1)
            config[key.strip()] = value.strip()

# Configure Tesseract path (portable)
os.environ['TESSDATA_PREFIX'] = r'.\Tesseract-OCR\tessdata'
pytesseract.pytesseract.tesseract_cmd = r'.\Tesseract-OCR\tesseract.exe'

# Global variables for region selection
start_x = None
start_y = None
end_x = None
end_y = None
selecting = False
canvas = None
rect = None
root = None
api_choice = None
client = None
is_playing = False
audio_queue = []

def on_mouse_down(event):
    global start_x, start_y, selecting, rect
    start_x = event.x
    start_y = event.y
    selecting = True
    if rect:
        canvas.delete(rect)
    rect = canvas.create_rectangle(start_x, start_y, start_x, start_y, outline='red', width=2)

def on_mouse_move(event):
    global rect
    if selecting and rect:
        canvas.coords(rect, start_x, start_y, event.x, event.y)

def on_mouse_up(event):
    global end_x, end_y, selecting
    end_x = event.x
    end_y = event.y
    selecting = False
    root.destroy()

def select_region():
    global canvas, root
    root = tk.Tk()
    root.attributes('-fullscreen', True)
    root.attributes('-alpha', 0.3)
    root.attributes('-topmost', True)
    root.configure(bg='black')
    root.focus_force()

    canvas = tk.Canvas(root, bg='black', highlightthickness=0)
    canvas.pack(fill=tk.BOTH, expand=True)

    canvas.bind('<Button-1>', on_mouse_down)
    canvas.bind('<B1-Motion>', on_mouse_move)
    canvas.bind('<ButtonRelease-1>', on_mouse_up)

    root.mainloop()

def ocr_image(image):
    import subprocess
    result = subprocess.run([r'.\Tesseract-OCR\tesseract.exe', 'temp.png', 'stdout', '-l', 'eng', '--psm', '3'], capture_output=True, text=True)
    if result.returncode == 0:
        return result.stdout.strip()
    else:
        print(f"Tesseract error: {result.stderr}")
        return ""

def translate_text(text):
    print(f"Translating with {api_choice}")
    prompt = config['prompt'] + "\n\n" + text
    if api_choice == 'groq':
        response = client.chat.completions.create(
            model="llama-3.1-8b-instant",
            messages=[{"role": "user", "content": prompt}]
        )
        return response.choices[0].message.content.strip()
    else:
        response = model.generate_content(prompt)
        return response.text.strip()

def text_to_speech(text):
    filename = f'output_{int(time.time())}.mp3'
    tts = gTTS(text=text, lang='vi', slow=False)
    tts.save(filename)
    return filename

def play_audio(filename='output.mp3'):
    global is_playing, audio_queue
    if is_playing:
        audio_queue.append(filename)
        return
    is_playing = True
    try:
        playsound(filename)
    except Exception as e:
        print(f"Play error: {e}")
    finally:
        is_playing = False
        try:
            os.remove(filename)
        except Exception as e:
            print(f"Remove error: {e}")
        if audio_queue:
            next_file = audio_queue.pop(0)
            play_audio(next_file)

def process_translation():
    print("Processing translation...")
    if start_x is None or end_x is None:
        print("No region selected")
        return

    # Take screenshot of selected region
    region = (min(start_x, end_x), min(start_y, end_y), abs(end_x - start_x), abs(end_y - start_y))
    screenshot = pyautogui.screenshot(region=region)
    screenshot.save('temp.png')

    # OCR
    try:
        image = Image.open('temp.png')
        text = ocr_image(image)
        print(f"OCR Text: '{text}'")
    except Exception as e:
        print(f"OCR error: {e}")
        return

    if not text:
        print("No text found")
        return

    # Translate
    try:
        translated = translate_text(text)
        print(f"Translated: {translated}")
    except Exception as e:
        print(f"Translation error: {e}")
        return

    # TTS
    try:
        if config.get('split_tts', 'true') == 'true':
            import re
            # Split translated text into sentences by periods
            parts = re.split(r'([.])', translated)
            combined_parts = []
            for i in range(0, len(parts), 2):
                part = parts[i]
                if i + 1 < len(parts):
                    part += parts[i + 1]
                if part.strip() and re.search(r'[a-zA-Z]', part):
                    combined_parts.append(part.strip())
            if not combined_parts:
                combined_parts = [translated]
            # Generate TTS in parallel
            filenames = [None] * len(combined_parts)
            def generate_tts(i, part):
                if not part.strip():
                    return
                filename = f'output_{int(time.time())}_{i}.mp3'
                tts = gTTS(text=part, lang='vi', slow=False)
                tts.save(filename)
                filenames[i] = filename
            threads = []
            for i, part in enumerate(combined_parts):
                t = threading.Thread(target=generate_tts, args=(i, part))
                threads.append(t)
                t.start()
            for t in threads:
                t.join()
            for filename in filenames:
                if filename:
                    play_audio(filename)
        else:
            filename = text_to_speech(translated)
            play_audio(filename)
    except Exception as e:
        print(f"TTS error: {e}")

def on_key_press():
    threading.Thread(target=process_translation).start()

def save_config():
    current_key = api_key_entry.get()
    if selected_api.get() == "Gemini (Dịch có vẻ ổn)":
        config['api_key'] = current_key
    else:
        config['groq_api_key'] = current_key
    config['api_choice'] = 'gemini' if selected_api.get() == "Gemini (Dịch có vẻ ổn)" else 'groq'
    config['prompt'] = prompt_text.get("1.0", tk.END).strip()
    config['split_tts'] = 'true' if split_var.get() else 'false'
    with open('config.txt', 'w') as f:
        f.write(f"translate_key = {config['translate_key']}\n")
        f.write(f"select_key = {config['select_key']}\n")
        f.write(f"api_key = {config['api_key']}\n")
        f.write(f"groq_api_key = {config['groq_api_key']}\n")
        f.write(f"api_choice = {config['api_choice']}\n")
        f.write(f"prompt = {config['prompt']}\n")

def start_app():
    global client, model
    print(f"Using API: {api_choice}")
    if api_choice == 'groq':
        import groq
        client = groq.Groq(api_key=config['groq_api_key'])
    else:
        import google.generativeai as genai
        genai.configure(api_key=config['api_key'])
        model = genai.GenerativeModel('gemini-2.5-flash-lite')
    root.destroy()
    keyboard.add_hotkey(config['translate_key'], on_key_press)
    keyboard.add_hotkey(config['select_key'], select_region)
    keyboard.wait()

def set_translate_key():
    global waiting_for_key
    waiting_for_key = 'translate'
    status_label.config(text="Press the key you want to bind for Translate")

def set_select_key():
    global waiting_for_key
    waiting_for_key = 'select'
    status_label.config(text="Press the key you want to bind for Select Region")

def set_api_choice(choice):
    global api_choice
    api_choice = 'gemini' if choice == "Gemini (Dịch có vẻ ổn)" else 'groq'

def set_fixed_region():
    global start_x, start_y, end_x, end_y
    # Original coordinates based on 1920x1080 screen
    # top-left: 191,923 bottom-right: 1750,1050
    screen_width, screen_height = pyautogui.size()
    start_x = int(191 / 1920 * screen_width)
    start_y = int(923 / 1080 * screen_height)
    end_x = int(1750 / 1920 * screen_width)
    end_y = int(1050 / 1080 * screen_height)

def set_fixed_region_larger():
    global start_x, start_y, end_x, end_y
    # Larger coordinates based on 1920x1080 screen
    # top-left: 277,822 bottom-right: 1628,1061
    screen_width, screen_height = pyautogui.size()
    start_x = int(277 / 1920 * screen_width)
    start_y = int(822 / 1080 * screen_height)
    end_x = int(1628 / 1920 * screen_width)
    end_y = int(1061 / 1080 * screen_height)

waiting_for_key = None

def on_key_event(e):
    global waiting_for_key
    if waiting_for_key:
        key = e.name
        if waiting_for_key == 'translate':
            config['translate_key'] = key
            translate_key_label.config(text=f"Translate Key (Nhấn nút này để dịch): {key}")
        elif waiting_for_key == 'select':
            config['select_key'] = key
            select_key_label.config(text=f"Select Key (Nhấn nút này để chọn vùng cần dịch): {key}")
        waiting_for_key = None
        status_label.config(text="Key bound successfully")

def show_api_instructions():
    import webbrowser
    instructions = tk.Toplevel(root)
    instructions.title("Cách lấy Gemini API key")
    instructions.geometry("600x300")
    tk.Label(instructions, text="Truy cập vào", font=font).pack(pady=5)
    link_label = tk.Label(instructions, text="https://aistudio.google.com/api-keys", fg="blue", cursor="hand2", font=font)
    link_label.pack(pady=5)
    link_label.bind("<Button-1>", lambda e: webbrowser.open("https://aistudio.google.com/api-keys"))
    tk.Label(instructions, text="Đăng nhập -> Create API key -> Nhập tên project và tên key -> Create key -> Copy key dán vào", font=font).pack(pady=5)

def show_groq_instructions():
    import webbrowser
    instructions = tk.Toplevel(root)
    instructions.title("Cách lấy Groq API Key")
    instructions.geometry("600x300")
    tk.Label(instructions, text="Truy cập vào", font=font).pack(pady=5)
    link_label = tk.Label(instructions, text="https://console.groq.com/login", fg="blue", cursor="hand2", font=font)
    link_label.pack(pady=5)
    link_label.bind("<Button-1>", lambda e: webbrowser.open("https://console.groq.com/login"))
    tk.Label(instructions, text="Đăng nhập -> Go to API Keys -> Create API Key -> Copy key dán vào", font=font).pack(pady=5)

def reset_prompt():
    default_prompt = "Translate the following English text to Vietnamese. The translation must strictly use vocabulary and tone consistent with wuxia novels, make it as short as possible. Crucially, provide ONLY the translated text and nothing else. Do not include any introductory phrases, explanations, or conversational elements. Note: just output the translated text and make it as short as possible"
    prompt_text.delete("1.0", tk.END)
    prompt_text.insert(tk.END, default_prompt)

def reset_normal_prompt():
    normal_prompt = "Translate the following English text to Vietnamese. Provide only the translated text without any additional explanations or notes:"
    prompt_text.delete("1.0", tk.END)
    prompt_text.insert(tk.END, normal_prompt)

def show_image(path):
    img_window = tk.Toplevel(root)
    img_window.title("Region Image")
    img = Image.open(path)
    photo = ImageTk.PhotoImage(img)
    label = tk.Label(img_window, image=photo)
    label.image = photo
    label.pack()
    img_window.attributes('-fullscreen', True)  # Fullscreen to avoid taskbar
    label.bind("<Button-1>", lambda e: img_window.destroy())  # Close on click

if __name__ == "__main__":
    import tkinter as tk
    root = tk.Tk()
    root.title("Screen Translator Setup")
    root.geometry("700x630")

    font = ("font", 10)

    # Translate key frame
    translate_frame = tk.Frame(root)
    translate_frame.pack(pady=5)
    translate_key_label = tk.Label(translate_frame, text=f"Translate Key (Nhấn nút này để dịch): {config['translate_key']}", font=font)
    translate_key_label.pack(side=tk.LEFT, padx=5)
    tk.Button(translate_frame, text="Change", command=set_translate_key, font=font).pack(side=tk.LEFT)

    # Select key frame
    select_frame = tk.Frame(root)
    select_frame.pack(pady=5)
    select_key_label = tk.Label(select_frame, text=f"Select Key (Nhấn nút này để chọn vùng cần dịch): {config['select_key']}", font=font)
    select_key_label.pack(side=tk.LEFT, padx=5)
    tk.Button(select_frame, text="Change", command=set_select_key, font=font).pack(side=tk.LEFT)

    api_choice_config = config.get('api_choice', 'gemini')
    initial_display = "Gemini (Dịch có vẻ ổn)" if api_choice_config == 'gemini' else "Groq (Nhanh hơn Gemini)"
    selected_api = tk.StringVar(value=initial_display)

    tk.Label(root, text="Select API:", font=font).pack(pady=5)
    import tkinter.ttk as ttk
    api_menu = ttk.Combobox(root, textvariable=selected_api, values=["Gemini (Dịch có vẻ ổn)", "Groq (Nhanh hơn Gemini)"], state='readonly', font=font)
    api_menu.pack(pady=5)
    api_menu.bind("<<ComboboxSelected>>", lambda e: update_api_fields())

    api_container = tk.Frame(root)
    api_container.pack(pady=5)

    api_label = tk.Label(api_container, text="", font=font)
    api_label.pack(side=tk.LEFT, padx=5)
    api_key_entry = tk.Entry(api_container, width=40, font=font)
    api_key_entry.pack(side=tk.LEFT, padx=5)
    api_button = tk.Button(api_container, text="", command=None, font=font)
    api_button.pack(side=tk.LEFT)

    def update_api_fields(*args):
        if selected_api.get() == "Gemini (Dịch có vẻ ổn)":
            api_label.config(text="Gemini API Key:")
            api_key_entry.delete(0, tk.END)
            api_key_entry.insert(0, config['api_key'])
            api_button.config(text="Cách lấy API key", command=show_api_instructions)
        else:
            api_label.config(text="Groq API Key:")
            api_key_entry.delete(0, tk.END)
            api_key_entry.insert(0, config['groq_api_key'])
            api_button.config(text="Cách lấy Groq API Key", command=show_groq_instructions)

    selected_api.trace_add('write', update_api_fields)

    update_api_fields()

    tk.Label(root, text="Translation Prompt (Cần thì thay đổi) \n \n Lưu ý: con Groq lâu lâu dịch hơi lỏ và lan man mặc dù chung một prompt với Gemini \n Ai biết thì sửa cái prompt bên dưới cho đỡ lỏ :v", font=font).pack(pady=5)
    prompt_text = tk.Text(root, height=4, width=50, font=font)
    prompt_text.insert(tk.END, config['prompt'])
    prompt_text.pack(pady=5)
    prompt_frame = tk.Frame(root)
    prompt_frame.pack(pady=5)
    tk.Button(prompt_frame, text="Dịch kiểu kiếm hiệp", command=reset_prompt, font=font).pack(side=tk.LEFT)
    tk.Button(prompt_frame, text="Dịch bình thường", command=reset_normal_prompt, font=font).pack(side=tk.LEFT)

    split_var = tk.BooleanVar(value=config.get('split_tts', 'true').lower() == 'true')
    tk.Checkbutton(root, text="Tách câu để TTS nhanh hơn (nên chọn, nó sẽ convert tts từng thành phần trong câu để bắt đầu nhanh hơn)", variable=split_var, font=font).pack(pady=5)

    status_label = tk.Label(root, text="", font=font)
    status_label.pack(pady=5)

    fixed_frame = tk.Frame(root)
    fixed_frame.pack(pady=5)
    tk.Button(fixed_frame, text="Start with Fixed Region", command=lambda: [set_fixed_region(), save_config(), set_api_choice(selected_api.get()), start_app()], font=font).pack(side=tk.LEFT)
    tk.Label(fixed_frame, text="Tự động chọn vùng cần dịch như trong ảnh (cho màn 16:9)", font=font).pack(side=tk.LEFT, padx=10)
    tk.Button(fixed_frame, text="Xem ảnh", command=lambda: show_image("area.png"), font=font).pack(side=tk.LEFT)

    larger_frame = tk.Frame(root)
    larger_frame.pack(pady=5)
    tk.Button(larger_frame, text="Start with Fixed Region but Larger", command=lambda: [set_fixed_region_larger(), save_config(), set_api_choice(selected_api.get()), start_app()], font=font).pack(side=tk.LEFT)
    tk.Label(larger_frame, text="Tự động chọn vùng cần dịch như trong ảnh (cho màn 16:9)", font=font).pack(side=tk.LEFT, padx=10)
    tk.Button(larger_frame, text="Xem ảnh", command=lambda: show_image("area2.png"), font=font).pack(side=tk.LEFT)

    start_frame = tk.Frame(root)
    start_frame.pack(pady=10)
    tk.Button(start_frame, text="Start", command=lambda: [save_config(), set_api_choice(selected_api.get()), start_app()], font=font).pack(side=tk.LEFT)
    tk.Label(start_frame, text="Bạn tự chọn vùng cần dịch", font=font).pack(side=tk.LEFT, padx=10)

    tk.Label(root, text="Chưa làm chức năng auto dịch do chưa nghĩ ra cách làm nên mỗi lần đến đoạn hội thoại mới thì bạn nhấn nút dịch", font=font, fg="red").pack(pady=5)
    tk.Label(root, text="Cũng chưa biết cách làm cho giọng nói nhanh hơn, khi nào biết sẽ update", font=font, fg="red").pack(pady=5)

    keyboard.on_press(on_key_event)
    root.mainloop()