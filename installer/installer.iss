; AwayTerminal installer (Inno Setup)
; Installs the app (framework-dependent .NET), plus the .NET Desktop Runtime and the
; WebView2 runtime when either is missing. The MSIX/Store build stays self-contained -
; see msix\build-msix.ps1 - because a package cannot install the .NET runtime itself.
;
; Setup does NOT touch the machine's certificate stores. Up to 1.0.11 it silently
; imported the self-signed certificate into Trusted Root + Trusted Publisher; that was
; removed in 1.0.12 (see the [Run] section for the reasoning). Trusting the publisher
; is now opt-in and per-user via {app}\trust-publisher.ps1.
;
; NOTE: keep this file ASCII-only. A .iss containing non-ASCII text needs a UTF-8 BOM,
; and re-encoding it with Set-Content has already corrupted the comments once.

#define AppName "AwayTerminal"
#define AppVersion "1.1.1"
#define AppPublisher "Chih-Wei Su (Awaysu)"
#define AppCopyright "Copyright (c) 2026 Chih-Wei Su (Awaysu)"
#define AppExe "AwayTerminal.exe"

[Setup]
AppId={{A8F5C3B1-9D2E-4F6A-B7C8-1234567890AB}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
; Shown on the setup wizard and copied into the generated Setup exe's version info
AppCopyright={#AppCopyright}
VersionInfoCopyright={#AppCopyright}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
OutputDir=.\dist
OutputBaseFilename=AwayTerminal-Setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
SetupIconFile=..\app.ico
UninstallDisplayIcon={app}\{#AppExe}
WizardStyle=modern

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"

; Wipe the install folder before copying. Inno upgrades in place and never removes
; files that a previous version installed but the new one no longer ships, so without
; this an upgrade from <=1.0.12 keeps ~172 MB of orphaned self-contained .NET runtime
; files plus the bundled tools\adb - exactly the space the move to a framework-dependent
; build was meant to reclaim. Nothing user-generated lives here (settings are in
; %LOCALAPPDATA%, logs in Documents), so a clean slate is safe.
; InstallDelete runs before [Files], so the app's own files are restored immediately.
[InstallDelete]
Type: filesandordirs; Name: "{app}\*"

[Files]
Source: "..\bin\publish\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion
; The public certificate and an opt-in trust script ship into the app folder.
; They are NOT applied during setup - the user runs the script only if they want
; the publisher name shown in UAC. See [Run] for why setup no longer does this.
Source: "AwayTerminal.cer"; DestDir: "{app}"
Source: "trust-publisher.ps1"; DestDir: "{app}"
Source: "MicrosoftEdgeWebview2Setup.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall
; .NET 9 Desktop Runtime - only run when no 9.x is present (see Check in [Run])
Source: "windowsdesktop-runtime-win-x64.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
; NOTE: setup deliberately no longer installs the publisher certificate into the
; machine's Trusted Root store. A silent, hidden "certutil -addstore -f Root" run
; with admin rights is indistinguishable from MITRE ATT&CK T1553.004 (Subvert Trust
; Controls: Install Root Certificate) and is a likely cause of antivirus detections.
; It also asked every user to trust a root CA whose private key lives on a dev box.
; Users who want the publisher name in UAC run {app}\trust-publisher.ps1 themselves,
; which imports into their own CurrentUser stores instead of machine-wide.
;
; One-time cleanup: remove the machine-wide certificate that setup versions up to
; 1.0.11 installed. This only touches LocalMachine, so it never undoes a user's own
; opt-in via trust-publisher.ps1 (that writes to CurrentUser).
; A non-zero exit (certificate not present, i.e. a clean install) is ignored by Inno.
Filename: "certutil.exe"; Parameters: "-delstore Root ""AwayTerminal (awaysu)"""; Flags: runhidden
Filename: "certutil.exe"; Parameters: "-delstore TrustedPublisher ""AwayTerminal (awaysu)"""; Flags: runhidden
; Install the .NET Desktop Runtime only when no 9.x is present.
; The app is published framework-dependent (3 MB instead of 172 MB), so the shared
; runtime is required. Bundling the official installer keeps setup usable offline;
; once installed, Microsoft services the runtime through Windows Update instead of
; users waiting for an AwayTerminal release to pick up a .NET security fix.
Filename: "{tmp}\windowsdesktop-runtime-win-x64.exe"; Parameters: "/install /quiet /norestart"; Check: DesktopRuntimeMissing; Flags: runhidden waituntilterminated; StatusMsg: "Installing .NET Desktop Runtime..."
; Install the WebView2 runtime only if it is not present (idempotent bootstrapper)
Filename: "{tmp}\MicrosoftEdgeWebview2Setup.exe"; Parameters: "/silent /install"; Check: WebView2Missing; Flags: runhidden waituntilterminated; StatusMsg: "Installing WebView2 runtime..."
; Offer to launch after install
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "certutil.exe"; Parameters: "-delstore Root ""AwayTerminal (awaysu)"""; Flags: runhidden; RunOnceId: "DelRootCert"
Filename: "certutil.exe"; Parameters: "-delstore TrustedPublisher ""AwayTerminal (awaysu)"""; Flags: runhidden; RunOnceId: "DelPubCert"
; 1.0.45: the app registers an "Open in AwayTerminal" folder context-menu verb under HKCU at
; startup (Services\ShellIntegration.cs). Setup never writes it; remove it here so the entry
; does not point at a deleted exe after uninstall. Missing keys make reg.exe exit non-zero,
; which the uninstaller ignores.
Filename: "reg.exe"; Parameters: "delete ""HKCU\Software\Classes\Directory\shell\AwayTerminal"" /f"; Flags: runhidden; RunOnceId: "DelShellMenuDir"
Filename: "reg.exe"; Parameters: "delete ""HKCU\Software\Classes\Directory\Background\shell\AwayTerminal"" /f"; Flags: runhidden; RunOnceId: "DelShellMenuBg"

[Code]
function WebView2Installed: Boolean;
var
  v: String;
begin
  Result :=
    (RegQueryStringValue(HKLM64, 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', v) and (v <> '') and (v <> '0.0.0.0')) or
    (RegQueryStringValue(HKLM32, 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', v) and (v <> '') and (v <> '0.0.0.0')) or
    (RegQueryStringValue(HKCU,   'Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', v) and (v <> '') and (v <> '0.0.0.0'));
end;

function WebView2Missing: Boolean;
begin
  Result := not WebView2Installed;
end;

// Detect the .NET Desktop Runtime by scanning
// %ProgramFiles%\dotnet\shared\Microsoft.WindowsDesktop.App for a 9.* version folder.
// Deliberately NOT a registry check: HKLM\SOFTWARE\dotnet\Setup\InstalledVersions\x64\sharedfx
// does not exist on the tested Win10 19045 machine even though the runtime is installed,
// so the folder scan is the reliable test.
function DesktopRuntimeInstalled: Boolean;
var
  base: String;
  fr: TFindRec;
begin
  Result := False;
  base := ExpandConstant('{commonpf64}\dotnet\shared\Microsoft.WindowsDesktop.App');
  if not DirExists(base) then Exit;
  if FindFirst(base + '\*', fr) then
  begin
    try
      repeat
        if (fr.Attributes and FILE_ATTRIBUTE_DIRECTORY) <> 0 then
          if Copy(fr.Name, 1, 2) = '9.' then
          begin
            Result := True;
            Exit;
          end;
      until not FindNext(fr);
    finally
      FindClose(fr);
    end;
  end;
end;

function DesktopRuntimeMissing: Boolean;
begin
  Result := not DesktopRuntimeInstalled;
end;
