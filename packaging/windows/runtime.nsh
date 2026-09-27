; The Windows App Runtime 2.x that crates/vsesvit-winui/src/platform.rs loads at startup.
; Framework packages install per user, so none of this needs elevation.

!include LogicLib.nsh

!define RUNTIME_FAMILY "Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe"
; PACKAGE_VERSION of 2.5.1.0: (2 << 48) | (5 << 32) | (1 << 16).
!define RUNTIME_MIN_VERSION 562971428323328
; PackageDependencyProcessorArchitectures_Neutral | _X64, as platform.rs passes on x64.
!define RUNTIME_ARCHITECTURES 5
!define RUNTIME_URL "https://aka.ms/windowsappsdk/2.5/latest/windowsappruntimeinstall-x64.exe"

; Sets $0 to 1 when a runtime at least RUNTIME_MIN_VERSION is registered for this user, else 0.
; It asks the dynamic-dependency API the app itself uses, so both agree on "installed".
Function RuntimeInstalled
  System::Call 'kernelbase::TryCreatePackageDependency(p 0, w "${RUNTIME_FAMILY}", l ${RUNTIME_MIN_VERSION}, i ${RUNTIME_ARCHITECTURES}, i 0, p 0, i 0, *p .r1) i .r0'
  ${If} $0 == 0
    StrCpy $0 1
  ${Else}
    StrCpy $0 0
  ${EndIf}
FunctionEnd

; Downloads and runs Microsoft's runtime installer when the runtime is missing. If that fails,
; asks whether to install anyway; silent installs abort, which leaves an existing install as is.
Function EnsureRuntime
  Call RuntimeInstalled
  ${If} $0 = 1
    DetailPrint "Windows App Runtime 2.5.1 or newer is installed"
    Return
  ${EndIf}

  InitPluginsDir
  StrCpy $1 "$PLUGINSDIR\windowsappruntimeinstall-x64.exe"
  DetailPrint "Downloading the Windows App Runtime (about 120 MB) from ${RUNTIME_URL}"
  System::Call 'urlmon::URLDownloadToFileW(p 0, w "${RUNTIME_URL}", w r1, i 0, p 0) i .r0'
  ${If} $0 != 0
    IntFmt $2 "0x%08X" $0
    StrCpy $2 "The download failed (error $2)."
  ${Else}
    DetailPrint "Installing the Windows App Runtime"
    ClearErrors
    ExecWait '"$1" --quiet' $3
    ${If} ${Errors}
      StrCpy $2 "The runtime installer could not be started."
    ${Else}
      Call RuntimeInstalled
      ${If} $0 = 1
        DetailPrint "Windows App Runtime installed"
        Return
      ${EndIf}
      IntFmt $3 "0x%08X" $3
      StrCpy $2 "The runtime installer exited with code $3."
    ${EndIf}
  ${EndIf}

  DetailPrint "Windows App Runtime: $2"
  MessageBox MB_YESNO|MB_ICONEXCLAMATION "${PRODUCTNAME} needs the Windows App Runtime 2.x (2.5.1 or newer, x64), and it could not be installed.$\r$\n$\r$\n$2$\r$\n$\r$\nYou can install it later from https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads.$\r$\n$\r$\nInstall ${PRODUCTNAME} anyway?" /SD IDNO IDYES +2
  Abort "The Windows App Runtime could not be installed: $2"
FunctionEnd
