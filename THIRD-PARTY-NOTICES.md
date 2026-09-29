# Third-party notices

AwayTerminal 2 is licensed under the MIT licence. It ships and/or borrows from the
third-party components listed below. Each section states what is included, under which
licence, and — where code was ported rather than linked — which part of this project it
went into.

---

## 1. xterm.js — MIT

<https://github.com/xtermjs/xterm.js>

Packages `@xterm/xterm` and the addons `@xterm/addon-webgl`, `@xterm/addon-fit`,
`@xterm/addon-unicode11`, `@xterm/addon-web-links`, `@xterm/addon-serialize`.
Bundled into the frontend by Vite.

`@xterm/addon-search` was listed here and installed, but **nothing ever imported it** —
`src/terminal.js` implements Ctrl+F search itself (scanning `term.buffer`, with the
column-mapping fixes for emoji and combining characters), which is why
`docs/TERMINAL-JS-DIFF.md` says the addon must *not* be added. Removed in TASK-023.

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

## 12. ttf-parser — MIT OR Apache-2.0

- Used for: listing the font families actually installed on the machine for the Settings
  dialog's font picker (`src-tauri/src/fonts.rs`). It parses each font file's `name` table
  for the family name and the `post` table's `isFixedPitch` flag to mark monospaced families.
- Upstream: <https://github.com/RazrFalcon/ttf-parser>
- Read-only, no dependencies, default features disabled (only `std`): it reads font files and
  changes nothing. No font data is redistributed — we only read what the OS already has.

---

## 13. Bundled fonts — SIL Open Font License 1.1

AwayTerminal **ships these font files** in `src-tauri/fonts/` and installs them with the
application (Tauri resources). They are loaded into the webview with the `FontFace` API at
startup — **they are never installed into the operating system**, so uninstalling
AwayTerminal removes them completely and no administrator rights are needed.

| Font | Files | Size | Copyright |
|---|---|---|---|
| **JetBrains Mono** | `JetBrainsMono-Regular.ttf`, `JetBrainsMono-Bold.ttf` | 268 + 271 KB | Copyright 2020 The JetBrains Mono Project Authors (<https://github.com/JetBrains/JetBrainsMono>) |
| **Cascadia Mono** | `CascadiaMono-Regular.ttf`, `CascadiaMono-Bold.ttf` | 562 + 568 KB | Copyright (c) 2019 - Present, Microsoft Corporation, **with Reserved Font Name Cascadia Code** (<https://github.com/microsoft/cascadia-code>) |
| **Sarasa Mono TC** (更紗黑體) | `SarasaMonoTC-Regular.ttf` | 13.5 MB | Copyright (c) 2015-2025, Renzhi Li (aka. Belleve Invis, belleve@typeof.net). Portions Copyright (c) 2016 The Inter Project Authors. Portions Copyright (c) 2014-2021 Adobe Systems Incorporated, **with Reserved Font Name 'Source'**. Portions Copyright (c) 2012 Google Inc. (<https://github.com/be5invis/Sarasa-Gothic>) |

**The files are redistributed byte-for-byte, unmodified and unrenamed.** We do not modify,
subset or re-encode them, so the Reserved Font Name clauses of the OFL are not triggered.
Only the Regular weight of Sarasa Mono TC is bundled (Bold would add another 13.3 MB);
the webview synthesises bold, and the real Bold can be fetched with "Download more CJK
monospaced fonts…".

---

## 14. Downloadable fonts — SIL Open Font License 1.1

These are **not** shipped with AwayTerminal. Settings → Font → "Download more CJK
monospaced fonts…" fetches them, only when the user asks, over HTTPS, from the URLs
hard-coded in `src-tauri/src/fontstore.rs` (`CATALOG`), into the user's own config folder.
Nothing is installed system-wide.

| Font | Source | Size | Copyright |
|---|---|---|---|
| **Cascadia Next TC** | `github.com/microsoft/cascadia-code` release `cascadia-next` | 6.5 MB | Copyright (c) 2019 - Present, Microsoft Corporation, with Reserved Font Name Cascadia Code |
| **LXGW WenKai Mono TC** (霞鶩文楷) | `github.com/lxgw/LxgwWenKaiTC` release v1.522 | 14.6 MB | Copyright 2022-2026 The LXGW WenKai Project Authors (<https://github.com/lxgw/LxgwWenkaiTC>) |
| **Noto Sans Mono CJK TC** | `github.com/notofonts/noto-cjk` tag `Sans2.004` | 15.6 MB | Copyright 2014-2021 Adobe (<http://www.adobe.com/>), with Reserved Font Name 'Source'. Distributed by Google as part of Noto CJK |

Downloaded files are written verbatim; AwayTerminal never modifies or renames them.

---

## 15. SIL Open Font License, Version 1.1 (full text)

This is the licence referred to by sections 13 and 14 above. Its body is identical for every
font listed there; only the copyright line at the top differs, and those are given per font
in the tables above.

```
-----------------------------------------------------------
SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007
-----------------------------------------------------------

PREAMBLE
The goals of the Open Font License (OFL) are to stimulate worldwide
development of collaborative font projects, to support the font creation
efforts of academic and linguistic communities, and to provide a free and
open framework in which fonts may be shared and improved in partnership
with others.

The OFL allows the licensed fonts to be used, studied, modified and
redistributed freely as long as they are not sold by themselves. The
fonts, including any derivative works, can be bundled, embedded, 
redistributed and/or sold with any software provided that any reserved
names are not used by derivative works. The fonts and derivatives,
however, cannot be released under any other type of license. The
requirement for fonts to remain under this license does not apply
to any document created using the fonts or their derivatives.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this license and clearly marked as such. This may
include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the
copyright statement(s).

"Original Version" refers to the collection of Font Software components as
distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting -- in part or in whole -- any of the components of the
Original Version, by changing formats or by porting the Font Software to a
new environment.

"Author" refers to any designer, engineer, programmer, technical
writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS
Permission is hereby granted, free of charge, to any person obtaining
a copy of the Font Software, to use, study, copy, merge, embed, modify,
redistribute, and sell modified and unmodified copies of the Font
Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components,
in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled,
redistributed and/or sold with any software, provided that each copy
contains the above copyright notice and this license. These can be
included either as stand-alone text files, human-readable headers or
in the appropriate machine-readable metadata fields within text or
binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font
Name(s) unless explicit written permission is granted by the corresponding
Copyright Holder. This restriction only applies to the primary font name as
presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font
Software shall not be used to promote, endorse or advertise any
Modified Version, except to acknowledge the contribution(s) of the
Copyright Holder(s) and the Author(s) or with their explicit written
permission.

5) The Font Software, modified or unmodified, in part or in whole,
must be distributed entirely under this license, and must not be
distributed under any other license. The requirement for fonts to
remain under this license does not apply to any document created
using the Font Software.

TERMINATION
This license becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM
OTHER DEALINGS IN THE FONT SOFTWARE.
```

---

## 16. Rust crates

Linked as dependencies, each under MIT or MIT/Apache-2.0:
`windows-sys`, `libloading`, `serde`, `serde_json`, `tokio`, `chrono`,
`tauri-plugin-dialog`, `tauri-plugin-opener`, `md5`, `sys-locale`, `ttf-parser`.
Full per-crate licence text is reproduced by `cargo about` / `cargo license` output.

---

## 17. Complete licence inventory (verified with `cargo deny`)

`cd src-tauri && cargo deny check` (config: `src-tauri/deny.toml`) enforces an
**allow-list** of licences across four targets (Windows msvc, linux-gnu,
aarch64/x86_64 apple-darwin). `cargo deny list` produces the full crate-by-crate
inventory. The counts below are from **2026-09-27**; re-run before every release
(it is in the `docs/RELEASE.md` checklist).

| Licence | Crates | Obligation when we redistribute |
|---|---|---|
| MIT | ~496 | Reproduce the notice and licence text (sections 1–12 cover the ones we use directly) |
| Apache-2.0 | ~372 | Reproduce the notice, licence text and any `NOTICE` file (see section 4, russh) |
| Unicode-3.0 | 19 | Reproduce the Unicode licence notice — see below |
| Zlib | 12 | Keep the notice; do not misrepresent origin |
| BSD-3-Clause | 8 | Reproduce copyright, conditions, disclaimer (sections 6, 8) |
| ISC | 7 | Reproduce the notice (`ring`, `rustls`, `untrusted` — section 9) |
| MPL-2.0 | 6 | File-level copyleft; unmodified linking is fine, ship the licence text (section 5) |
| Unlicense | 6 | Public-domain dedication — no obligation |
| Zlib/0BSD | 1 | 0BSD is a public-domain-equivalent; no attribution required |
| BSD-2-Clause | 1 | Reproduce copyright, conditions, disclaimer |
| MIT-0 | 1 | MIT without the attribution requirement |
| CC0-1.0 | 1 | Public-domain dedication — no obligation |
| CDLA-Permissive-2.0 | 1 | Permissive data licence; keep the notice |

**No GPL / LGPL / AGPL code is linked.** Two crates offer a copyleft option in a
dual-licence expression, and we take the permissive one — recorded explicitly in
`deny.toml` so that a future version adding another copyleft option cannot slip through:

| Crate | Expression | We choose |
|---|---|---|
| `unescaper` | `MIT OR GPL-3.0-only` | **MIT** |
| `r-efi` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | **MIT** |

### Zlib licence (12 crates, e.g. `objc2-*`, `miniz_oxide`, `bytemuck`)

```
This software is provided 'as-is', without any express or implied warranty. In no
event will the authors be held liable for any damages arising from the use of this
software.

Permission is granted to anyone to use this software for any purpose, including
commercial applications, and to alter it and redistribute it freely, subject to the
following restrictions:

1. The origin of this software must not be misrepresented; you must not claim that you
   wrote the original software. If you use this software in a product, an
   acknowledgment in the product documentation would be appreciated but is not
   required.
2. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.
3. This notice may not be removed or altered from any source distribution.
```

### BSD-2-Clause (1 crate)

```
Redistribution and use in source and binary forms, with or without modification, are
permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of
   conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list
   of conditions and the following disclaimer in the documentation and/or other
   materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY
EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL
THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### Unicode licence v3 (19 crates: `icu_*`, `unicode-ident`, `zerovec`, …)

```
Copyright © 1991-2024 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY DOWNLOADING,
INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR SOFTWARE, YOU
UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE TERMS AND CONDITIONS OF
THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE
THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a copy of data
files and any associated documentation (the "Data Files") or software and any
associated documentation (the "Software") to deal in the Data Files or Software
without restriction, including without limitation the rights to use, copy, modify,
merge, publish, distribute, and/or sell copies of the Data Files or Software, and to
permit persons to whom the Data Files or Software are furnished to do so, provided
that either (a) this copyright and permission notice appear with all copies of the
Data Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF THIRD PARTY RIGHTS. IN NO
EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE BE LIABLE FOR ANY
CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES, OR ANY DAMAGES WHATSOEVER
RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT,
NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THE DATA FILES OR SOFTWARE.
```

### 0BSD / MIT-0 / CC0-1.0 / Unlicense / CDLA-Permissive-2.0

These impose **no attribution requirement**; they are listed for completeness.
Their full texts are at <https://spdx.org/licenses/> under the identifiers
`0BSD`, `MIT-0`, `CC0-1.0`, `Unlicense`, `CDLA-Permissive-2.0`.

### Known security advisory we ship with

`RUSTSEC-2023-0071` (Marvin Attack — RSA private-key operations in the `rsa` crate are
not constant-time) reaches us through `russh`. **There is no patched release**: the
advisory states that both rsa 0.9.10 and 0.10.0-rc.18 are still affected. The decision,
our exposure, and the workaround (**use Ed25519 keys**) are documented in
`src-tauri/deny.toml` next to the `ignore` entry, and in `docs/SSH.md`. Re-check every
release.

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
- ~~The full licence texts of every crate in the dependency tree~~ — **done in
  section 13** (TASK-023): the inventory is now verified with `cargo deny` against an
  allow-list across four targets, and the licence families that were missing from this
  file (Zlib, BSD-2-Clause, Unicode-3.0, 0BSD, MIT-0, CC0-1.0, Unlicense,
  CDLA-Permissive-2.0) have been added. The installers already ship this file
  (verified with `7z l` / `msiexec /a`; see `docs/RELEASE.md`).

（TeraTerm and serialport-rs used to be listed here; they now have their own sections
above — 6 and 5 — because the code that uses them has landed.）
