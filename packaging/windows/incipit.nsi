; Incipit's installer, made by tools/package-windows with NSIS (makensis), which runs in the folder
; this file is copied to, beside `app\` (the files to install) and the icon. It is given:
;   -DVERSION=1.2.3   -DVERSION_NUMBER=1.2.3.0 (four numbers)   -DIDENTIFIER=...
;   -DOUT=..\Incipit-1.2.3-x64-setup.exe
;
; It installs for the person who runs it, into %LOCALAPPDATA%\Programs\Incipit, so it asks for no
; administrator's password. It puts a Start menu shortcut, an entry in Settings > Apps, and Incipit
; in the "Open with" list of PDF and EPUB files, and takes nothing from the programs already there.
; Uninstalling removes what it installed and never touches a vault or the app's own folder
; (%APPDATA%\Incipit, which remembers the vaults).

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
ManifestDPIAware true

!include "MUI2.nsh"
!include "WinVer.nsh"

!define PROGRAM "incipit.exe"
!define PROGID "Incipit.Book"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${IDENTIFIER}"

Name "Incipit"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\Incipit"
InstallDirRegKey HKCU "Software\${IDENTIFIER}" "InstallDir"
BrandingText "Incipit ${VERSION}"
VIProductVersion "${VERSION_NUMBER}"
VIAddVersionKey "ProductName" "Incipit"
VIAddVersionKey "FileDescription" "Incipit installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Licensed under the GNU Affero General Public License, version 3."

!define MUI_ICON "incipit.ico"
!define MUI_UNICON "incipit.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${PROGRAM}"
!define MUI_FINISHPAGE_RUN_TEXT "Open Incipit"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_OK|MB_ICONSTOP "Incipit needs Windows 10 or later."
    Abort
  ${EndIf}
FunctionEnd

Section "Incipit"
  SetOutPath "$INSTDIR"
  File /r "app\*.*"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; The Start menu.
  CreateShortcut "$SMPROGRAMS\Incipit.lnk" "$INSTDIR\${PROGRAM}"

  ; Settings > Apps.
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "Incipit"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${PROGRAM}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  WriteRegStr HKCU "Software\${IDENTIFIER}" "InstallDir" "$INSTDIR"

  ; "Open with" for PDF and EPUB files: offered, never made the default.
  WriteRegStr HKCU "Software\Classes\${PROGID}" "" "Book"
  WriteRegStr HKCU "Software\Classes\${PROGID}\DefaultIcon" "" "$INSTDIR\${PROGRAM},0"
  WriteRegStr HKCU "Software\Classes\${PROGID}\shell\open\command" "" '"$INSTDIR\${PROGRAM}" "%1"'
  WriteRegStr HKCU "Software\Classes\.pdf\OpenWithProgids" "${PROGID}" ""
  WriteRegStr HKCU "Software\Classes\.epub\OpenWithProgids" "${PROGID}" ""
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\Incipit.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "Software\${IDENTIFIER}"
  DeleteRegKey HKCU "Software\Classes\${PROGID}"
  DeleteRegValue HKCU "Software\Classes\.pdf\OpenWithProgids" "${PROGID}"
  DeleteRegValue HKCU "Software\Classes\.epub\OpenWithProgids" "${PROGID}"
  ; The files that were installed, then the folder if nothing else was put in it.
  RMDir /r "$INSTDIR\licenses"
  Delete "$INSTDIR\${PROGRAM}"
  Delete "$INSTDIR\pdfium.dll"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
