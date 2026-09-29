; Removes the browser registration BrowserPicker writes on first launch
; (see src-tauri/src/platform/windows.rs) and its launch-at-login entry.
!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey HKCU "Software\Clients\StartMenuInternet\BrowserPicker"
  DeleteRegKey HKCU "Software\Classes\BrowserPickerURL"
  DeleteRegKey HKCU "Software\Classes\BrowserPickerHTML"
  DeleteRegValue HKCU "Software\RegisteredApplications" "BrowserPicker"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "BrowserPicker"
!macroend
