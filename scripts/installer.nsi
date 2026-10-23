Unicode true
Name "Pulse Helper"
VIProductVersion "0.1.2.0"
VIAddVersionKey "ProductName" "Pulse Helper"
VIAddVersionKey "ProductVersion" "0.1.2"
VIAddVersionKey "FileVersion" "0.1.2"
VIAddVersionKey "FileDescription" "Pulse native screen sharing helper"
OutFile "../release/Pulse-Helper-Windows.exe"
InstallDir "$LOCALAPPDATA\Programs\Pulse Helper"
RequestExecutionLevel user
SetCompressor /SOLID lzma
Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles
Section
SetOutPath "$INSTDIR"
File /r "../release/windows/*"
WriteRegStr HKCU "Software\Classes\pulse-capture" "" "URL:Pulse Capture"
WriteRegStr HKCU "Software\Classes\pulse-capture" "URL Protocol" ""
WriteRegStr HKCU "Software\Classes\pulse-capture\shell\open\command" "" '$\"$INSTDIR\pulse-media.exe$\" $\"%1$\"'
WriteUninstaller "$INSTDIR\Uninstall.exe"
WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PulseHelper" "DisplayName" "Pulse Helper"
WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PulseHelper" "DisplayVersion" "0.1.2"
WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PulseHelper" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
SectionEnd
Section "Uninstall"
DeleteRegKey HKCU "Software\Classes\pulse-capture"
DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PulseHelper"
RMDir /r "$INSTDIR"
SectionEnd
