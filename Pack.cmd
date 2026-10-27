@echo off
setlocal
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -STA -WindowStyle Hidden -File "%~dp0scripts\package-gui.ps1"
if errorlevel 1 (
  "%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -WindowStyle Hidden -Command "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.MessageBox]::Show(('UiPilot packaging tool could not start. Check scripts/package-gui.ps1 and ' + [IO.Path]::Combine([IO.Path]::GetTempPath(), 'uipilot-package-startup.log')), 'UiPilot packaging tool') | Out-Null"
  exit /b 1
)
