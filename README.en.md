# AwayTerminal 2

A tabbed terminal: local shells, SSH, Telnet, serial ports, TTL macros, AI agent teams
and a Telegram remote.

[繁體中文](README.md)　|　MIT licence

**This is a cross-platform rewrite of [AwayTerminal 1.x](https://github.com/awaysu/AwayTerminal)**
(C# WPF + WebView2, Windows only). The goal is "every feature of the old version, the same
behaviour, faster, and cross-platform".

| | 1.2.8 | This version |
|---|---|---|
| Stack | .NET 9 + WPF + WebView2 | **Rust (Tauri 2)** + xterm.js |
| Platforms | Windows only | Windows (**done**), macOS/Linux (**code written, awaiting real hardware**) |
| Terminal rendering | DOM | **WebGL** (falls back to DOM automatically) |
| Needs the .NET runtime | Yes | **No** |
| Installer | Bundles the .NET + WebView2 installers | ~5.8 MB (WebView2 bootstrapper included) |
| UI languages | Chinese/English | **Eight** |

The terminal front-end (`src/terminal.js`) is **the original file from the old version**
with only four necessary changes (+61/−5 lines, each marked `// AT2-n` and documented in
[docs/TERMINAL-JS-DIFF.md](docs/TERMINAL-JS-DIFF.md)). The long-tuned behaviours — Bopomofo
composition, re-entering text, multi-line paste into Claude Code — are carried over as-is.

---

## Status

| Platform | State |
|---|---|
| **Windows 10/11 x64** | Feature-complete; automated and manual tests pass |
| **macOS** | Platform code written and passing `cargo check`/`clippy` for `aarch64`/`x86_64`, **never run on real hardware** |
| **Linux** | Same, for `x86_64-unknown-linux-gnu`, **never run on real hardware** |

Not done yet: code signing, auto-update (the config skeleton is in place with an empty
public key, i.e. the feature is disabled), and macOS/Linux testing and packaging.
See [docs/PLATFORM-UNIX.md](docs/PLATFORM-UNIX.md) and [CHANGELOG.md](CHANGELOG.md).

---

## Features

### Connections

| | |
|---|---|
| Local shell (Windows PowerShell/`pwsh`; macOS `$SHELL` = zsh; Linux `$SHELL` = bash) | ✅ |
| **Built-in SSH** (no longer shells out to `ssh.exe`): in-terminal `login as:`, host-key confirmation (SHA256 + MD5), PuTTY's algorithm ordering with weak-algorithm warnings, `.ppk` keys, Pageant on Windows, keepalive, automatic reconnect | ✅ |
| **Built-in Telnet**: option negotiation, IAC escaping, **NAWS** (the old version never sent it), `TTYPE` | ✅ |
| **Serial ports**: friendly USB names, baud/bits/parity/flow control | ✅ |
| WSL, ADB (device picker via `adb devices`) | ✅ |
| Custom connections (Claude Code, Codex CLI, Gemini CLI, OpenCode, QwenCode, Antigravity CLI, Grok CLI… with auto-detection) | ✅ |

### Terminal and UI

Tabs / split / columns (drag to rearrange), right-hand tab strip (resizable, hideable,
drag to reorder, status lights, rename, runtime in the tooltip), copy / paste-as-plain-text /
copy-all / save, Ctrl+F search, URL menu, OSC 52 clipboard, Ctrl+wheel zoom, per-tab colours,
clear screen (with confirmation), the compose window (UTF-8/Big5), session logging (ANSI
stripped, timestamps, append), favourites, session restore (**including scrollback
playback**), keepalive, update check, About page.

Eight UI languages: Traditional Chinese, English, Simplified Chinese, Japanese, Korean,
Spanish, German, French.

### TTL macros

Translated into Rust section by section from TeraTerm's own `ttpmacro/` sources.
**139 of the 214 reserved words are implemented** — considerably more than the old
hand-written C# interpreter. Includes `if/elseif/else`, `for/while/until/do`,
`break/continue`, `goto/call`, `include`, arrays, `sprintf`, date and time, 37 file and
directory commands, regular expressions ([`fancy-regex`; 28 of 30 measured Oniguruma
constructs](docs/TTL-REGEX.md)), and CRC/checksums.

⚠️ **Three places where the old C# interpreter disagreed with TeraTerm are now fixed**:
`and`/`or`/`xor`/`not` are **bitwise** (not logical), bitwise operators bind tighter than
comparisons, and integers are 32-bit and wrap. Old `.ttl` files that relied on the wrong
behaviour will produce different results.

### New features (not in the old version)

- **Sandbox mode** (an option for custom connections and agent teams, off by default): a `git worktree`
  per tab, `TEMP`/`CARGO_TARGET_DIR` redirected into the sandbox (**`HOME`/`APPDATA` are
  deliberately not isolated** — that would log Claude Code and Codex out), a Windows Job
  Object with kill-on-close (process groups on Unix), and generated guard configuration for
  each tool (Claude Code's `PreToolUse` hook refuses `taskkill /IM`, deleting paths outside
  the repo, `git push --force`).
  **This is a guard rail, not a sandbox** — the agents share the user's login session and
  can still reach the desktop if they go around it
  ([docs/AGENT-SANDBOX.md](docs/AGENT-SANDBOX.md) is explicit about this).
- **Agent teams**: two to four AI CLIs, each with a role, writing to each other through an
  `.ai/bus/` mailbox, and only typing into a pane while its agent is idle.
- **AI chat room**: 21 roles taking turns, with a host writing the conclusion into `.ai/chat/`.
- **Telegram remote**: read panes, send commands, take screenshots, get a push when a tab
  finishes, with a 37-rule noise filter.
- Per-tab "push to Telegram" toggle, window geometry persistence, "Select all" in the
  terminal context menu.

---

## Download

Download from <https://www.awaysu.cc/software/awayterminal>.
For Windows there is an installer (`.exe`) and a portable version (`.zip`).

⚠️ The binaries are **not code-signed yet**, so Windows SmartScreen will warn
([docs/RELEASE.md](docs/RELEASE.md) explains why and what the plan is).

### Upgrading from 1.x

On first run it offers to import `%LOCALAPPDATA%\AwayTerminal\settings.json`
(**read-only; the old file is never modified**). The field mapping and the 21 deliberately
skipped keys are in [docs/MIGRATION.md](docs/MIGRATION.md). Both versions can coexist —
they store settings in different places.

---

## Building from source

You need Rust (1.89+), Node 20+, and your platform's webview development packages.
Full instructions, pitfalls and the directory layout are in
**[docs/DEV-SETUP.md](docs/DEV-SETUP.md)**.

```bash
npm ci
npm run tauri dev      # development
npm run tauri build    # produce installers
```

The checks that must pass before a release (full list in
[docs/RELEASE.md](docs/RELEASE.md)):

```bash
cd src-tauri && cargo test --lib && cargo clippy --all-targets -- -D warnings
cargo deny check                           # licences and security advisories
cd .. && node scripts/test-i18n.mjs        # no missing strings in any of the 8 languages
node scripts/i18n-audit.mjs                # no user-visible Chinese literal left unclassified
node scripts/test-bridge-args.mjs          # every session_create parameter is actually passed
npm run verify                             # end to end (opens a window, closes itself)
npm run verify:release                     # same, against the release binary
```

---

## Documentation

All documentation is in Traditional Chinese (it is written for the project's own
maintenance). [CLAUDE.md](CLAUDE.md) is the specification; the index below is in the
Chinese README: **[文件索引](README.md#文件)**.

Quick pointers:

| Topic | File |
|---|---|
| Specification, technology choices, risks, plan | [CLAUDE.md](CLAUDE.md) |
| Development environment and build | [docs/DEV-SETUP.md](docs/DEV-SETUP.md) |
| The four changes to `terminal.js`, and the IME/paste pitfalls carried over | [docs/TERMINAL-JS-DIFF.md](docs/TERMINAL-JS-DIFF.md) |
| macOS/Linux: what is written, what needs real hardware, day-one checklist | [docs/PLATFORM-UNIX.md](docs/PLATFORM-UNIX.md) |
| Regression checklist (automated + 👤 manual), deliberate differences, implicit contracts | [docs/REGRESSION-CHECKLIST.md](docs/REGRESSION-CHECKLIST.md) |
| Manual test plan, grouped P0/P1/P2 into 20–30 minute sessions | [docs/MANUAL-TEST-PLAN.md](docs/MANUAL-TEST-PLAN.md) |
| Release process, signing, updater | [docs/RELEASE.md](docs/RELEASE.md) |
| **Security notes for users** (token storage, sandbox limits, SSH keys, reporting) | [docs/SECURITY.md](docs/SECURITY.md) |

---

## Licence

MIT (see [LICENSE](LICENSE)).

Third-party components are used under MIT, Apache-2.0, BSD-3-Clause, MPL-2.0 and others
(`russh` is Apache-2.0, `serialport-rs` is MPL-2.0, TeraTerm's `ttpmacro` is BSD-3-Clause).
The full inventory — verified with `cargo deny` across four targets — and every licence
text is in **[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)**, which the installers also
place next to the executable. **No GPL/LGPL/AGPL code is linked.**

Behaviour was referenced from
[PuTTY](https://www.chiark.greenend.org.uk/~sgtatham/putty/) (the SSH user flow),
[TeraTerm](https://github.com/TeraTermProject/teraterm) (TTL macros) and
[microsoft/terminal](https://github.com/microsoft/terminal) (ConPTY and OpenConsole).
