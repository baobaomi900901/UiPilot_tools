; Compile-time path, never a runtime caller-selected executable.
!define UIPILOT_INSTALL_CHECK "${__FILEDIR__}\..\..\spikes\process-elevation-feasibility\target\release\uipilot-elevation-install-check.exe"

!macro UIPILOT_CHECK_DIRECTORY Phase
  Push $OUTDIR
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /oname=uipilot-install-check.exe "${UIPILOT_INSTALL_CHECK}"
  ; nsExec loads the executable directly; no shell, cmd, or PowerShell.
  nsExec::ExecToStack '"$PLUGINSDIR\uipilot-install-check.exe" ${Phase} "$INSTDIR"'
  Pop $R0
  Pop $R1
  ${If} $R0 != 0
    MessageBox MB_OK|MB_ICONSTOP "The protected installation directory could not be verified. No permissions will be changed automatically."
    Abort
  ${EndIf}
  Pop $R2
  SetOutPath "$R2"
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro UIPILOT_CHECK_DIRECTORY preinstall
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro UIPILOT_CHECK_DIRECTORY postinstall
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro UIPILOT_CHECK_DIRECTORY preuninstall
!macroend
