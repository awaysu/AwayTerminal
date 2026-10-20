# AwayTerminal

**A tabbed terminal built for coding with AI agents**, for Windows, macOS and Linux.

[繁體中文](README.md)　|　MIT licence　|　Download: <https://www.awaysu.cc/software/awayterminal>

---

## For working with AI agents

- **One-click AI CLIs**: Claude Code, Codex, OpenCode, Antigravity, Grok and more; auto-detects what is installed on this machine.
- **Multi-Agent team**: mix different AI agents with 2–4 roles (PM, engineer, QA…) working side by side in one window.
- **AI Chat Room**: mix different AI agents with 2–4 roles taking turns on one topic; the host wraps up with a conclusion.
- **Telegram remote**: watch the screen, send commands and get a push when a task finishes, from your phone.
- **GitHub repo list**: download a repo directly and hand it to an AI agent to edit.
- **Busy indicator**: busy tabs and the taskbar icon get a red badge, so you can minimise the window and do something else.
- **Usage quota**: Claude Code and Codex usage shown live in the top-right corner.

## Regular terminal features

- Local shell, built-in SSH (PuTTY-style, `.ppk`, auto-reconnect), Telnet, serial ports, WSL, ADB.
- Tabs / split / columns, search, logging, favourites, restore tabs with scrollback, eight UI languages.
- TeraTerm-compatible TTL macros.

---

## Screenshots

### AI CLIs and the busy indicator

Claude Code, Codex, OpenCode, Antigravity, Grok and more. Busy tabs and the taskbar icon get a red badge, so you can minimise the window and do something else; Claude Code and Codex usage is shown live in the top-right corner.

![New Tab menu and the usage quota in the top-right corner](docs/img/awterminal01.jpg)

### Multi-Agent team

Mix different AI agents with 2–4 roles side by side in one window; agents message each other through the project's `.ai/bus/` mailbox.

![Multi-Agent team setup](docs/img/awterminal07.jpg)
![A team at work: one AI per pane](docs/img/awterminal08.jpg)

### AI Chat Room

Mix different AI agents with 2–4 roles taking turns on one topic; the host wraps up with a conclusion.

![AI Chat Room setup](docs/img/awterminal11.jpg)
![AI Chat Room in progress](docs/img/awterminal12.jpg)

### Telegram remote

Enter a bot token and chat ID in Settings, then watch the screen and send commands from your phone; a push arrives when a task finishes.

![Settings → Remote: Telegram](docs/img/awterminal04.jpg)
![Claude Code finishing a task](docs/img/awterminal13.jpg)
<img src="docs/img/telegram01.jpg" alt="The completion push on the phone" width="360">

### GitHub repo list

Lists your repos (including private ones); pick one to download and hand to an AI agent.

![GitHub repo list](docs/img/awterminal05.jpg)

### Compose text

Type multi-line text, or load a prompt you wrote earlier from a text file, and send it to the AI CLI in one go.

![Compose text dialog](docs/img/awterminal06.jpg)

### macOS and Linux

![Linux](docs/img/awterminal-linux.jpg)
![macOS](docs/img/awterminal-mac.jpg)

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
