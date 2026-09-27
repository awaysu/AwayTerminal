# Third-party notices

AwayTerminal 2 is licensed under the MIT licence. It ships and/or borrows from the
third-party components listed below. Each section states what is included, under which
licence, and — where code was ported rather than linked — which part of this project it
went into.

---

## 1. xterm.js — MIT

<https://github.com/xtermjs/xterm.js>

Packages `@xterm/xterm` and the addons `@xterm/addon-webgl`, `@xterm/addon-fit`,
`@xterm/addon-unicode11`, `@xterm/addon-web-links`, `@xterm/addon-serialize`,
`@xterm/addon-search`. Bundled into the frontend by Vite.

```
Copyright (c) 2017-2022, The xterm.js authors (https://github.com/xtermjs/xterm.js)
Copyright (c) 2014-2016, SourceLair, Sàrl (https://www.sourcelair.com)
Copyright (c) 2012-2013, Christopher Jeffrey (https://github.com/chjj/)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## 2. Windows Terminal (microsoft/terminal) — MIT

<https://github.com/microsoft/terminal>

### 2.1 Redistributed binaries

`src-tauri/resources/conpty/` contains **unmodified** binaries from the Windows Terminal
project, Authenticode-signed by Microsoft Corporation. They are copied next to the
executable (as `conpty/`) at build time and are also bundled into the MSI / NSIS installers.

| File | Version | SHA-256 |
|---|---|---|
| `conpty.dll` | 1.23.2510.08001 | `7c7430632052ff703540b68371ec43821820aa1335d8e11dfbcd9ff00e9daaed` |
| `OpenConsole.exe` | 1.23.2510.08001 | `d1fe7faa62f9e955e2ac2371f95d7e5513df4d496255097158f979c94782c5fc` |

Taken verbatim from the npm package `node-pty@1.1.0`
(`third_party/conpty/1.23.251008001/win10-x64/`), which is how VS Code ships them.
See `src-tauri/resources/conpty/README.md` for why AwayTerminal uses this ConPTY host
instead of the Windows 10 inbox `conhost.exe`, and for update instructions.

### 2.2 Behaviour reference

Windows Terminal is also used as the behaviour reference for ConPTY handling in
`src-tauri/src/pty/` (pipe setup, `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`,
`ConptyReleasePseudoConsole` after the child is attached). No source code was copied.

```
Copyright (c) Microsoft Corporation.

MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED *AS IS*, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## 3. Tauri — MIT / Apache-2.0

<https://github.com/tauri-apps/tauri>

The Rust backend and the `@tauri-apps/api` frontend package. Used as a dependency,
dual-licensed MIT or Apache-2.0; this project uses it under the MIT option.

---

## 4. russh — Apache-2.0

<https://github.com/Eugeny/russh>

The built-in SSH client (`src-tauri/src/ssh/`). Linked as a dependency, **not** ported.

> ⚠️ **russh is Apache-2.0 only — not MIT.** Apache-2.0 is a permissive licence and is
> compatible with shipping it inside this MIT-licensed application, but it carries
> obligations MIT does not: the licence text must accompany the distribution, any
> `NOTICE` file from the upstream project must be reproduced, and modified files must be
> marked. We link it unmodified, so the requirement is to ship the licence text with the
> installers. `CLAUDE.md` lists the licences of the behaviour references (PuTTY, TeraTerm,
> microsoft/terminal, xterm.js) but did not state russh's — recording it here.

Built with `default-features = false, features = ["ring", "des", "rsa", "flate2"]`:

- `ring` instead of the default `aws-lc-rs`, because `aws-lc-rs` needs NASM installed to
  build on Windows (`NASM command not found! Build cannot continue.`).
- `des` for `3des-cbc`, and `rsa` for `ssh-rsa`, both needed by the old network devices
  listed under risk 3 in `CLAUDE.md`.

Notable crates pulled in by russh and shipped with it:

| Crate | Licence |
|---|---|
| `ssh-key`, `ssh-encoding`, `ssh-cipher` | Apache-2.0 OR MIT |
| `ring` | Apache-2.0 AND ISC (contains BoringSSL-derived code) |
| `pageant` | Apache-2.0 |
| `md5` | Apache-2.0 OR MIT |

---

## 5. serialport-rs — MPL-2.0

- Used for: the serial port (COM) backend, `src-tauri/src/com/`
- Upstream: <https://github.com/serialport/serialport-rs>
- Version: 4.10

**MPL-2.0 is not MIT.** It is a *file-level* copyleft licence, which is compatible with
shipping inside an MIT-licensed application on two conditions:

1. the licence text must accompany the distribution (this NOTICES file has to be included
   in the installers — stage 5);
2. **if we modified any of its source files**, those files must be released under MPL-2.0.

We do **not** modify `serialport`; it is used strictly as a library, so only (1) applies.
Transitive dependencies it pulls in: `nix` (MIT) and, on macOS only, `io-kit-sys`
(MIT OR Apache-2.0).

---

## 6. TeraTerm (ttpmacro) — BSD-3-Clause

- Used for: the TTL macro interpreter, `src-tauri/src/ttl/` — **translated section by section from**
  `ttpmacro/ttl.cpp`, `ttmparse.cpp`, `ttmbuff.c` and `ttmparse.h` (the reserved-word table
  and error codes are taken directly from the source).
- Upstream: <https://github.com/TeraTermProject/teraterm>
- Copyright (C) 1994-1998 T. Teranishi / (C) 2005- TeraTerm Project

The reference checkout lives in `reference/teraterm/` and is **not** part of this repository
(`reference/` is git-ignored). BSD-3-Clause requires the copyright notice, the conditions and
the disclaimer to accompany redistributions, so the full licence text below must ship with the
installers (stage 5).

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:

1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.
3. The name of the author may not be used to endorse or promote products
   derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE AUTHORS ``AS IS'' AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY DIRECT, INDIRECT,
INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

---

## 7. fancy-regex — MIT

- Used for: the regular-expression engine behind the TTL commands `strmatch`,
  `strreplace`, `waitregex` and `regexoption` (`src-tauri/src/ttl/regex.rs`).
- Upstream: <https://github.com/fancy-regex/fancy-regex>
- Licence: MIT (same as this project, so only attribution is required).

Chosen over the `regex` crate because TeraTerm's Oniguruma engine supports look-behind and
back-references, which `regex` deliberately does not. The measured feature-by-feature
comparison is in `docs/TTL-REGEX.md`; no Oniguruma code is used (it is a C library and was
not vendored).

---

## 8. encoding_rs — (Apache-2.0 OR MIT) AND BSD-3-Clause

- Used for: decoding **Big5** (and UTF-16) text files in the 輸入文字 / compose window
  (`src-tauri/src/compose.rs`), replacing .NET's `Encoding.Default` (cp950) from v1.
- Upstream: <https://github.com/hsivonen/encoding_rs>
- Copyright: Mozilla Foundation.

> ⚠️ The licence is a **conjunction**: the code is Apache-2.0 OR MIT (our choice: MIT),
> **and** the encoding tables derived from the WHATWG Encoding Standard are BSD-3-Clause
> (© WHATWG — Apple, Google, Mozilla, Microsoft). The BSD-3 notice, conditions and
> disclaimer must therefore accompany the installers (stage 5) — the same requirement the
> TeraTerm section above already creates, so one BSD-3 text with both copyright lines is
> enough.

---

## 9. ureq — MIT OR Apache-2.0

- Used for: the one outbound HTTPS request this program makes — **"Check for updates"**,
  and only when the user presses that button (`src-tauri/src/update.rs`).
- Upstream: <https://github.com/algesten/ureq>
- Licence: MIT OR Apache-2.0 (we take MIT, same as this project).

Brings in the rustls TLS stack (`rustls`, `webpki-roots`, `ring`) — all permissive
(Apache-2.0 / ISC / MIT); `ring` is already in the tree via russh. Chosen over `reqwest`
because a blocking one-shot GET needs neither an async HTTP stack nor the system
certificate store, which keeps behaviour identical on Windows, macOS and Linux.

---

## 10. sys-locale — MIT OR Apache-2.0

- Used for: reading the operating system's language **once**, on first run, to pick one of the
  eight UI languages (`src-tauri/src/i18n.rs`, `system_locale`).
- Upstream: <https://github.com/1Password/sys-locale>
- Read-only: it queries the OS locale and changes nothing.

---

## 11. winreg / tauri-plugin-single-instance — MIT

- `winreg` — used for the Explorer context menu: it writes two keys under **HKCU only**
  (`src-tauri/src/shellmenu.rs`). Upstream: <https://github.com/gentoo90/winreg-rs>. MIT.
- `tauri-plugin-single-instance` — used so the Explorer context menu hands the folder to the
  window that is already running instead of opening a second one (replaces v1's named pipe).
  Part of the Tauri plugins workspace; MIT / Apache-2.0.

---

## 12. Rust crates

Linked as dependencies, each under MIT or MIT/Apache-2.0:
`windows-sys`, `libloading`, `serde`, `serde_json`, `tokio`, `chrono`,
`tauri-plugin-dialog`, `tauri-plugin-opener`, `md5`, `sys-locale`.
Full per-crate licence text is reproduced by `cargo about` / `cargo license` output.

---

## Test fixtures

`src-tauri/tests/keys/id_ed25519.ppk` and `id_ed25519_enc.ppk` are copied from the
**ssh-key** project's test vectors (`tests/examples/`, Apache-2.0 OR MIT). They are
PuTTY-format private keys used only by `cargo run --example ssh_probe` to prove that
`.ppk` files load; the encrypted one's passphrase is `123`. They are **not** used by the
application and must never be treated as real credentials.

---

## Still to be added

These are planned for later stages and are listed here so the notices file tracks the
plan in `CLAUDE.md`:

- **PuTTY** (MIT) — SSH behaviour reference (dialog wording, host-key cache semantics,
  algorithm ordering). No PuTTY code has been copied.
- The **full licence texts** of every crate in the dependency tree, generated with
  `cargo about`, bundled into the installers (stage 5). Sections 4–8 above are the ones
  that need more than a name in a list: russh (Apache-2.0), serialport-rs (MPL-2.0),
  TeraTerm (BSD-3), encoding_rs (BSD-3 tables), ureq's rustls stack (Apache-2.0 / ISC).

（TeraTerm and serialport-rs used to be listed here; they now have their own sections
above — 6 and 5 — because the code that uses them has landed.）
