Unicode true
!include "MUI2.nsh"
Name "ToolboxCraft"
OutFile "${OUT}\toolboxcraft-${VERSION}-${ARCH}-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\ToolboxCraft"
RequestExecutionLevel user
!ifdef SIGN_SCRIPT
  !uninstfinalize '"${SIGN_PYTHON}" "${SIGN_SCRIPT}" "%1"' = 0
!endif
!define MUI_ICON "${SOURCE}\toolboxcraft.ico"
!define MUI_UNICON "${SOURCE}\toolboxcraft.ico"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
Section
  SetOutPath "$INSTDIR"
  File /r "${SOURCE}\*"
  CreateShortcut "$SMPROGRAMS\ToolboxCraft.lnk" "$INSTDIR\toolboxcraft.exe"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ToolboxCraft" "DisplayName" "ToolboxCraft"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ToolboxCraft" "DisplayIcon" '"$INSTDIR\toolboxcraft.exe",0'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ToolboxCraft" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ToolboxCraft" "UninstallString" '"$INSTDIR\Uninstall.exe"'
SectionEnd
Section "Uninstall"
  Delete "$SMPROGRAMS\ToolboxCraft.lnk"
  Delete "$INSTDIR\toolboxcraft.exe"
  Delete "$INSTDIR\toolboxcraft-updater.exe"
  Delete "$INSTDIR\toolboxcraft.ico"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\ATTRIBUTION.md"
  RMDir /r "$INSTDIR\licenses"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ToolboxCraft"
SectionEnd
