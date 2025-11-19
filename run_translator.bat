@echo off
powershell -Command "Start-Process python -ArgumentList 'screen_translator.py' -Verb RunAs -Wait -WindowStyle Minimized"
pause