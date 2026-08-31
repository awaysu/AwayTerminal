# Windows Terminal ConPTY host (conpty.dll + OpenConsole.exe)

Unmodified binaries from the **Windows Terminal** project (https://github.com/microsoft/terminal),
MIT licence, Authenticode-signed by Microsoft Corporation. See `THIRD-PARTY-NOTICES.md` §2.

| File | Version | SHA-256 |
|---|---|---|
| `conpty.dll` | 1.23.2510.08001 | `7c7430632052ff703540b68371ec43821820aa1335d8e11dfbcd9ff00e9daaed` |
| `OpenConsole.exe` | 1.23.2510.08001 | `d1fe7faa62f9e955e2ac2371f95d7e5513df4d496255097158f979c94782c5fc` |

Taken verbatim from the npm package `node-pty@1.1.0`
(`third_party/conpty/1.23.251008001/win10-x64/`), which is how VS Code ships them.

## Why

AwayTerminal creates its pseudo-console through `CreatePseudoConsole`. On Windows 10 the
inbox `conhost.exe` (10.0.19041.x, 2019-era ConPTY) re-renders the whole screen with absolute
cursor positioning on every change, drops bracketed-paste markers, flattens the alternate
screen, converts soft wraps into hard line breaks, and swallows multi-character input that
follows a lone ESC (the "type after pressing Esc and the sentence vanishes" bug with Claude Code).
Windows Terminal does not have these problems because it ships its own, current `OpenConsole.exe`
as the ConPTY host. `conpty.dll` is the small loader that spawns `OpenConsole.exe` from the same
directory and exposes `ConptyCreatePseudoConsole` / `ConptyResizePseudoConsole` /
`ConptyClosePseudoConsole` / `ConptyReleasePseudoConsole`.

## How it is used

The build copies both files to `<output>\conpty\`. `ConPty/ConptyDll.cs` loads
`conpty\conpty.dll` next to `AwayTerminal.exe` at start-up; if either file is missing it silently
falls back to kernel32's `CreatePseudoConsole` (inbox conhost), so removing the folder is a valid
way to go back to the old behaviour. Environment overrides for testing:
`AWAYTERMINAL_CONPTY=inbox` (force inbox conhost), `AWAYTERMINAL_CONPTY_DIR=<dir>` (use another copy).

## Updating

Grab a newer `node-pty` (or build `conpty.dll`/`OpenConsole.exe` from microsoft/terminal), replace
both files together (they must be from the same build), verify the Authenticode signature
(`Get-AuthenticodeSignature`) and update the table above and the version in THIRD-PARTY-NOTICES.md.
