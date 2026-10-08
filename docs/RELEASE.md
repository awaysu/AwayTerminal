# 發佈流程（Windows／macOS）

階段 5。第 1～6 節是 Windows；**macOS 在第 7 節**（2026-10-05 第一次實際跑過，2.0.8）。
Linux（只發 .deb ＋ .rpm）的打包與上傳目前記在 `CLAUDE.md` 的「慣例」，還沒有獨立一節。

**這份文件裡沒有任何金鑰。** 程式碼簽章憑證與 updater 的私鑰都由使用者自己保管，
`repo` 裡一個都不會有。

---

## 0. 三份版本號要一起改

| 檔案 | 欄位 |
|---|---|
| `src-tauri/Cargo.toml` | `version` |
| `src-tauri/tauri.conf.json` | `version` |
| `package.json` | `version` |

漏改一處的後果是安靜的：`npm run tauri build` 照樣過，但安裝檔的版本和程式自己報的
版本不一樣 → 更新檢查會一直說有新版（或一直說已是最新）。
`cargo test --lib version_tests` 會抓到。

---

## 1. 建置

```powershell
npm ci
npm run tauri build
```

產出：

| 檔案 | 什麼 |
|---|---|
| `src-tauri/target/release/AwayTerminal.exe` | 主程式 |
| `src-tauri/target/release/bundle/nsis/AwayTerminal_<版本>_x64-setup.exe` | NSIS 安裝檔（主要發佈用） |
| `src-tauri/target/release/bundle/msi/AwayTerminal_<版本>_x64_<語言>.msi` | MSI（企業派送用；`en-US` 與 `zh-TW` 各一個） |

### 安裝檔內容該有什麼

```powershell
& "C:\Program Files\7-Zip\7z.exe" l .\src-tauri\target\release\bundle\nsis\AwayTerminal_2.0.0_x64-setup.exe
```

必須看到（2026-09-27 實測）：

```
AwayTerminal.exe
THIRD-PARTY-NOTICES.md
conpty\OpenConsole.exe
conpty\conpty.dll
conpty\README.md
```

MSI 用系統管理安裝解出來看（**不會裝進系統**）：

```powershell
msiexec /a "<msi 的完整路徑>" /qn TARGETDIR="$env:TEMP\msi-check"
Get-ChildItem -Recurse "$env:TEMP\msi-check" | Select-Object FullName
```

`THIRD-PARTY-NOTICES.md` **一定要在**：`russh` 是 Apache-2.0、`serialport-rs` 是
MPL-2.0、TeraTerm 是 BSD-3，散布時必須附授權全文。`cargo test --lib version_tests`
守著設定那一端，上面兩條指令驗真的產物。

角色範本（代理團隊／AI 聊天室的 26 個 .md）與沙盒護欄腳本**不在**這份清單裡：
它們是 `include_str!` 嵌在 exe 裡的，不以檔案形式安裝。

---

## 2. 安裝檔的行為（和舊版對照）

| 項目 | 舊版 1.2.8（Inno Setup） | 這一版（Tauri NSIS） |
|---|---|---|
| 安裝範圍 | `PrivilegesRequired=admin`＋`{autopf}` ＝ Program Files | `installMode: perMachine`（同） |
| 已裝 1.x 時 | — | **安裝前先靜默移除 1.x**（`hooks.nsh` 的 `NSIS_HOOK_PREINSTALL`，找 1.x 的 Inno AppId 解除安裝項目，跑 `unins000.exe /VERYSILENT`，等它刪完最多 60 秒），再裝進同一個 `Program FilesAwayTerminal`。設定在 `%LOCALAPPDATA%`，不會被刪，2.0 第一次啟動可匯入（2026-10-01 使用者定案） |
| 開始功能表捷徑 | 一律建立 | 同 |
| 桌面捷徑 | `[Tasks] desktopicon`，**預設不勾** | 完成頁的核取方塊，**預設不勾**（Tauri 樣板本來就這樣） |
| 授權頁 | 沒有 | **有**（`bundle.licenseFile` → 本專案的 MIT 全文） |
| 安裝檔語言 | 只有英文 | **八種**（對應介面八語）＋安裝前的語言選擇 |
| WebView2 | 內含 bootstrapper，缺少時才靜默安裝 | `webviewInstallMode: embedBootstrapper`（同樣的做法） |
| .NET 執行環境 | 內含 .NET 9 Desktop Runtime 安裝程式 | **不需要**（Rust 原生，這是換 Tauri 的收穫之一） |
| 升級保留設定 | 設定在 `%LOCALAPPDATA%`、log 在「我的文件」，不在安裝目錄 | 同（設定在 `%APPDATA%\com.awaysu.awayterminal`） |
| 「用 AwayTerminal 開啟」右鍵選單 | 安裝檔**不寫**（程式啟動時自己註冊 HKCU）；解除安裝時用 `reg delete` 清掉 | 安裝檔**不寫**（v2 改成設定視窗裡的勾選，`shellmenu.rs`）；解除安裝時由 `src-tauri/installer/hooks.nsh` 的 `NSIS_HOOK_POSTUNINSTALL` 清掉 |
| 憑證 | **刻意不動系統憑證存放區**（見下） | 同（我們根本不做這件事） |

### 為什麼安裝檔不寫右鍵選單

1. 會和設定視窗那個勾選打對台（使用者取消勾選，下次升級又被寫回來）。
2. `HKCU` 是「目前使用者」，但 per-machine 安裝跑在提權環境下，寫進去的可能是別的帳號。

舊版是同樣的結論。解除安裝時清掉才是必要的——不清的話選單會指向一個已經被刪掉的 exe。

⚠️ **已知限制（2026-09-30 稽核 I8）**：同樣的理由也適用在**解除安裝**。per-machine 的
解除安裝程式跑在提權環境，`hooks.nsh` 的 `DeleteRegKey HKCU …` 刪的是**提權那個帳號**的 HKCU：

- 一般使用者 + UAC 輸入**另一個**系統管理員帳密 → 刪到管理員的，使用者自己的選單**留著**，
  點下去指向已經被刪掉的 exe；
- 使用者本身就是管理員（最常見）→ 提權後還是同一個帳號，刪得到；
- 機器上**其他**勾過選單的帳號，任何做法都刪不到（每個帳號各一份 HKCU）。

沒有改成 `HKU\<SID>`：NSIS 拿「啟動解除安裝的那個非提權使用者」的 SID 要靠外掛或
`System::Call` 查 token，還要處理「那個帳號的 hive 沒載入」的情況，寫錯的代價是刪到
別人的登錄檔——在能和簽章、MSI 一起實機驗之前不值得冒險。殘留的選單要由使用者
重新安裝後在設定視窗取消勾選（`shellmenu.rs` 寫／刪的是自己帳號的 HKCU）才清得掉。

### ⬜ 待辦（階段 5）：捷徑要帶和程式一樣的 AppUserModelID

TASK-035 起，程式在啟動時（建立視窗之前）設了明確的工作列身分：

```rust
// src-tauri/src/winicon.rs
pub const APP_USER_MODEL_ID: &str = "com.awaysu.awayterminal2";
```

**但 NSIS 建立的開始功能表／桌面捷徑沒有帶同一個值。** Windows 對
「捷徑的 AUMID ≠ 執行中行程的 AUMID」的處理是**把它們當成兩個不同的程式**：

* 釘選在工作列的那一顆，和程式跑起來之後的那一顆**不會合併**（工作列上會有兩顆）；
* 跳躍清單（Jump List）掛在捷徑的身分上，執行中的視窗貢獻不進去。

要根治得在 `src-tauri/installer/hooks.nsh` 幫捷徑寫入
`System.AppUserModel.ID`（Shell 屬性 `PKEY_AppUserModel_ID`）。NSIS 本身沒有這個能力，
兩條路：

1. 用 NSIS 外掛（例如 `WinShell`／`ApplicationID`）——要把外掛 DLL 一起帶進 repo；
2. 安裝後呼叫一小段 PowerShell 改捷徑的屬性——不必帶外掛，但要多開一個行程。

**為什麼現在不做**：它只影響「釘選」的體驗，不影響圖示本身（圖示已經在 TASK-035 修好，
`ICON_BIG`／`ICON_SMALL` 都有值）；而且要動安裝檔，得和簽章、MSI 一起驗才有意義 →
留到階段 5 做安裝檔的時候一併處理。

驗收方式：安裝之後把程式釘選到工作列 → 關掉 → 從釘選的圖示啟動 →
工作列上應該只有**一顆**，不是兩顆。

---

## 3. 程式碼簽章

### 憑證從哪來

| 選項 | 費用／年 | SmartScreen |
|---|---|---|
| **OV（Organization Validation）程式碼簽章憑證** | 約 US$200～400 | 仍要累積信譽（下載量），一開始照樣會擋 |
| **EV（Extended Validation）** | 約 US$300～600，要硬體 token／HSM | **立刻**通過 SmartScreen |
| 自我簽署 | 0 | **沒有幫助**（舊版 1.0.11 用過，1.0.12 拿掉了，見下） |

2023 年 6 月起，CA 一律要求私鑰放在硬體（token／HSM／雲端簽章服務），所以不會再有
「下載一個 .pfx 檔」的流程了。

⚠️ **舊版的教訓（`installer.iss` 的註解）**：1.0.11 以前的安裝檔會靜默把自我簽署憑證
匯入「受信任的根憑證授權」，1.0.12 拿掉了——那個動作和 MITRE ATT&CK T1553.004
（Subvert Trust Controls: Install Root Certificate）無法區分，很可能就是被防毒軟體
擋下的原因，而且等於要求每個使用者信任一把私鑰放在開發機上的根憑證。
**新版不做這件事，永遠不要加回來。**

### 怎麼簽

Tauri 可以在 build 的時候自動簽（憑證已經在 Windows 憑證存放區裡）：

```jsonc
// src-tauri/tauri.conf.json → bundle.windows
"certificateThumbprint": "<憑證的 SHA1 指紋，不含空白>",
"digestAlgorithm": "sha256",
"timestampUrl": "http://timestamp.digicert.com"
```

指紋從這裡拿：

```powershell
Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert |
  Select-Object Subject, Thumbprint, NotAfter
```

用 HSM／雲端簽章服務（Azure Trusted Signing 之類）時改用 `signCommand`：

```jsonc
"signCommand": "my-signing-tool sign %1"
```

`%1` 會被換成要簽的檔案路徑。Tauri 會對主程式與兩種安裝檔各呼叫一次。

⚠️ **`Get-AuthenticodeSignature` 回 `UnknownError` 不一定是壞事**（舊版踩雷第 48 條，
2026-08-04 一度誤判）：訊息如果是「terminated in a root certificate which is not
trusted」，那代表簽章用的根憑證不在信任存放區——**自我簽署的憑證本來就會這樣**，
程式照樣能執行。別把它當成防毒攔截或簽章失敗。真的有問題的是
`HashMismatch`（檔案被改過）或 `NotSigned`。

手動簽（確認用）：

```powershell
signtool sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 /sha1 <指紋> <檔案>
signtool verify /pa /v <檔案>
```

### SmartScreen

沒有 EV 憑證的話，第一版一定會出現「Windows 已保護你的電腦」。信譽是**按憑證**累積的
（不是按檔案），所以：換憑證＝信譽歸零，每次發佈都用同一張憑證簽。
舊版若已經在累積信譽，**沿用同一張憑證**。

---

## 4. Tauri updater

現在的狀態：**設定骨架在、公鑰留空 → updater 沒有掛載，功能不存在。**
「關於 → 檢查更新」走的是 TASK-015 那條路（`update.rs` 問 `awaysu.cc` 的 `api.php`，
只告知有新版、不自動下載）。`cargo test --lib version_tests` 有一條測試守著這個狀態。

### 要開啟自動更新的步驟

1. **產生金鑰對**（使用者自己做，在自己的機器上）：

   ```powershell
   npm run tauri signer generate -- -w $HOME\.tauri\awayterminal2.key
   ```

   會產出兩個東西：
   - `awayterminal2.key` ＝**私鑰**。⚠️ **絕不進 repo、絕不進任何雲端同步資料夾。**
     弄丟＝以後沒辦法再發自動更新（所有已安裝的版本都不會接受新的簽章），
     所以要另外離線備份一份。
   - `awayterminal2.key.pub` ＝公鑰，內容貼進設定檔。

2. **公鑰貼進 `src-tauri/tauri.conf.json`**：

   ```jsonc
   "plugins": {
     "updater": {
       "pubkey": "<awayterminal2.key.pub 的內容，一整行>",
       "endpoints": ["https://github.com/awaysu/AwayTerminal/releases/latest/download/latest.json"],
       "windows": { "installMode": "passive" }
     }
   }
   ```

   貼上之後 `the_updater_is_configured_but_disabled_until_a_key_exists` 這條測試會失敗
   ——那是刻意的，改成檢查格式並在這裡記一筆「哪一版開始有自動更新」。

   **同一個 commit 裡把 `bundle.createUpdaterArtifacts` 改成 `true`**（repo 裡現在明寫著 `false`）：

   ```jsonc
   "bundle": {
     "createUpdaterArtifacts": true,
     …
   }
   ```

   ⚠️ 這是 `.sig` 會不會產生的**總開關**（2026-09-30 稽核 I1）：它是 `false` 的時候，
   就算給了私鑰也**一個 `.sig` 都不會產生**，`release.mjs` 只會一直說「沒有簽章」。
   反過來，它是 `true` 而 build 時**沒給私鑰**，`npm run tauri build` 會在最後失敗
   （`A public key has been found, but no private key…`）——所以它和公鑰**一起開**，
   在那之前保持 `false`，平常沒有私鑰的機器才 build 得起來。
   `node scripts/release.mjs` 會檢查這兩個是不是一起開／一起關。

3. **簽章時把私鑰給 build**（環境變數，不要寫進檔案）：

   ```powershell
   $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $HOME\.tauri\awayterminal2.key -Raw
   $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "<產生金鑰時設的密碼>"
   npm run tauri build
   ```

   會多產出 `AwayTerminal_<版本>_x64-setup.exe.sig`。

4. **`latest.json` 放進 GitHub Release**（檔名固定，`endpoints` 指著它）：

   ```json
   {
     "version": "2.0.1",
     "notes": "修正……",
     "pub_date": "2026-10-01T12:00:00Z",
     "platforms": {
       "windows-x86_64": {
         "signature": "<.sig 檔的內容，一整行>",
         "url": "https://github.com/awaysu/AwayTerminal/releases/download/v2.0.1/AwayTerminal_2.0.1_x64-setup.exe"
       }
     }
   }
   ```

   之後加 mac／Linux 時在 `platforms` 裡補 `darwin-aarch64`／`darwin-x86_64`／
   `linux-x86_64`。`version` **不要**帶 `v` 前綴（tag 才有）。

### `awaysu.cc` 的 `check_update` 回應格式

「關於 → 檢查更新」問的是 `https://awaysu.cc/software/api.php?action=check_update&app=awayterminal&platform=windows&version=<目前版本>`
（`update.rs`）。後端要回：

```json
{
  "has_update": true,
  "latest_version": "2.0.1",
  "download_url": "https://www.awaysu.cc/software/awayterminal"
}
```

2.0 **沿用舊版的 `awayterminal` 代號**（2026-10-01 使用者定案），上傳安裝檔與 zip 也傳到這個代號（見 §5 的上傳）。

---

## 5. 發佈檢查清單

```
[ ] 三份版本號一致（cargo test --lib version_tests）
[ ] CHANGELOG.md 有這一版的條目
[ ] cargo test --lib 全過
[ ] cargo clippy --all-targets -- -D warnings 零警告
[ ] node scripts/test-i18n.mjs      PASS（八語沒有缺漏）
[ ] node scripts/i18n-audit.mjs     PASS（沒有沒歸類的中文字串）
[ ] node scripts/test-bridge-args.mjs  PASS
[ ] node scripts/audit-pitfalls.mjs   PASS（舊版 CLAUDE.md 更新過就要重新稽核）
[ ] node scripts/test-sandbox-guard.mjs PASS
[ ] node scripts/audit-control-bytes.mjs PASS（原始碼沒有字面控制字元，稽核 A9）
[ ] node scripts/make-manual-plan.mjs --check PASS
[ ] cd src-tauri && cargo deny check      advisories/bans/licenses/sources 全 ok
[ ] npm audit --omit=dev                  0 vulnerabilities
[ ] 重看 deny.toml 的 ignore 清單          上游修好了就拿掉（目前只有 RUSTSEC-2023-0071）
[ ] THIRD-PARTY-NOTICES.md 第 17 節的授權盤點與 cargo deny list 一致
[ ] THIRD-PARTY-NOTICES.md 第 13/14 節的字型與 src-tauri/fonts/ 的檔案一致（OFL 全文在第 15 節）
[ ] 捷徑的 AppUserModelID 與 winicon.rs 的 APP_USER_MODEL_ID 一致（見 §2 的待辦；還沒做）
[ ] 各 probe：pty / ssh / telnet / com / ttl / agent / chat / sandbox / job
[ ] npm run verify                  0 FAIL
[ ] npm run tauri build             msi + nsis 都出來
[ ] release exe 的 --verify 跑過（AwayTerminal.exe --verify 1）
[ ] 7z l 看 NSIS 內容：THIRD-PARTY-NOTICES.md + conpty 三個檔 + fonts 五個字型檔都在
[ ] msiexec /a 看 MSI 內容：同上
[ ] 安裝檔有簽章（signtool verify /pa）
[ ] docs/MANUAL-TEST-PLAN.md 的 P0 一節由使用者跑過
[ ] 工作樹乾淨、HEAD 已 push（release.mjs --publish 會用 --target <HEAD> 打 tag，沒 push 就擋）
[ ] GitHub Release：tag v<版本>，資產命名見下
[ ] awaysu.cc 的 awayterminal：上傳安裝檔＋zip（api.php?action=upload，帶 version／sha256／changelog）
```

### GitHub Release 的資產命名

`npm run tauri build` 的輸出名稱直接用，不要改名（updater 的 `url` 與使用者的習慣都靠它）：

```
AwayTerminal_2.0.0_x64-setup.exe          NSIS（主要）
AwayTerminal_2.0.0_x64-setup.exe.sig      有開自動更新時才有
AwayTerminal_2.0.0_x64_en-US.msi          MSI
AwayTerminal_2.0.0_x64_zh-TW.msi
latest.json                               有開自動更新時才有（檔名固定）
```

---

## 6. 還需要使用者提供的東西

| 要什麼 | 給誰 | 沒有的後果 |
|---|---|---|
| 程式碼簽章憑證（沿用舊版那張，如果有） | `certificateThumbprint` 或 `signCommand` | SmartScreen 會擋，而且信譽從零開始 |
| updater 金鑰對（使用者自己產生、自己保管） | `pubkey` ＋ build 時的環境變數 | 沒有自動更新（現在就是這個狀態） |
| macOS 機器 ＋ Apple Developer ID（年費已付） | 簽章 ＋ notarization ＋ .dmg | ✅ 已有（2026-10-05，見第 7 節） |
| Linux 機器（Ubuntu 22.04／24.04） | .deb ＋ .rpm（不發 AppImage，見 `CLAUDE.md`「慣例」） | Linux 版做不了 |

---

## 7. macOS

2026-10-05 在真機（Apple Silicon、macOS 26.6）跑過一次，產出 `AwayTerminal-2.0.8-macOS.dmg`
並上架到 `awaysu.cc`。**這一節裡沒有任何密碼**：簽章憑證在登入鑰匙圈，公證帳密在
鑰匙圈的 profile，網站密碼在使用者自己的檔案裡。

### 7.1 一次性的前置

| 要什麼 | 怎麼確認／怎麼做 |
|---|---|
| Developer ID 憑證在**登入**鑰匙圈 | `security find-identity -v -p codesigning` 要看到 `Developer ID Application: Chih-Wei Su (BNH8YS88T9)` |
| 公證帳密的 profile | `xcrun notarytool store-credentials AwayTerminalNotary --apple-id <Apple ID> --team-id BNH8YS88T9`（互動式問 App 專用密碼，在 appleid.apple.com 產生）。確認：`xcrun notarytool history --keychain-profile AwayTerminalNotary` |
| Intel 的編譯目標（universal 要） | `rustup target add x86_64-apple-darwin` |

⚠️ 這台機器上另外兩個鑰匙圈（`awpr-signing`、`ios-signing`）是**鎖著的**，
`security`／`notarytool` 一碰到它們就會停在螢幕上的解鎖對話框、指令不會回來。
profile 一律存在登入鑰匙圈、指令不要加 `--keychain` 指過去。

### 7.2 建置（universal、已簽章）

```sh
export APPLE_SIGNING_IDENTITY="Developer ID Application: Chih-Wei Su (BNH8YS88T9)"
npm run tauri build -- --target universal-apple-darwin --bundles app
```

產出 `src-tauri/target/universal-apple-darwin/release/bundle/macos/AwayTerminal.app`
（arm64 ＋ x86_64，hardened runtime，帶時間戳）。

- 簽章身分用**環境變數**給，不寫進設定檔——沒有憑證的機器才 build 得起來。
- `src-tauri/tauri.macos.conf.json` 把 `resources/conpty/*` 設成 `null`：
  那是 Windows 的 ConPTY（`OpenConsole.exe`／`conpty.dll`），mac 版不帶。
- 只做 `app`、**不讓 Tauri 做 dmg**：Tauri 的公證只吃 `APPLE_ID`＋`APPLE_PASSWORD`
  或 API key 的環境變數，不吃鑰匙圈 profile；而且它做的 dmg 裡面是**還沒 staple** 的 app。
  build 最後那行 `skipping app notarization` 是預期的。

確認：

```sh
APP=src-tauri/target/universal-apple-darwin/release/bundle/macos/AwayTerminal.app
lipo -archs $APP/Contents/MacOS/AwayTerminal          # x86_64 arm64
codesign --verify --deep --strict --verbose=2 $APP     # valid on disk
ls $APP/Contents/Resources                             # 有 fonts／THIRD-PARTY-NOTICES.md，沒有 conpty
$APP/Contents/MacOS/AwayTerminal --verify 1            # 0 FAIL（跑完視窗不會自己關，依 PID 收）
```

### 7.3 公證 → staple → DMG → DMG 也公證

順序不能換：**先公證並 staple `.app`，再把它包進 DMG**，使用者把 app 拖出來之後
離線也驗得過。

```sh
B=src-tauri/target/universal-apple-darwin/release/bundle
ID="Developer ID Application: Chih-Wei Su (BNH8YS88T9)"
V=2.0.8

# 1) .app：用 ditto 壓（不要用 zip 指令，會掉 metadata）→ 公證 → staple
ditto -c -k --keepParent $B/macos/AwayTerminal.app $B/AwayTerminal-notarize.zip
xcrun notarytool submit $B/AwayTerminal-notarize.zip --keychain-profile AwayTerminalNotary --wait
xcrun stapler staple $B/macos/AwayTerminal.app

# 2) DMG：app ＋「應用程式」捷徑
STG=$(mktemp -d)
ditto $B/macos/AwayTerminal.app "$STG/AwayTerminal.app"
ln -s /Applications "$STG/Applications"
hdiutil create -volname AwayTerminal -srcfolder "$STG" -ov -format UDZO $B/AwayTerminal-$V-macOS.dmg

# 3) DMG：簽 → 公證 → staple
codesign --force --timestamp --sign "$ID" $B/AwayTerminal-$V-macOS.dmg
xcrun notarytool submit $B/AwayTerminal-$V-macOS.dmg --keychain-profile AwayTerminalNotary --wait
xcrun stapler staple $B/AwayTerminal-$V-macOS.dmg
```

兩次 `submit` 都要回 `status: Accepted`（2026-10-05 各等一兩分鐘）。被退件時：
`xcrun notarytool log <submission-id> --keychain-profile AwayTerminalNotary`。

### 7.4 驗收（模擬使用者下載）

本機做出來的檔案沒有 quarantine 標記，Gatekeeper 不會完整檢查，要自己補上：

```sh
cp $B/AwayTerminal-$V-macOS.dmg /tmp/dl-test.dmg
xattr -w com.apple.quarantine "0083;$(printf '%08x' $(date +%s));Safari;$(uuidgen)" /tmp/dl-test.dmg
spctl -a -vvv -t install /tmp/dl-test.dmg      # accepted / source=Notarized Developer ID
```

掛載之後對裡面的 app 再做一次 `spctl --assess --type execute --verbose=4`，
同樣要是 `Notarized Developer ID`。

### 7.5 上架到 awaysu.cc

SHA-256 **全部簽完、公證完才算**（簽章與 staple 都會改檔案內容）。

```sh
SHA=$(shasum -a 256 $B/AwayTerminal-$V-macOS.dmg | cut -d' ' -f1)
curl -H "X-Api-Password: $AWAYSU_API_PASSWORD" \
  -F app=awayterminal -F version=$V -F platform=macos -F sha256=$SHA \
  -F "file=@$B/AwayTerminal-$V-macOS.dmg" \
  "https://www.awaysu.cc/software/api.php?action=upload"
```

- `platform=macos` ＋ 副檔名 `dmg` 的既有項目會被取代（舊檔自動保留為「舊版本」）；沒有就新增。
- `changelog` 欄位：同一版在網站上已經有紀錄時會被跳過，所以 Windows 先發過的版本不用再帶。
- 上傳完從 `download.url` 抓回來比對一次 SHA-256，並確認
  `api.php?action=check_update&app=awayterminal&platform=macos` 列得到它
  （mac 版的「檢查更新」問的就是這一條，`update.rs`）。

### 7.6 還沒做的

| 項目 | 現況 |
|---|---|
| 一支腳本跑完 7.2～7.5 | 沒有，2026-10-05 是手動逐步跑的 |
| `scripts/release.mjs` | 只認 Windows 的資產（nsis／msi），不知道 dmg |
| GitHub Release 放 dmg、`latest.json` 的 `darwin-*` | 沒有（updater 本來就還沒開，見第 4 節） |
| `Info.plist` 的用途說明字串（`NS…UsageDescription`） | 沒加。終端機裡的程式要用麥克風／Apple Events 之類的權限時可能需要，等真的遇到再補 |
| DMG 的背景圖與圖示排版 | 用 `hdiutil` 的預設樣子 |

