; Extra installer steps for aLogs (Tauri NSIS hooks).
; Controller blocking uses HidHide's signed driver (Nefarius, MIT). The installer doesn't touch it: the app installs
; just the driver when you set up controller blocking (Results > Controller), and the uninstaller removes it again.
; Nothing else to install: the meter and Sanctum Helper are plain programs (no .NET or Python runtime).

!macro NSIS_HOOK_PREINSTALL
  ; Updating over a running copy: close the app and its meter first (quietly), so files can be replaced and
  ; the installer never has to ask. Settings and logs live in %LOCALAPPDATA%\aLogs and are kept.
  ; (alogs-meter.exe is the meter; meter.exe the one builds up to 0.1.37 B16 carried)
  nsExec::Exec 'taskkill /IM alogs-app.exe /T /F'
  nsExec::Exec 'taskkill /IM alogs-meter.exe /T /F'
  nsExec::Exec 'taskkill /IM meter.exe /T /F'
  nsExec::Exec 'taskkill /IM SanctumHelper.exe /T /F'
  Sleep 800
  ; The bundled programs are replaced as a whole, so files an older version shipped never mix with the new ones.
  ; Updating from 0.1.37 B16 or older, this is what removes the Python meter (meter.exe and its _internal folder) and
  ; the .NET Sanctum Helper's DLLs (an old self-contained one left behind once made Windows ask for .NET over and
  ; over). Only the program folders under resources\ are cleared; your settings, logs and records are elsewhere
  ; and are kept.
  RMDir /r "$INSTDIR\resources\meter"
  RMDir /r "$INSTDIR\resources\sanctumhelper"
  RMDir /r "$INSTDIR\resources\vendor"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; stop a running meter and Sanctum Helper so their files can be removed (the app stops it too when it quits)
  nsExec::Exec 'taskkill /IM alogs-meter.exe /T /F'
  nsExec::Exec 'taskkill /IM meter.exe /T /F'
  nsExec::Exec 'taskkill /IM SanctumHelper.exe /T /F'
  ; remove the start-with-Windows entry if the user had turned it on
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "aLogs"
  ; the HidHide driver, if aLogs installed it (not on an update; Windows asks for admin approval)
  ${If} $UpdateMode <> 1
    ReadEnvStr $0 PROGRAMDATA
    IfFileExists "$0\aLogs\hidhide-driver" 0 hidhide_kept
      ExecShellWait "runas" "$INSTDIR\alogs-app.exe" "--hidhide remove" SW_HIDE
    hidhide_kept:
  ${EndIf}
!macroend
