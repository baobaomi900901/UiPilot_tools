@echo off
setlocal
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\package-console.ps1"
set "packExit=%errorlevel%"
pause
exit /b %packExit%
