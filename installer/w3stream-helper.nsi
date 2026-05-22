; ============================================================================
; w3stream-helper installer
;
; Drops:
;   1. helper.exe                      → %LOCALAPPDATA%\w3stream\helper.exe
;   2. disable-gamepad.ps1             → %LOCALAPPDATA%\w3stream\disable-gamepad.ps1
;   3. native-messaging manifest JSON  → %LOCALAPPDATA%\w3stream\manifest.json
;   4. HKCU registry key pointing Chrome/Edge at that manifest
;   5. "w3stream-input-guard" Scheduled Task — runs disable-gamepad.ps1 as
;      SYSTEM, on demand, registered via ONE elevated step (single UAC).
;
; The helper itself is per-user (HKCU, no admin). The ONLY privileged action
; is registering the input-guard task: disabling a physical game controller
; needs SYSTEM rights and a user-level process cannot do it. After that single
; UAC prompt every emote is silent — the user-level helper just triggers the
; already-registered task with `schtasks /run`.
;
; CLI usage (NSIS):
;   makensis -DVERSION=0.1.0 -DEXTENSION_ID=<id> w3stream-helper.nsi
; ============================================================================

!ifndef VERSION
  !define VERSION "0.0.0"
!endif

!ifndef FILE_VERSION
  ; VIProductVersion requires strictly X.X.X.X.  If CI didn't compute it,
  ; default to the version with .0 appended (only valid if VERSION itself
  ; is purely numeric — release tags only).
  !define FILE_VERSION "${VERSION}.0"
!endif

!ifndef EXTENSION_ID
  !error "EXTENSION_ID must be defined: -DEXTENSION_ID=<chrome-extension-id>"
!endif

!include "MUI2.nsh"
!include "FileFunc.nsh"

Name        "w3stream Helper"
OutFile     "w3stream-helper-setup-${VERSION}.exe"
Unicode     true
RequestExecutionLevel user
InstallDir  "$LOCALAPPDATA\w3stream"
BrandingText "w3stream"

!define MUI_ABORTWARNING
!define MUI_ICON   "${NSISDIR}\Contrib\Graphics\Icons\modern-install.ico"
!define MUI_UNICON "${NSISDIR}\Contrib\Graphics\Icons\modern-uninstall.ico"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; Full path to Windows PowerShell — used for the one elevated install step.
!define POWERSHELL "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"

VIProductVersion "${FILE_VERSION}"
VIAddVersionKey  "ProductName"     "w3stream Helper"
VIAddVersionKey  "FileDescription" "w3stream Agent native helper"
VIAddVersionKey  "FileVersion"     "${VERSION}"
VIAddVersionKey  "CompanyName"     "Connect3"

Section "Install"
  SetOutPath "$INSTDIR"

  ; Stop any running helper so its .exe isn't locked. Chrome relaunches the
  ; helper automatically on the next native-messaging connect.
  nsExec::Exec 'taskkill /F /IM helper.exe'
  Pop $0

  ; Helper binary. Built by CI before invoking makensis. Drop it straight in
  ; as helper.exe via /oname — this OVERWRITES an existing helper.exe.
  ;
  ; The previous File + Rename pattern silently failed on every reinstall:
  ; NSIS Rename will not replace an existing destination, so the stale
  ; helper.exe stayed put and the freshly-extracted binary sat unused next
  ; to it as w3stream-helper.exe. /oname makes the extract authoritative.
  File "/oname=helper.exe" "..\target\x86_64-pc-windows-msvc\release\w3stream-helper.exe"
  ; Remove the misnamed leftover from any prior broken install.
  Delete "$INSTDIR\w3stream-helper.exe"

  ; The gamepad-lockout worker. The input-guard Scheduled Task runs this as
  ; SYSTEM whenever an emote fires: it disables the game controller(s) for a
  ; few seconds, then re-enables them.
  File "disable-gamepad.ps1"

  ; --- Register the input-guard Scheduled Task (single UAC prompt) ---
  ;
  ; Disabling a physical game controller for the emote window needs SYSTEM
  ; rights. We register a Scheduled Task that runs disable-gamepad.ps1 as
  ; SYSTEM; a standard user (the helper) can TRIGGER it with `schtasks /run`
  ; without elevation, so this is the ONLY UAC prompt the streamer ever sees.
  ;
  ; register-guard.ps1 is an install-time-only helper, extracted to the temp
  ; $PLUGINSDIR. It calls Register-ScheduledTask and logs to install-guard.log.
  ; If the user declines UAC the task isn't created — the helper still fires
  ; emote keystrokes and still locks keyboard + mouse; only the gamepad part
  ; of the lockout is missing, and `health.input_guard.available` reports it.
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File "register-guard.ps1"
  SetOutPath "$INSTDIR"

  DetailPrint "Registering the w3stream input-guard task (single UAC consent expected)..."
  ExecShellWait "runas" "${POWERSHELL}" '-NoProfile -ExecutionPolicy Bypass -File "$PLUGINSDIR\register-guard.ps1" -ScriptPath "$INSTDIR\disable-gamepad.ps1" -LogPath "$INSTDIR\install-guard.log"' SW_HIDE

  ; Render the native-messaging manifest with absolute helper path.
  ; NSIS-double-backslash because the manifest is JSON.
  FileOpen $0 "$INSTDIR\manifest.json" w
  FileWrite $0 '{$\r$\n'
  FileWrite $0 '  "name": "io.connect3.w3stream.helper",$\r$\n'
  FileWrite $0 '  "description": "w3stream Agent native helper",$\r$\n'
  StrCpy $1 "$INSTDIR\helper.exe"
  ; JSON requires forward-slash or escaped-backslash paths
  Push $1
  Call EscapeJSONBackslashes
  Pop $2
  FileWrite $0 '  "path": "$2",$\r$\n'
  FileWrite $0 '  "type": "stdio",$\r$\n'
  FileWrite $0 '  "allowed_origins": ["chrome-extension://${EXTENSION_ID}/"]$\r$\n'
  FileWrite $0 '}$\r$\n'
  FileClose $0

  ; Register the manifest with Chrome (per-user).
  WriteRegStr HKCU "Software\Google\Chrome\NativeMessagingHosts\io.connect3.w3stream.helper" "" "$INSTDIR\manifest.json"
  ; Also register with Edge (Chromium) so the extension works there too.
  WriteRegStr HKCU "Software\Microsoft\Edge\NativeMessagingHosts\io.connect3.w3stream.helper" "" "$INSTDIR\manifest.json"

  ; Uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Add/Remove Programs entry
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\w3stream-helper" "DisplayName"     "w3stream Helper"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\w3stream-helper" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\w3stream-helper" "Publisher"      "Connect3"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\w3stream-helper" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\w3stream-helper" "InstallLocation" '"$INSTDIR"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\w3stream-helper" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\w3stream-helper" "NoRepair" 1
SectionEnd

Section "Uninstall"
  DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\io.connect3.w3stream.helper"
  DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\io.connect3.w3stream.helper"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\w3stream-helper"

  ; Remove the input-guard Scheduled Task. Deleting a task needs admin, so
  ; this is one elevated step. If the user declines, the leftover task is
  ; harmless once disable-gamepad.ps1 (deleted below) is gone — it just fails
  ; fast with nothing to run.
  ExecShellWait "runas" "$SYSDIR\schtasks.exe" '/delete /tn "w3stream-input-guard" /f' SW_HIDE

  Delete "$INSTDIR\helper.exe"
  Delete "$INSTDIR\disable-gamepad.ps1"
  Delete "$INSTDIR\manifest.json"
  Delete "$INSTDIR\install-guard.log"
  Delete "$INSTDIR\uninstall.exe"
  ; Leave actions.json + helper.log on disk so the streamer keeps their config
  ; if they reinstall. To do a hard clean: also delete actions.json + logs.
  RMDir "$INSTDIR"
SectionEnd

; Escape "\" -> "\\" in $0 (path), push result to stack.
Function EscapeJSONBackslashes
  Exch $0
  Push $1
  Push $2
  Push $3
  StrCpy $1 ""        ; output accumulator
  StrCpy $2 0         ; index
  loop:
    StrCpy $3 $0 1 $2
    StrCmp $3 "" done
    StrCmp $3 "\" 0 +3
      StrCpy $1 "$1\\"
      Goto inc
    StrCpy $1 "$1$3"
    inc:
    IntOp $2 $2 + 1
    Goto loop
  done:
  StrCpy $0 $1
  Pop $3
  Pop $2
  Pop $1
  Exch $0
FunctionEnd
