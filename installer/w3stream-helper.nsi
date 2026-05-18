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

; Default path of HidHide's CLI after the MSI installs it. The MSI ships
; signed by Nefarius and uses Program Files unconditionally regardless
; of per-user install for the helper, because the driver itself is
; machine-wide.
!define HIDHIDE_CLI "$PROGRAMFILES64\Nefarius Software Solutions\HidHide\x64\HidHideCLI.exe"

; Fortnite's main process — added to HidHide's blocked-apps list so the
; physical pad is invisible while Fortnite runs. Keep in sync with
; src/hidhide.rs::FORTNITE_EXE; if Epic renames this binary in a future
; patch both constants need to change together.
!define FORTNITE_EXE "FortniteClient-Win64-Shipping.exe"

VIProductVersion "${FILE_VERSION}"
VIAddVersionKey  "ProductName"     "w3stream Helper"
VIAddVersionKey  "FileDescription" "w3stream Agent native helper"
VIAddVersionKey  "FileVersion"     "${VERSION}"
VIAddVersionKey  "CompanyName"     "Connect3"

Section "Install"
  SetOutPath "$INSTDIR"

  ; Helper binary. Built by CI before invoking makensis.
  File "..\target\x86_64-pc-windows-msvc\release\w3stream-helper.exe"
  Rename "$INSTDIR\w3stream-helper.exe" "$INSTDIR\helper.exe"

  ; --- ViGEmBus driver ---
  ; Bundled .exe is staged into installer\vendor\ViGEmBus.exe by CI
  ; (release.yml fetches the signed Nefarius release and verifies the
  ; pinned SHA-256). Silent install via /quiet /norestart per
  ; Nefarius docs. We do NOT uninstall ViGEmBus on helper uninstall
  ; because reWASD, DS4Windows, and others depend on it.
  IfFileExists "$EXEDIR\vendor\ViGEmBus.exe" vigem_present vigem_skip
vigem_present:
    DetailPrint "Installing ViGEmBus driver (silent, may take 30s)..."
    ; Nefarius's signed setup uses standard Inno/WiX exit codes:
    ;   0    success
    ;   1602 user cancel (shouldn't happen with /quiet)
    ;   1638 already installed at the same or newer version (success)
    ;   3010 install ok but reboot required
    ExecWait '"$EXEDIR\vendor\ViGEmBus.exe" /quiet /norestart' $0
    StrCmp $0 "0"    vigem_done
    StrCmp $0 "1638" vigem_done
    StrCmp $0 "3010" vigem_reboot
    DetailPrint "ViGEmBus install returned $0; continuing without virtual pad support"
    Goto vigem_skip
  vigem_reboot:
    DetailPrint "ViGEmBus installed; reboot required for the driver to load"
    SetRebootFlag true
  vigem_done:
  vigem_skip:

  ; --- HidHide driver ---
  ; Bundled .msi is staged into installer\vendor\HidHide.msi by CI
  ; (release.yml fetches the signed Nefarius release and verifies the
  ; pinned SHA-256). If the file is missing we keep installing — the
  ; helper degrades gracefully without HidHide, only the suppress-the-
  ; physical-pad behaviour is lost.
  IfFileExists "$EXEDIR\vendor\HidHide.msi" hidhide_present hidhide_skip
hidhide_present:
    DetailPrint "Installing HidHide driver (silent, may take 30s)..."
    ; msiexec returns 0 (installed), 1638 (already same version), 1641/3010
    ; (reboot required). Treat all of these as success.
    ExecWait '"$SYSDIR\msiexec.exe" /i "$EXEDIR\vendor\HidHide.msi" /qn /norestart' $0
    StrCmp $0 "0"    hidhide_configure
    StrCmp $0 "1638" hidhide_configure
    StrCmp $0 "1641" hidhide_reboot
    StrCmp $0 "3010" hidhide_reboot
    DetailPrint "HidHide install returned $0; continuing without HidHide config"
    Goto hidhide_skip
  hidhide_reboot:
    DetailPrint "HidHide installed; a reboot will be required for the driver to load"
    SetRebootFlag true
    ; fall through and try to configure anyway — CLI exits cleanly even
    ; before the driver loads.
  hidhide_configure:
    ; Whitelist the helper so HidHide doesn't also hide the pad from US.
    ; Without this the forwarder can't read XInputGetState either.
    ExecWait '"${HIDHIDE_CLI}" --app-reg "$INSTDIR\helper.exe"' $1
    DetailPrint "HidHide app-reg helper returned $1"
    ; Block Fortnite so it stops seeing the physical pad.
    ExecWait '"${HIDHIDE_CLI}" --app-reg "${FORTNITE_EXE}"' $2
    DetailPrint "HidHide app-reg fortnite returned $2"
    ; Turn the HidHide cloak on (it's a per-machine toggle).
    ExecWait '"${HIDHIDE_CLI}" --cloak-on' $3
    DetailPrint "HidHide cloak-on returned $3"
    ; Record what we changed so the uninstaller can revert cleanly
    ; without touching entries another app/user added.
    FileOpen $4 "$INSTDIR\hidhide-managed.json" w
    FileWrite $4 '{$\r$\n'
    FileWrite $4 '  "apps_added": [$\r$\n'
    StrCpy $5 "$INSTDIR\helper.exe"
    Push $5
    Call EscapeJSONBackslashes
    Pop $6
    FileWrite $4 '    "$6",$\r$\n'
    FileWrite $4 '    "${FORTNITE_EXE}"$\r$\n'
    FileWrite $4 '  ],$\r$\n'
    FileWrite $4 '  "cloak_was_on": true$\r$\n'
    FileWrite $4 '}$\r$\n'
    FileClose $4
  hidhide_skip:

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
  ; We don't uninstall the HidHide MSI either — that's a manual user
  ; decision.
  IfFileExists "${HIDHIDE_CLI}" 0 hidhide_revert_skip
    ExecWait '"${HIDHIDE_CLI}" --app-unreg "$INSTDIR\helper.exe"'
    ExecWait '"${HIDHIDE_CLI}" --app-unreg "${FORTNITE_EXE}"'
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
