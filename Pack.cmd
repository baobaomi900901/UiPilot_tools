@echo off
setlocal
where py >nul 2>nul
if not errorlevel 1 (
  py -3 -X utf8 "%~dp0scripts\package.py"
  goto finished
)
where python >nul 2>nul
if not errorlevel 1 (
  python -X utf8 "%~dp0scripts\package.py"
  goto finished
)
echo Python 3.10+ is required. Install Python and run Pack.cmd again.
pause
exit /b 1
:finished
set "packExit=%errorlevel%"
pause
exit /b %packExit%
