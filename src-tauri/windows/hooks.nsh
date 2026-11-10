; The official autostart plugin uses this per-user Run value.
; Uninstall removes the registration but preserves user preferences/credentials.
!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Mi Home Desktop"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Mi Home Desktop"
  ClearErrors
!macroend
