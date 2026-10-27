@echo off
setlocal
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -STA -WindowStyle Hidden -File "%~dp0scripts\package-gui.ps1"
if errorlevel 1 (
  echo UiPilot packaging tool failed. See %%TEMP%%\uipilot-package-startup.log
  pause
)
