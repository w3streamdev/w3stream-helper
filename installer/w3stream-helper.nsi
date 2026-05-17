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

VIProductVersion "${VERSION}.0"
VIAddVersionKey  "ProductName"     "w3stream Helper"
VIAddVersionKey  "FileDescription" "w3stream Agent native helper"
VIAddVersionKey  "FileVersion"     "${VERSION}"
VIAddVersionKey  "CompanyName"     "Connect3"

Section "Install"
  SetOutPath "$INSTDIR"

  ; Helper binary. Built by CI before invoking makensis.
  File "..\target\x86_64-pc-windows-msvc\release\w3stream-helper.exe"
  Rename "$INSTDIR\w3stream-helper.exe" "$INSTDIR\helper.exe"

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
