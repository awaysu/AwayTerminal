; AwayTerminal installer (Inno Setup)
; Installs the app (self-contained .NET) and the WebView2 runtime if it is missing.
;
; Setup does NOT touch the machine's certificate stores. Up to 1.0.11 it silently
; imported the self-signed certificate into Trusted Root + Trusted Publisher; that was
; removed in 1.0.12 (see the [Run] section for the reasoning). Trusting the publisher
; is now opt-in and per-user via {app}\trust-publisher.ps1.

#define AppName "AwayTerminal"
#define AppVersion "1.0.12"
#define AppPublisher "awaysu@gmail.com"
#define AppExe "AwayTerminal.exe"

[Setup]
AppId={{A8F5C3B1-9D2E-4F6A-B7C8-1234567890AB}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
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

[Files]
Source: "..\bin\publish\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion
; The public certificate and an opt-in trust script ship into the app folder.
; They are NOT applied during setup - the user runs the script only if they want
; the publisher name shown in UAC. See [Run] for why setup no longer does this.
Source: "AwayTerminal.cer"; DestDir: "{app}"
Source: "trust-publisher.ps1"; DestDir: "{app}"
Source: "MicrosoftEdgeWebview2Setup.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall

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
; Install the WebView2 runtime only if it is not present (idempotent bootstrapper)
Filename: "{tmp}\MicrosoftEdgeWebview2Setup.exe"; Parameters: "/silent /install"; Check: WebView2Missing; Flags: runhidden waituntilterminated; StatusMsg: "Installing WebView2 runtime..."
; Offer to launch after install
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "certutil.exe"; Parameters: "-delstore Root ""AwayTerminal (awaysu)"""; Flags: runhidden; RunOnceId: "DelRootCert"
Filename: "certutil.exe"; Parameters: "-delstore TrustedPublisher ""AwayTerminal (awaysu)"""; Flags: runhidden; RunOnceId: "DelPubCert"

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
