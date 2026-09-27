# 發佈流程（Windows）

階段 5。mac／Linux 等使用者提供機器之後再補（`CLAUDE.md` 的平台順序）。

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
       "endpoints": ["https://github.com/awaysu/AwayTerminal2/releases/latest/download/latest.json"],
       "windows": { "installMode": "passive" }
     }
   }
   ```

   貼上之後 `the_updater_is_configured_but_disabled_until_a_key_exists` 這條測試會失敗
   ——那是刻意的，改成檢查格式並在這裡記一筆「哪一版開始有自動更新」。

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
         "url": "https://github.com/awaysu/AwayTerminal2/releases/download/v2.0.1/AwayTerminal_2.0.1_x64-setup.exe"
       }
     }
   }
   ```

   之後加 mac／Linux 時在 `platforms` 裡補 `darwin-aarch64`／`darwin-x86_64`／
   `linux-x86_64`。`version` **不要**帶 `v` 前綴（tag 才有）。

### `awaysu.cc` 的 `check_update` 回應格式

「關於 → 檢查更新」問的是 `https://awaysu.cc/software/api.php?action=check_update&app=awayterminal2&version=<目前版本>`
（`update.rs`）。後端要回：

```json
{
  "has_update": true,
  "latest_version": "2.0.1",
  "download_url": "https://awaysu.cc/software/awayterminal2/"
}
```

⚠️ **`awayterminal2` 這個代號還沒在 awaysu.cc 後台建立**（TASK-015 就回報過）。
沒建立的話檢查更新永遠顯示「檢查失敗」——失敗是靜默的，不會跳錯誤視窗。

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
[ ] cd src-tauri && cargo deny check      advisories/bans/licenses/sources 全 ok
[ ] npm audit --omit=dev                  0 vulnerabilities
[ ] 重看 deny.toml 的 ignore 清單          上游修好了就拿掉（目前只有 RUSTSEC-2023-0071）
[ ] THIRD-PARTY-NOTICES.md 第 13 節的授權盤點與 cargo deny list 一致
[ ] 各 probe：pty / ssh / telnet / com / ttl / agent / chat / sandbox / job
[ ] npm run verify                  0 FAIL
[ ] npm run tauri build             msi + nsis 都出來
[ ] release exe 的 --verify 跑過（AwayTerminal.exe --verify 1）
[ ] 7z l 看 NSIS 內容：THIRD-PARTY-NOTICES.md + conpty 三個檔都在
[ ] msiexec /a 看 MSI 內容：同上
[ ] 安裝檔有簽章（signtool verify /pa）
[ ] docs/MANUAL-TEST-PLAN.md 的 P0 一節由使用者跑過
[ ] GitHub Release：tag v<版本>，資產命名見下
[ ] awaysu.cc 後台的 awayterminal2 版本號更新
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
| `awaysu.cc` 後台建立 `awayterminal2` 代號 ＋ 下載頁 URL | `update.rs` 的檢查更新 | 檢查更新永遠「失敗」 |
| macOS 機器 ＋ Apple Developer ID（年費已付） | 簽章 ＋ notarization ＋ .dmg | mac 版做不了 |
| Linux 機器（Ubuntu 22.04／24.04） | AppImage ＋ .deb | Linux 版做不了 |
