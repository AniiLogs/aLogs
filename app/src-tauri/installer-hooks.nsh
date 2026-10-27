; Extra installer steps for aLogs (Tauri NSIS hooks).
; Controller blocking uses HidHide's signed driver (Nefarius, MIT). The installer doesn't touch it: the app installs
; just the driver when you set up controller blocking (Results > Controller), and the uninstaller removes it again.

!macro NSIS_HOOK_PREINSTALL
  ; Updating over a running copy: close the app and its meter first (quietly), so files can be replaced and
  ; the installer never has to ask. Settings and logs live in %LOCALAPPDATA%\aLogs and are kept.
  nsExec::Exec 'taskkill /IM alogs-app.exe /T /F'
  nsExec::Exec 'taskkill /IM meter.exe /T /F'
  nsExec::Exec 'taskkill /IM SanctumHelper.exe /T /F'
  Sleep 800
  ; The bundled programs are replaced as a whole, so files an older version shipped never mix with the new ones
  ; (an old self-contained Sanctum Helper left behind made Windows ask for .NET over and over). Only the
  ; program folders under resources\ are cleared; your settings, logs and records are elsewhere and are kept.
  RMDir /r "$INSTDIR\resources\meter"
  RMDir /r "$INSTDIR\resources\sanctumhelper"
  RMDir /r "$INSTDIR\resources\vendor"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Sanctum Helper runs on Microsoft's .NET Desktop Runtime (9 or newer) instead of carrying its own ~150 MB copy
  IfSilent dotnet_done
  ; any installed .NET Desktop Runtime 9 or newer will do (Sanctum Helper runs on newer ones too)
  FindFirst $0 $1 "$PROGRAMFILES64\dotnet\shared\Microsoft.WindowsDesktop.App\*"
  dotnet_next:
    StrCmp $1 "" dotnet_none
    IntOp $2 $1 + 0                 ; the major version: "10.0.5" -> 10 ("." and ".." -> 0)
    IntCmp $2 9 dotnet_found 0 dotnet_found
    FindNext $0 $1
    Goto dotnet_next
  dotnet_none:
  FindClose $0
  MessageBox MB_YESNO|MB_ICONQUESTION "Sanctum Helper needs Microsoft's .NET Desktop Runtime 9 (about 55 MB download).$\r$\n$\r$\nDownload and install it now? (You can also do it later from the Sanctum Helper page.)" IDNO dotnet_done
  DetailPrint "Downloading the .NET Desktop Runtime from Microsoft..."
  nsExec::ExecToLog 'powershell -NoProfile -ExecutionPolicy Bypass -Command "Invoke-WebRequest -UseBasicParsing https://aka.ms/dotnet/9.0/windowsdesktop-runtime-win-x64.exe -OutFile $\"$TEMP\windowsdesktop-runtime-9-win-x64.exe$\""'
  IfFileExists "$TEMP\windowsdesktop-runtime-9-win-x64.exe" 0 dotnet_done
  DetailPrint "Installing the .NET Desktop Runtime..."
  ExecShellWait "runas" "$TEMP\windowsdesktop-runtime-9-win-x64.exe" "/install /quiet /norestart"
  Delete "$TEMP\windowsdesktop-runtime-9-win-x64.exe"
  Goto dotnet_done
  dotnet_found:
    FindClose $0
  dotnet_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; stop a running meter and Sanctum Helper so their files can be removed (the app stops it too when it quits)
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
