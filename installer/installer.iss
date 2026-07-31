; AwayTerminal installer (Inno Setup)
; Installs the app (self-contained .NET), imports the self-signed certificate into the
; machine's Trusted Root + Trusted Publisher stores so the app signature is trusted,
; and installs the WebView2 runtime if it is missing.

#define AppName "AwayTerminal"
#define AppVersion "1.0.11"
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
Source: "AwayTerminal.cer"; DestDir: "{tmp}"; Flags: deleteafterinstall
Source: "MicrosoftEdgeWebview2Setup.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
; Trust the self-signed certificate (so the app's Authenticode signature is valid on this PC)
Filename: "certutil.exe"; Parameters: "-addstore -f Root ""{tmp}\AwayTerminal.cer"""; Flags: runhidden; StatusMsg: "Installing certificate (Trusted Root)..."
Filename: "certutil.exe"; Parameters: "-addstore -f TrustedPublisher ""{tmp}\AwayTerminal.cer"""; Flags: runhidden; StatusMsg: "Installing certificate (Trusted Publisher)..."
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
