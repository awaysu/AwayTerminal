# Third-Party Notices

AwayTerminal is licensed under the MIT License (see [LICENSE](./LICENSE)).

It bundles and redistributes the third-party components listed below, each under
its own licence. The notices reproduced here are provided to satisfy those
licences' attribution requirements. Nothing in this file changes the licence of
AwayTerminal's own source code.

None of the bundled components are licensed under the GPL, LGPL, or any other
copyleft licence.

---

## 1. xterm.js and addons

**Used for:** terminal rendering inside the embedded WebView2 control.
**Shipped as:** `web/vendor/xterm.js`, `web/vendor/xterm.css`,
`web/vendor/addon-fit.js`, `web/vendor/addon-serialize.js`,
`web/vendor/addon-unicode11.js`, `web/vendor/addon-web-links.js`
**Project:** https://github.com/xtermjs/xterm.js
**Licence:** MIT

> Copyright (c) 2017, The xterm.js authors (https://github.com/xtermjs/xterm.js)
> Copyright (c) 2014, The xterm.js authors. All rights reserved.
> Copyright (c) 2012-2013, Christopher Jeffrey (https://github.com/chjj/term.js)
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in
> all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
> THE SOFTWARE.

xterm.js was originally forked, with the author's permission, from Fabrice
Bellard's JavaScript vt100 terminal for jslinux
(http://bellard.org/jslinux/, Copyright (c) 2011 Fabrice Bellard).

> **Note:** the bundled `.js` files are minified builds from which the licence
> banner was stripped by the bundler. This file supplies the notice required by
> the MIT licence on their behalf. The unminified banner is preserved in
> `web/vendor/xterm.css`.

---

## 2. .NET Runtime and libraries

**Used for:** the application runtime. AwayTerminal is published self-contained,
so the .NET runtime is redistributed inside the installer and the MSIX package.
**Includes:** .NET Runtime 9.0.x, `System.IO.Ports` 10.0.x (serial port support)
**Project:** https://github.com/dotnet/runtime
**Licence:** MIT — Copyright (c) .NET Foundation and Contributors

> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in
> all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
> THE SOFTWARE.

---

## 3. Microsoft Edge WebView2

**Used for:** hosting the terminal UI (xterm.js) inside the WPF shell.
**Shipped as:** the `Microsoft.Web.WebView2` SDK assemblies and, in the Inno
Setup installer, the Evergreen Runtime bootstrapper
(`MicrosoftEdgeWebview2Setup.exe`), which is run only when the WebView2 runtime
is not already present on the machine.
**Project:** https://developer.microsoft.com/microsoft-edge/webview2/
**Licence:** Microsoft Software Licence Terms for the Microsoft Edge WebView2
SDK and Runtime. Not an open source licence; redistribution of the SDK and of
the Evergreen bootstrapper is permitted under those terms.

> Copyright (c) Microsoft Corporation. All rights reserved.

---

## 4. Android SDK Platform Tools (adb) — NOT redistributed

AwayTerminal's ADB connection type runs `adb.exe`, but **no Android SDK
Platform Tools binaries are bundled or redistributed with AwayTerminal.**

Up to and including version 1.0.12, Google's prebuilt `adb.exe`,
`AdbWinApi.dll`, `AdbWinUsbApi.dll` and `libwinpthread-1.dll` were shipped in
`tools/adb/`. They were removed in 1.0.13: the Android SDK Terms of Service
§3.4 prohibit redistributing the SDK, and the boundary against §3.5
(separately-licensed open source components) is not clear enough to rely on
when distributing through an app store.

AwayTerminal now locates an existing `adb.exe` already installed on the user's
machine — searching `PATH`, `ANDROID_HOME` / `ANDROID_SDK_ROOT`, and the default
Android Studio SDK location — and, if none is found, points the user at Google's
official download page. Users obtain the Platform Tools directly from Google
under Google's own terms.

---

## 5. Inno Setup

**Used for:** building the Windows installer. Portions of the Inno Setup runtime
are embedded in the generated `AwayTerminal-Setup-*.exe`.
**Project:** https://jrsoftware.org/isinfo.php
**Licence:** Inno Setup License — Copyright (c) 1997-2025 Jordan Russell,
portions Copyright (c) 2000-2025 Martijn Laan. All rights reserved.

---

## Icons

All icon artwork in `icon/` — including the icons that denote third-party
connection types — is original work created for AwayTerminal and is covered by
the project's MIT licence. No third-party logo files, icon packs or stock
artwork are redistributed, so no icon attribution is owed to anyone else.

## Trademarks

Product names, logos and brands appearing in AwayTerminal's user interface —
including but not limited to Microsoft, Windows, PowerShell, WSL, Docker, Git,
Python, Android, ADB and Claude — are the property of their respective owners.
They are used solely to identify the corresponding connection type or program,
and their use does not imply any affiliation with, sponsorship by, or
endorsement from those owners.
