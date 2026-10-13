Unicode true
!include "MUI2.nsh"
Name "ToolboxCraft"
OutFile "${OUT}\toolboxcraft-${VERSION}-${ARCH}-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\ToolboxCraft"
RequestExecutionLevel user
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
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ToolboxCraft" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ToolboxCraft" "UninstallString" '"$INSTDIR\Uninstall.exe"'
SectionEnd
Section "Uninstall"
  Delete "$SMPROGRAMS\ToolboxCraft.lnk"
  Delete "$INSTDIR\toolboxcraft.exe"
  Delete "$INSTDIR\toolboxcraft-updater.exe"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\ATTRIBUTION.md"
  RMDir /r "$INSTDIR\licenses"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ToolboxCraft"
SectionEnd
