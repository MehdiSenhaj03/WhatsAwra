; No longer shipped: releases after 0.20.1 ship only the Windows Installer
; package (whatsawra.wxs), which takes over a copy this script installed.
; test-installer.ps1 still builds it to make such a copy to take over.
;
; Windows installer built from a release binary with Inno Setup 6.3 or newer:
;
;   iscc /DVersion=0.1.0 /DArch=x86_64 /DBinary=...\whatsawra.exe ^
;        /DOutputDir=dist packaging\windows\whatsawra.iss
;
; Arch matches the Rust target: x86_64 or aarch64. Installation uses the
; current user's Programs folder and does not need administrator rights.
; Updates close a running copy before replacing it.

#ifndef Version
  #error Version must be defined on the ISCC command line
#endif
#ifndef Arch
  #error Arch must be defined on the ISCC command line (x86_64 or aarch64)
#endif
#ifndef NumericVersion
  #define NumericVersion Version
#endif
#ifndef Binary
  #error Binary must be defined on the ISCC command line
#endif
#ifndef OutputDir
  #error OutputDir must be defined on the ISCC command line
#endif
#if Arch == "aarch64"
  #define InnoArch "arm64"
#else
  #define InnoArch "x64compatible"
#endif

#define AppName "WhatsAwra"
#define AppExeName "whatsawra.exe"

[Setup]
; Never change: this is how Windows tells an update from a new program.
AppId={{F2512314-384A-4002-9933-AB840FD01639}
AppName={#AppName}
AppVersion={#Version}
AppVerName={#AppName} {#Version}
AppPublisher=Carmine Paolino
AppCopyright=© 2026 Carmine Paolino
AppPublisherURL=https://whatsawra.rocks
AppSupportURL=https://github.com/crmne/whatsawra/issues
AppUpdatesURL=https://github.com/crmne/whatsawra/releases
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed={#InnoArch}
ArchitecturesInstallIn64BitMode={#InnoArch}
MinVersion=10.0
LicenseFile=..\..\LICENSE
OutputDir={#OutputDir}
OutputBaseFilename=whatsawra-v{#Version}-{#Arch}-pc-windows-msvc-setup
SetupIconFile=whatsawra.ico
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
UninstallDisplayIcon={app}\{#AppExeName}
VersionInfoVersion={#NumericVersion}.0

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#Binary}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\THIRD-PARTY-NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion

Source: "whatsawra-installer.txt"; DestDir: "{app}"; Flags: ignoreversion

