; Vsesvit's per-user installer. `cargo xtask package nsis` stages the files and passes
; PRODUCTNAME, BINARY, PUBLISHER, HOMEPAGE, DESCRIPTION, VERSION, VIVERSION, STAGE and OUTFILE.
;
; The command line follows the Tauri NSIS installer, because the updater launches it the way
; tauri-plugin-updater does (docs/design/packaging.md):
;   /S          silent            /P        passive: progress only, no questions
;   /UPDATE     keep shortcuts and settings as they are, close the running app without asking
;   /R          relaunch the app after a silent or passive install
;   /ARGS ...   the rest of the command line is passed to the relaunched app
;   /NS         no shortcuts      /D=DIR    install directory (must be last, unquoted)

Unicode true
ManifestDPIAware true
ManifestDPIAwareness PerMonitorV2
RequestExecutionLevel user
SetCompressor /SOLID lzma

!include MUI2.nsh
!include FileFunc.nsh
!include LogicLib.nsh
!include x64.nsh
!include "Win\RestartManager.nsh"
!include "runtime.nsh"

!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
!define CLIENTKEY "Software\Clients\StartMenuInternet\${PRODUCTNAME}"
!define PROGID "${PRODUCTNAME}HTML"
!define EXE "$INSTDIR\${BINARY}.exe"
!define ICON "$INSTDIR\${BINARY}.ico"
!define /ifndef SHCNE_ASSOCCHANGED 0x08000000

Name "${PRODUCTNAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCTNAME}"
InstallDirRegKey HKCU "${UNINSTKEY}" "InstallLocation"
BrandingText "${PRODUCTNAME} ${VERSION}"

Var PassiveMode
Var UpdateMode
Var NoShortcutMode
Var DeleteDataCheckbox
Var DeleteData

!define MUI_ICON "${STAGE}\${BINARY}.ico"
!define MUI_UNICON "${STAGE}\${BINARY}.ico"
!define MUI_ABORTWARNING

!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_WELCOME
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "${EXE}"
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Create a desktop shortcut"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION CreateDesktopShortcut
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!insertmacro MUI_PAGE_FINISH

!define MUI_PAGE_CUSTOMFUNCTION_PRE un.SkipIfPassive
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.ConfirmShow
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE un.ConfirmLeave
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

VIProductVersion "${VIVERSION}"
VIFileVersion "${VIVERSION}"
VIAddVersionKey "ProductName" "${PRODUCTNAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${PRODUCTNAME} Setup"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "Copyright ${PUBLISHER}"

; Every file the package installs, so install and uninstall cannot drift apart.
!macro ForEachPayloadFile MACRO
  !insertmacro ${MACRO} "${BINARY}.exe"
  !insertmacro ${MACRO} "Microsoft.Web.WebView2.Core.dll"
  !insertmacro ${MACRO} "${BINARY}.ico"
  !insertmacro ${MACRO} "package-format"
!macroend
!macro InstallPayloadFile NAME
  File "${STAGE}\${NAME}"
!macroend
!macro DeletePayloadFile NAME
  Delete "$INSTDIR\${NAME}"
!macroend

; File types and URL schemes offered in Default Apps. Registering never makes Vsesvit the default.
!macro ForEachAssociation MACRO
  !insertmacro ${MACRO} FileAssociations ".htm"
  !insertmacro ${MACRO} FileAssociations ".html"
  !insertmacro ${MACRO} FileAssociations ".shtml"
  !insertmacro ${MACRO} FileAssociations ".xht"
  !insertmacro ${MACRO} FileAssociations ".xhtml"
  !insertmacro ${MACRO} URLAssociations "http"
  !insertmacro ${MACRO} URLAssociations "https"
!macroend
!macro RegisterAssociation KIND NAME
  WriteRegStr HKCU "${CLIENTKEY}\Capabilities\${KIND}" "${NAME}" "${PROGID}"
  !if "${KIND}" == "FileAssociations"
    WriteRegStr HKCU "Software\Classes\${NAME}\OpenWithProgids" "${PROGID}" ""
  !endif
!macroend
!macro UnregisterAssociation KIND NAME
  !if "${KIND}" == "FileAssociations"
    DeleteRegValue HKCU "Software\Classes\${NAME}\OpenWithProgids" "${PROGID}"
    DeleteRegKey /ifempty HKCU "Software\Classes\${NAME}\OpenWithProgids"
    DeleteRegKey /ifempty HKCU "Software\Classes\${NAME}"
  !endif
!macroend

; Waits a few seconds for a running Vsesvit from $INSTDIR to exit (the updater quits right
; after starting the installer), then closes it through the Restart Manager. Only an
; interactive, non-update run asks first.
!macro CLOSE_APP_FUNCTION UN
; Sets $0 to 1 once nothing runs "${EXE}", polling every 250 ms for at most $1 tries.
; A running image cannot be opened for writing; the Restart Manager's process list is no
; help here, because it keeps listing a process after it exits.
Function ${UN}WaitForExe
  StrCpy $0 1
  ${IfNot} ${FileExists} "${EXE}"
    Return
  ${EndIf}
  ${Do}
    ClearErrors
    FileOpen $2 "${EXE}" a
    ${IfNot} ${Errors}
      FileClose $2
      Return
    ${EndIf}
    IntOp $1 $1 - 1
    ${If} $1 <= 0
      StrCpy $0 0
      Return
    ${EndIf}
    Sleep 250
  ${Loop}
FunctionEnd

Function ${UN}CloseApp
  StrCpy $1 20
  Call ${UN}WaitForExe
  ${If} $0 = 1
    Return
  ${EndIf}
  ${IfNot} ${Silent}
  ${AndIf} $PassiveMode <> 1
  ${AndIf} $UpdateMode <> 1
    MessageBox MB_OKCANCEL|MB_ICONINFORMATION "${PRODUCTNAME} is running. Close it and continue?" IDOK +2
    Abort "${PRODUCTNAME} is still running."
  ${EndIf}
  DetailPrint "Closing ${PRODUCTNAME}"
  !insertmacro RestartManager_StartSession $R0
  ${If} $R0 != ""
    !insertmacro RestartManager_RegisterFile $R0 "${EXE}"
    System::Call 'RSTRTMGR::RmShutdown(i R0, i ${RmForceShutdown}, p 0) i .r0'
    !insertmacro RestartManager_EndSession $R0
  ${EndIf}
  StrCpy $1 40
  Call ${UN}WaitForExe
  ${If} $0 <> 1
    MessageBox MB_OK|MB_ICONSTOP "Could not close ${PRODUCTNAME}. Close it and run the installer again." /SD IDOK
    Abort "Could not close ${PRODUCTNAME}."
  ${EndIf}
FunctionEnd
!macroend
!insertmacro CLOSE_APP_FUNCTION ""
!insertmacro CLOSE_APP_FUNCTION "un."

Function .onInit
  ${GetOptions} $CMDLINE "/P" $0
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}
  ${GetOptions} $CMDLINE "/UPDATE" $0
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
  ${GetOptions} $CMDLINE "/NS" $0
  ${IfNot} ${Errors}
    StrCpy $NoShortcutMode 1
  ${EndIf}
  ${If} ${RunningX64}
    SetRegView 64
  ${EndIf}
FunctionEnd

Section "-Runtime"
  Call EnsureRuntime
SectionEnd

Section "-Install"
  SetOutPath $INSTDIR
  Call CloseApp
  !insertmacro ForEachPayloadFile InstallPayloadFile
  WriteUninstaller "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "${ICON}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTKEY}" "URLInfoAbout" "${HOMEPAGE}"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K /G=0" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" $0

  WriteRegStr HKCU "${CLIENTKEY}" "" "${PRODUCTNAME}"
  WriteRegStr HKCU "${CLIENTKEY}\DefaultIcon" "" "${ICON}"
  WriteRegStr HKCU "${CLIENTKEY}\shell\open\command" "" '"${EXE}"'
  WriteRegStr HKCU "${CLIENTKEY}\Capabilities" "ApplicationName" "${PRODUCTNAME}"
  WriteRegStr HKCU "${CLIENTKEY}\Capabilities" "ApplicationDescription" "${DESCRIPTION}"
  WriteRegStr HKCU "${CLIENTKEY}\Capabilities" "ApplicationIcon" "${ICON}"
  WriteRegStr HKCU "${CLIENTKEY}\Capabilities\StartMenu" "StartMenuInternet" "${PRODUCTNAME}"
  !insertmacro ForEachAssociation RegisterAssociation
  WriteRegStr HKCU "Software\RegisteredApplications" "${PRODUCTNAME}" "${CLIENTKEY}\Capabilities"
  WriteRegStr HKCU "Software\Classes\${PROGID}" "" "${PRODUCTNAME} HTML Document"
  WriteRegStr HKCU "Software\Classes\${PROGID}\DefaultIcon" "" "${ICON}"
  ; `--` so a link can never be read as a command line option.
  WriteRegStr HKCU "Software\Classes\${PROGID}\shell\open\command" "" '"${EXE}" -- "%1"'
  System::Call 'shell32::SHChangeNotify(i ${SHCNE_ASSOCCHANGED}, i 0, p 0, p 0)'

  ; An update keeps whatever shortcuts the user has, including none.
  ${If} $UpdateMode <> 1
  ${AndIf} $NoShortcutMode <> 1
    CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "${EXE}" "" "${ICON}"
    ; The finish page offers the desktop shortcut; without it, do what the Tauri installer does.
    ${If} $PassiveMode = 1
    ${OrIf} ${Silent}
      Call CreateDesktopShortcut
    ${EndIf}
  ${EndIf}

  ${If} $PassiveMode = 1
    SetAutoClose true
  ${EndIf}
SectionEnd

Function .onInstSuccess
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    ${GetOptions} $CMDLINE "/R" $0
    ${IfNot} ${Errors}
      Call RelaunchArgs
      Exec '"${EXE}" $R0'
    ${EndIf}
  ${EndIf}
FunctionEnd

; $R0 = everything after " /ARGS" on the command line, verbatim. The updater quotes each
; argument like a Windows command line, so the app sees the arguments it was started with.
Function RelaunchArgs
  StrCpy $R0 ""
  StrLen $R1 $CMDLINE
  StrCpy $R2 0
  ${DoWhile} $R2 < $R1
    StrCpy $R3 $CMDLINE 6 $R2
    ${If} $R3 == " /ARGS"
      IntOp $R2 $R2 + 6
      StrCpy $R0 $CMDLINE "" $R2
      ${ExitDo}
    ${EndIf}
    IntOp $R2 $R2 + 1
  ${Loop}
FunctionEnd

Function CreateDesktopShortcut
  CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "${EXE}" "" "${ICON}"
FunctionEnd

Function SkipIfPassive
  ${IfThen} $PassiveMode = 1 ${|} Abort ${|}
FunctionEnd

Function un.onInit
  ${GetOptions} $CMDLINE "/P" $0
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}
  ${If} ${RunningX64}
    SetRegView 64
  ${EndIf}
FunctionEnd

Function un.SkipIfPassive
  ${IfThen} $PassiveMode = 1 ${|} Abort ${|}
FunctionEnd

; Adds "Delete browsing data" under the confirm page's text, as the Tauri uninstaller does.
Function un.ConfirmShow
  FindWindow $1 "#32770" "" $HWNDPARENT
  System::Call "user32::GetDpiForWindow(p r1) i .r2"
  IntOp $4 100 * $2
  IntOp $4 $4 / 96
  IntOp $5 400 * $2
  IntOp $5 $5 / 96
  IntOp $6 25 * $2
  IntOp $6 $6 / 96
  System::Call 'user32::CreateWindowEx(i ${__NSD_CheckBox_EXSTYLE}, w "${__NSD_CheckBox_CLASS}", w "Delete browsing data (history, cookies, extensions, settings)", i ${__NSD_CheckBox_STYLE}, i 0, i r4, i r5, i r6, p r1, i 0, i 0, i 0) i .s'
  Pop $DeleteDataCheckbox
  SendMessage $HWNDPARENT ${WM_GETFONT} 0 0 $1
  SendMessage $DeleteDataCheckbox ${WM_SETFONT} $1 1
FunctionEnd

Function un.ConfirmLeave
  SendMessage $DeleteDataCheckbox ${BM_GETCHECK} 0 0 $DeleteData
FunctionEnd

Section "Uninstall"
  Call un.CloseApp
  !insertmacro ForEachPayloadFile DeletePayloadFile
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  Delete "$DESKTOP\${PRODUCTNAME}.lnk"

  !insertmacro ForEachAssociation UnregisterAssociation
  DeleteRegKey HKCU "Software\Classes\${PROGID}"
  DeleteRegValue HKCU "Software\RegisteredApplications" "${PRODUCTNAME}"
  DeleteRegKey HKCU "${CLIENTKEY}"
  DeleteRegKey HKCU "${UNINSTKEY}"
  System::Call 'shell32::SHChangeNotify(i ${SHCNE_ASSOCCHANGED}, i 0, p 0, p 0)'

  ${If} $DeleteData = ${BST_CHECKED}
    RMDir /r "$LOCALAPPDATA\${PRODUCTNAME}"
  ${EndIf}

  ${If} $PassiveMode = 1
    SetAutoClose true
  ${EndIf}
SectionEnd
