; UiPilot managed Everything native Helper lifecycle.
; This file deliberately uses only native Win32 calls and the embedded Rust Helper.

!define UIPILOT_MUTEX_NAME "Global\UiPilotManagedEverythingInstaller"
!define UIPILOT_ACTIVITY_NAME "Global\UiPilotManagedEverythingInstallerActivity"
!define UIPILOT_NAMED_OBJECT_SDDL "O:BAG:BAD:P(A;;GA;;;SY)(A;;GA;;;BA)"
!define UIPILOT_STAGING_SDDL "D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)"
!define MUI_CUSTOMFUNCTION_ABORT UiPilotManagedEverythingOnAbort
!define MUI_CUSTOMFUNCTION_UNABORT un.UiPilotManagedEverythingOnAbort

Var UiPilotMutexHandle
Var UiPilotActivityHandle
Var UiPilotStaging
Var UiPilotHelper
Var UiPilotRequest
Var UiPilotJournal
Var UiPilotOperationId
Var UiPilotParentPid
Var UiPilotParentCreationTime
Var UiPilotHelperExit
Var UiPilotRecoveryId
Var UiPilotOwnerSid

!macro UIPILOT_LOCAL_FREE POINTER
  ${If} ${POINTER} != 0
    System::Call 'kernel32::LocalFree(p ${POINTER})'
  ${EndIf}
!macroend

!macro UIPILOT_VALIDATE_NAMED_OBJECT HANDLE
  StrCpy $R8 0
  StrCpy $R9 0
  System::Call 'advapi32::GetSecurityInfo(p ${HANDLE}, i 6, i 7, p 0, p 0, p 0, p 0, *p .R8) i .R7'
  ${If} $R7 != 0
    !insertmacro UIPILOT_LOCAL_FREE $R8
    MessageBox MB_ICONSTOP "Another installer owns an untrusted transaction object. Restart Windows and retry."
    Abort
  ${EndIf}
  System::Call 'advapi32::ConvertSecurityDescriptorToStringSecurityDescriptorW(p R8, i 1, i 7, *p .R9, p 0) i .R7'
  !insertmacro UIPILOT_LOCAL_FREE $R8
  ${If} $R7 = 0
    MessageBox MB_ICONSTOP "The installer transaction security descriptor could not be verified."
    Abort
  ${EndIf}
  System::Call '*$R9(&w${NSIS_MAX_STRLEN} .R7)'
  !insertmacro UIPILOT_LOCAL_FREE $R9
  ${If} $R7 != "${UIPILOT_NAMED_OBJECT_SDDL}"
    MessageBox MB_ICONSTOP "Another installer owns an untrusted transaction object. Restart Windows and retry."
    Abort
  ${EndIf}
!macroend

!macro UIPILOT_SECURITY_ATTRIBUTES SDDL
  StrCpy $R8 0
  StrCpy $R9 0
  System::Call 'advapi32::ConvertStringSecurityDescriptorToSecurityDescriptorW(w "${SDDL}", i 1, *p .R8, p 0) i .R7'
  ${If} $R7 = 0
    MessageBox MB_ICONSTOP "The installer could not create a protected transaction."
    Abort
  ${EndIf}
  System::Alloc 12
  Pop $R9
  ${If} $R9 = 0
    !insertmacro UIPILOT_LOCAL_FREE $R8
    MessageBox MB_ICONSTOP "The installer could not allocate a protected transaction."
    Abort
  ${EndIf}
  System::Call '*$R9(i 12, p $R8, i 0)'
!macroend

!macro UIPILOT_FREE_SECURITY_ATTRIBUTES
  !insertmacro UIPILOT_LOCAL_FREE $R8
  ${If} $R9 != 0
    System::Free $R9
  ${EndIf}
!macroend

!macro UIPILOT_RESOLVE_TOKEN_USER_SID
  StrCpy $UiPilotOwnerSid ""
  StrCpy $R4 ""
  StrCpy $R5 0
  StrCpy $R8 0
  System::Call 'advapi32::OpenProcessToken(p -1, i 8, *p .R5) i .R6'
  ${If} $R6 != 0
    StrCpy $R7 0
    System::Call 'kernel32::SetLastError(i 0)'
    System::Call 'advapi32::GetTokenInformation(p R5, i 1, p 0, i 0, *i .R7) i .R6'
    System::Call 'kernel32::GetLastError() i .R9'
    ${If} $R6 = 0
    ${AndIf} $R9 = 122
    ${AndIf} $R7 > 0
      System::Alloc $R7
      Pop $R8
      ${If} $R8 != 0
        System::Call 'advapi32::GetTokenInformation(p R5, i 1, p R8, i R7, *i .R9) i .R6'
        ${If} $R6 != 0
          System::Call '*$R8(p .R9)'
          ${If} $R9 != 0
            StrCpy $R6 0
            System::Call 'advapi32::ConvertSidToStringSidW(p R9, *p .R6) i .R7'
            ${If} $R7 != 0
            ${AndIf} $R6 != 0
              System::Call 'kernel32::lstrcpynW(w .R4, p R6, i ${NSIS_MAX_STRLEN}) p'
            ${EndIf}
            ${If} $R6 != 0
              System::Call 'kernel32::LocalFree(p R6)'
            ${EndIf}
          ${EndIf}
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ${If} $R8 != 0
    System::Free $R8
  ${EndIf}
  ${If} $R5 != 0
    System::Call 'kernel32::CloseHandle(p R5)'
  ${EndIf}
  StrCpy $UiPilotOwnerSid $R4
  ${If} $UiPilotOwnerSid == ""
    MessageBox MB_ICONSTOP "The installer token owner could not be resolved."
    Abort
  ${EndIf}
!macroend

!macro UIPILOT_SET_STAGING_FILE_OWNER PATH
  StrCpy $R8 0
  System::Call 'advapi32::ConvertStringSidToSidW(w "$UiPilotOwnerSid", *p .R8) i .R7'
  ${If} $R7 = 0
  ${OrIf} $R8 = 0
    !insertmacro UIPILOT_LOCAL_FREE $R8
    MessageBox MB_ICONSTOP "A protected staging file owner could not be prepared."
    Abort
  ${EndIf}
  System::Call 'advapi32::SetNamedSecurityInfoW(w ${PATH}, i 1, i 1, p R8, p 0, p 0, p 0) i .R6'
  !insertmacro UIPILOT_LOCAL_FREE $R8
  ${If} $R6 != 0
    MessageBox MB_ICONSTOP "A protected staging file owner could not be applied."
    Abort
  ${EndIf}
!macroend

!macro UIPILOT_ACQUIRE_LEASE
  !insertmacro UIPILOT_SECURITY_ATTRIBUTES "${UIPILOT_NAMED_OBJECT_SDDL}"
  System::Call 'kernel32::SetLastError(i 0)'
  System::Call 'kernel32::CreateMutexW(p R9, i 0, w "${UIPILOT_MUTEX_NAME}") p .R5'
  System::Call 'kernel32::GetLastError() i .R6'
  StrCpy $UiPilotMutexHandle $R5
  !insertmacro UIPILOT_FREE_SECURITY_ATTRIBUTES
  ${If} $UiPilotMutexHandle = 0
    MessageBox MB_ICONSTOP "The installer transaction lock could not be created."
    Abort
  ${EndIf}
  ${If} $R6 = 183
    !insertmacro UIPILOT_VALIDATE_NAMED_OBJECT $UiPilotMutexHandle
  ${ElseIf} $R6 != 0
    System::Call 'kernel32::CloseHandle(p $UiPilotMutexHandle)'
    StrCpy $UiPilotMutexHandle 0
    MessageBox MB_ICONSTOP "The installer transaction lock could not be verified."
    Abort
  ${EndIf}
  System::Call 'kernel32::WaitForSingleObject(p $UiPilotMutexHandle, i 0) i .R6'
  ${If} $R6 = 258
    MessageBox MB_ICONSTOP "Another UiPilot installer is active. Close it or restart Windows, then retry."
    Abort
  ${ElseIf} $R6 != 0
  ${AndIf} $R6 != 128
    MessageBox MB_ICONSTOP "The installer transaction lock could not be acquired."
    Abort
  ${EndIf}

  System::Call 'kernel32::OpenEventW(i 1179648, i 0, w "${UIPILOT_ACTIVITY_NAME}") p .R5'
  ${If} $R5 != 0
    !insertmacro UIPILOT_VALIDATE_NAMED_OBJECT $R5
    System::Call 'kernel32::CloseHandle(p R5)'
    MessageBox MB_ICONSTOP "Another UiPilot installer or recovery Helper is active."
    Abort
  ${EndIf}
  System::Call 'kernel32::GetLastError() i .R6'
  ${If} $R6 != 2
    MessageBox MB_ICONSTOP "An untrusted installer activity object already exists. Restart Windows and retry."
    Abort
  ${EndIf}
  !insertmacro UIPILOT_SECURITY_ATTRIBUTES "${UIPILOT_NAMED_OBJECT_SDDL}"
  System::Call 'kernel32::SetLastError(i 0)'
  System::Call 'kernel32::CreateEventW(p R9, i 1, i 0, w "${UIPILOT_ACTIVITY_NAME}") p .R5'
  System::Call 'kernel32::GetLastError() i .R6'
  StrCpy $UiPilotActivityHandle $R5
  !insertmacro UIPILOT_FREE_SECURITY_ATTRIBUTES
  ${If} $UiPilotActivityHandle = 0
  ${OrIf} $R6 = 183
    MessageBox MB_ICONSTOP "The protected installer activity lease could not be created."
    Abort
  ${EndIf}
!macroend

!macro UIPILOT_CREATE_STAGING UNINSTALL
  ; SHGetKnownFolderPath(FOLDERID_LocalAppData) - never trust environment variables.
  StrCpy $R4 ""
  StrCpy $R7 0
  System::Call 'shell32::SHGetKnownFolderPath(g "{F1B32785-6FBA-4FCF-9D55-7B8E7F157091}", i 0, p 0, *p .R7) i .R6'
  ${If} $R6 = 0
  ${AndIf} $R7 != 0
    System::Call 'kernel32::lstrcpynW(w .R4, p R7, i ${NSIS_MAX_STRLEN}) p'
  ${EndIf}
  ${If} $R7 != 0
    System::Call 'ole32::CoTaskMemFree(p R7)'
  ${EndIf}
  ${If} $R6 != 0
  ${OrIf} $R7 = 0
  ${OrIf} $R4 == ""
    MessageBox MB_ICONSTOP "The elevated account temporary folder could not be resolved."
    Abort
  ${EndIf}
  System::Call 'ole32::CoCreateGuid(g .R6) i .R5'
  ${If} $R5 != 0
    MessageBox MB_ICONSTOP "A secure installer transaction identifier could not be generated."
    Abort
  ${EndIf}
  StrCpy $UiPilotOperationId $R6 36 1
  !if "${UNINSTALL}" == "yes"
    ${UnStrCase} $UiPilotOperationId $UiPilotOperationId "L"
  !else
    ${StrCase} $UiPilotOperationId $UiPilotOperationId "L"
  !endif
  StrCpy $UiPilotStaging "$R4\Temp\UiPilotManagedEverything-$UiPilotOperationId"
  !insertmacro UIPILOT_RESOLVE_TOKEN_USER_SID
  !insertmacro UIPILOT_SECURITY_ATTRIBUTES "O:$UiPilotOwnerSid${UIPILOT_STAGING_SDDL}"
  System::Call 'kernel32::CreateDirectoryW(w "$UiPilotStaging", p R9) i .R6'
  !insertmacro UIPILOT_FREE_SECURITY_ATTRIBUTES
  ${If} $R6 = 0
    MessageBox MB_ICONSTOP "The protected installer staging directory could not be created."
    Abort
  ${EndIf}
  StrCpy $UiPilotHelper "$UiPilotStaging\uipilot-everything-installer.exe"
  StrCpy $UiPilotRequest "$UiPilotStaging\request.json"
  StrCpy $UiPilotJournal "$UiPilotStaging\journal.json"
  SetOutPath "$UiPilotStaging"
  File "/oname=uipilot-everything-installer.exe" "${__FILEDIR__}\..\..\uipilot-everything-installer.exe"
  !insertmacro UIPILOT_SET_STAGING_FILE_OWNER "$UiPilotHelper"
  FileOpen $R6 "$UiPilotJournal" w
  FileClose $R6
  !insertmacro UIPILOT_SET_STAGING_FILE_OWNER "$UiPilotJournal"
  System::Call 'kernel32::GetCurrentProcessId() i .R5'
  StrCpy $UiPilotParentPid $R5
  System::Call 'kernel32::GetProcessTimes(p -1, *l .R5, *l .R6, *l .R7, *l .R8) i .R9'
  StrCpy $UiPilotParentCreationTime $R5
  ${If} $R9 = 0
    MessageBox MB_ICONSTOP "The installer parent identity could not be captured."
    Abort
  ${EndIf}
!macroend

!macro UIPILOT_WRITE_REQUEST ACTION INSTALL_DIR RECOVERED_ID UNINSTALL
  !if "${UNINSTALL}" == "yes"
    ${UnStrRep} $R5 "$UiPilotStaging" "\" "/"
    ${UnStrRep} $R6 "$UiPilotRequest" "\" "/"
    ${UnStrRep} $R7 "$UiPilotJournal" "\" "/"
  !else
    ${StrRep} $R5 "$UiPilotStaging" "\" "/"
    ${StrRep} $R6 "$UiPilotRequest" "\" "/"
    ${StrRep} $R7 "$UiPilotJournal" "\" "/"
  !endif
  FileOpen $R8 "$UiPilotRequest" w
  FileWrite $R8 '{$\"schema_version$\":1,$\"action$\":$\"${ACTION}$\",$\"operation_id$\":$\"$UiPilotOperationId$\",$\"staging_dir$\":$\"$R5$\",$\"request_path$\":$\"$R6$\",$\"journal_path$\":$\"$R7$\",$\"parent_pid$\":$UiPilotParentPid,$\"parent_creation_time$\":$UiPilotParentCreationTime'
  !if "${INSTALL_DIR}" == "yes"
    !if "${UNINSTALL}" == "yes"
      ${UnStrRep} $R5 "$INSTDIR" "\" "/"
    !else
      ${StrRep} $R5 "$INSTDIR" "\" "/"
    !endif
    FileWrite $R8 ',$\"install_dir$\":$\"$R5$\"'
  !else
    FileWrite $R8 ',$\"install_dir$\":null'
  !endif
  !if "${RECOVERED_ID}" == "yes"
    FileWrite $R8 ',$\"recovered_transaction_id$\":$\"$UiPilotRecoveryId$\"}'
  !else
    FileWrite $R8 ',$\"recovered_transaction_id$\":null}'
  !endif
  FileClose $R8
  !insertmacro UIPILOT_SET_STAGING_FILE_OWNER "$UiPilotRequest"
!macroend

!macro UIPILOT_RUN_HELPER ACTION INSTALL_DIR RECOVERED_ID UNINSTALL
  !insertmacro UIPILOT_WRITE_REQUEST "${ACTION}" "${INSTALL_DIR}" "${RECOVERED_ID}" "${UNINSTALL}"
  StrCpy $R5 '"$UiPilotHelper" ${ACTION} --operation-id "$UiPilotOperationId" --parent-pid "$UiPilotParentPid" --parent-creation-time "$UiPilotParentCreationTime" --staging-dir "$UiPilotStaging" --request "$UiPilotRequest" --journal "$UiPilotJournal"'
  !if "${INSTALL_DIR}" == "yes"
    StrCpy $R5 '$R5 --install-dir "$INSTDIR"'
  !endif
  !if "${RECOVERED_ID}" == "yes"
    StrCpy $R5 '$R5 --recovered-transaction-id "$UiPilotRecoveryId"'
  !endif
  nsExec::ExecToLog $R5
  Pop $UiPilotHelperExit
!macroend

!macro UIPILOT_CLEANUP
  Delete "$UiPilotRequest"
  Delete "$UiPilotJournal"
  Delete "$UiPilotHelper"
  RMDir "$UiPilotStaging"
  Sleep 100
  Delete "$UiPilotRequest"
  Delete "$UiPilotJournal"
  Delete "$UiPilotHelper"
  RMDir "$UiPilotStaging"
  Sleep 250
  Delete "$UiPilotRequest"
  Delete "$UiPilotJournal"
  Delete "$UiPilotHelper"
  RMDir "$UiPilotStaging"
  ${If} $UiPilotActivityHandle != 0
    System::Call 'kernel32::CloseHandle(p $UiPilotActivityHandle)'
    StrCpy $UiPilotActivityHandle 0
  ${EndIf}
  ${If} $UiPilotMutexHandle != 0
    System::Call 'kernel32::ReleaseMutex(p $UiPilotMutexHandle)'
    System::Call 'kernel32::CloseHandle(p $UiPilotMutexHandle)'
    StrCpy $UiPilotMutexHandle 0
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro UIPILOT_ACQUIRE_LEASE
  !insertmacro UIPILOT_CREATE_STAGING "no"
  !insertmacro UIPILOT_RUN_HELPER "prepare-install" "no" "no" "no"
  ${If} $UiPilotHelperExit = 15
    FileOpen $R5 "$UiPilotJournal" r
    FileRead $R5 $UiPilotRecoveryId
    FileClose $R5
    ${If} $UiPilotRecoveryId == "bootstrap"
      !insertmacro UIPILOT_RUN_HELPER "recover-install" "no" "no" "no"
    ${Else}
      !insertmacro UIPILOT_RUN_HELPER "recover-install" "no" "yes" "no"
    ${EndIf}
  ${EndIf}
  ${If} $UiPilotHelperExit != 0
    !insertmacro UIPILOT_CLEANUP
    MessageBox MB_ICONSTOP "UiPilot's private search Service could not be prepared. Restart Windows and retry."
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro UIPILOT_RUN_HELPER "commit-install" "yes" "no" "no"
  ${If} $UiPilotHelperExit != 0
    !insertmacro UIPILOT_RUN_HELPER "rollback-install" "no" "no" "no"
    !insertmacro UIPILOT_CLEANUP
    MessageBox MB_ICONSTOP "UiPilot's private search Service could not be installed. Run this trusted Setup again."
    Abort
  ${EndIf}
  !insertmacro UIPILOT_CLEANUP
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode != 1
    !insertmacro UIPILOT_ACQUIRE_LEASE
    !insertmacro UIPILOT_CREATE_STAGING "yes"
    !insertmacro UIPILOT_RUN_HELPER "pre-uninstall" "no" "no" "yes"
    ${If} $UiPilotHelperExit = 15
      FileOpen $R5 "$UiPilotJournal" r
      FileRead $R5 $UiPilotRecoveryId
      FileClose $R5
      !insertmacro UIPILOT_RUN_HELPER "pre-uninstall" "no" "yes" "yes"
    ${EndIf}
    ${If} $UiPilotHelperExit != 0
      !insertmacro UIPILOT_CLEANUP
      MessageBox MB_ICONSTOP "UiPilot's private search Service could not be removed safely."
      Abort
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode != 1
    !insertmacro UIPILOT_RUN_HELPER "post-uninstall" "no" "no" "yes"
    !insertmacro UIPILOT_CLEANUP
    ${If} $UiPilotHelperExit != 0
      MessageBox MB_ICONSTOP "UiPilot's private search runtime cleanup is incomplete. Run the uninstaller again."
      Abort
    ${EndIf}
  ${EndIf}
!macroend

Function .onInstFailed
  !insertmacro UIPILOT_CLEANUP
FunctionEnd

Function UiPilotManagedEverythingOnAbort
  !insertmacro UIPILOT_CLEANUP
FunctionEnd

Function un.onUninstFailed
  !insertmacro UIPILOT_CLEANUP
FunctionEnd

Function un.UiPilotManagedEverythingOnAbort
  !insertmacro UIPILOT_CLEANUP
FunctionEnd
