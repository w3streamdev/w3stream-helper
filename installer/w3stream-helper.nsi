; ============================================================================
; w3stream-helper installer
;
; Drops:
;   1. helper.exe                       → %LOCALAPPDATA%\w3stream\helper.exe
;   2. native-messaging manifest JSON   → %LOCALAPPDATA%\w3stream\manifest.json
;   3. HKCU registry key pointing Chrome at that manifest
;
; Per-user install — no UAC prompt, no admin needed. Each Windows user that
; streams installs once; the manifest registers only for that user's Chrome
; profile (HKCU), which is what the Native Messaging spec wants anyway.
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

; Default path of HidHide's CLI after the bundled .exe installs it.
; Nefarius's signed setup uses Program Files unconditionally regardless
; of per-user install for the helper, because the driver itself is
; machine-wide. (Nefarius switched from .msi to a WiX-Burn .exe bundle in
; v1.5.230; the install layout under Program Files is unchanged.)
!define HIDHIDE_CLI "$PROGRAMFILES64\Nefarius Software Solutions\HidHide\x64\HidHideCLI.exe"

VIProductVersion "${FILE_VERSION}"
VIAddVersionKey  "ProductName"     "w3stream Helper"
VIAddVersionKey  "FileDescription" "w3stream Agent native helper"
VIAddVersionKey  "FileVersion"     "${VERSION}"
VIAddVersionKey  "CompanyName"     "Connect3"

Section "Install"
  ; Extract the bundled driver installers into $PLUGINSDIR — a temp
  ; directory NSIS creates per-install and deletes on exit, so streamers
  ; only ever download the single installer .exe. The release workflow
  ; stages vendor/HidHide.exe + vendor/ViGEmBus.exe before invoking
  ; makensis (release.yml::Fetch + verify driver installers).
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File "vendor\ViGEmBus.exe"
  File "vendor\HidHide.exe"

  SetOutPath "$INSTDIR"

  ; Helper binary. Built by CI before invoking makensis.
  File "..\target\x86_64-pc-windows-msvc\release\w3stream-helper.exe"
  Rename "$INSTDIR\w3stream-helper.exe" "$INSTDIR\helper.exe"

  ; --- Driver install (single UAC prompt) ---
  ;
  ; Both Nefarius bundles ship with an `asInvoker` manifest, so a plain
  ; ExecWait from this user-level NSIS can't drive their internal MSIs
  ; with admin rights -- the MSI returns 1925 / fatal 1603. The only
  ; userland fix is to launch them via ShellExecuteEx with the "runas"
  ; verb, which triggers a UAC consent dialog. To keep that down to ONE
  ; prompt for the streamer we shell out to a tiny .bat that chains:
  ;   1. ViGEmBus.exe /passive /norestart     (kernel + bus PnP device)
  ;   2. HidHide.exe  /passive /norestart     (kernel + control device)
  ;   3. HidHideCLI --app-reg <helper.exe>    (whitelist so the helper
  ;      itself can still see the physical pad once the cloak is on)
  ;   4. HidHideCLI --cloak-on                (machine-wide HID cloak)
  ;
  ; If the user declines UAC the bat never runs; the helper still works
  ; for keystroke-only actions and `health.gamepad.available` will be
  ; false so the extension can hint at the missing setup step.
  ;
  ; Note: NSIS's ExecShellWait does not return the child's exit code, so
  ; we infer success post-hoc from the presence of the HidHide CLI. The
  ; helper's own VirtualPad probe at startup is the source of truth for
  ; whether the install actually took.
  IfFileExists "$PLUGINSDIR\ViGEmBus.exe" 0 driver_skip
  IfFileExists "$PLUGINSDIR\HidHide.exe"  0 driver_skip

  FileOpen $4 "$PLUGINSDIR\drivers-bootstrap.bat" w
  FileWrite $4 '@echo off$\r$\n'
  FileWrite $4 'echo Installing ViGEmBus (Nefarius virtual gamepad driver)...$\r$\n'
  FileWrite $4 '"%~dp0ViGEmBus.exe" /passive /norestart$\r$\n'
  FileWrite $4 'echo Installing HidHide...$\r$\n'
  FileWrite $4 '"%~dp0HidHide.exe" /passive /norestart$\r$\n'
  FileWrite $4 'set CLI=%ProgramFiles%\Nefarius Software Solutions\HidHide\x64\HidHideCLI.exe$\r$\n'
  FileWrite $4 'if exist "%CLI%" ($\r$\n'
  FileWrite $4 '  "%CLI%" --app-reg "%~1"$\r$\n'
  FileWrite $4 '  "%CLI%" --cloak-on$\r$\n'
  FileWrite $4 ')$\r$\n'
  FileWrite $4 'exit /b 0$\r$\n'
  FileClose $4

  DetailPrint "Installing ViGEmBus + HidHide drivers (single UAC consent expected)..."
  ExecShellWait "runas" "$SYSDIR\cmd.exe" '/c "$PLUGINSDIR\drivers-bootstrap.bat" "$INSTDIR\helper.exe"' SW_SHOWNORMAL

  ; Driver installers (Burn .exe bundles) set the system reboot-pending
  ; flag internally when needed; we propagate that as a recommendation
  ; here because the freshly-installed virtual bus PnP device sometimes
  ; doesn't enumerate cleanly until a reboot.
  SetRebootFlag true

  ; Ledger of HidHide entries we added, so the uninstaller can revert
  ; cleanly without touching anything the user (or reWASD/DS4Windows)
  ; might have added. Only the helper itself goes on the whitelist --
  ; HidHide has no blocklist: --app-reg lists apps that should still
  ; see *hidden* devices, the cloak does the actual hiding by device
  ; instance path, and the device-hide list is left for the streamer
  ; to populate via the HidHide tray UI (or a future helper feature).
  IfFileExists "${HIDHIDE_CLI}" 0 hidhide_skip_ledger
    FileOpen $4 "$INSTDIR\hidhide-managed.json" w
    FileWrite $4 '{$\r$\n'
    FileWrite $4 '  "apps_added": [$\r$\n'
    StrCpy $5 "$INSTDIR\helper.exe"
    Push $5
    Call EscapeJSONBackslashes
    Pop $6
    FileWrite $4 '    "$6"$\r$\n'
    FileWrite $4 '  ],$\r$\n'
    FileWrite $4 '  "cloak_was_on": true$\r$\n'
    FileWrite $4 '}$\r$\n'
    FileClose $4
  hidhide_skip_ledger:

  Goto drivers_done
  driver_skip:
    DetailPrint "Driver bundles not present in installer staging; skipping driver install."
  drivers_done:

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

  ; Revert only the HidHide entries we added. We don't touch the cloak
  ; toggle because another app (reWASD/DS4Windows) may rely on it.
  ; We don't uninstall the HidHide bundle either -- that's a manual
  ; decision for the streamer. The CLI itself needs admin to mutate
  ; the config, but the uninstaller runs at user-level and we can't
  ; fire a UAC prompt mid-uninstall without the UAC plugin; leaving
  ; the helper on the whitelist is harmless if HidHide is also gone.
  IfFileExists "${HIDHIDE_CLI}" 0 hidhide_revert_skip
    ExecWait '"${HIDHIDE_CLI}" --app-unreg "$INSTDIR\helper.exe"'
  hidhide_revert_skip:
  Delete "$INSTDIR\hidhide-managed.json"

  ; ViGEmBus is NOT uninstalled here on purpose — other apps depend on it.

  Delete "$INSTDIR\helper.exe"
  Delete "$INSTDIR\manifest.json"
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
