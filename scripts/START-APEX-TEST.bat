@echo off
rem APEX Velox Test Center - double-click to open the benchmark / feature menu.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0APEX-TestCenter.ps1"
if errorlevel 1 pause
