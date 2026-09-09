; Compile only. This test installer must never be distributed or executed.
Unicode true
!include "LogicLib.nsh"
!include "..\..\src-tauri\windows\elevation-installer-hooks.nsh"
Name "UiPilot hook compilation test"
OutFile "target\hook-compile-test.exe"
RequestExecutionLevel admin
InstallDir "$PROGRAMFILES64\UiPilot_tools Protected"
Section
  !insertmacro NSIS_HOOK_PREINSTALL
  !insertmacro NSIS_HOOK_POSTINSTALL
  WriteUninstaller "$INSTDIR\uninstall-test.exe"
SectionEnd
Section "Uninstall"
  !insertmacro NSIS_HOOK_PREUNINSTALL
SectionEnd
