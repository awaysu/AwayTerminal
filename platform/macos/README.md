# macOS 的「用 AwayTerminal 開啟」（Finder Quick Action）

對應 Windows 的檔案總管右鍵選單（`src-tauri/src/shellmenu.rs` 寫的那兩個 HKCU key）
與 Linux 的 `platform/linux/awayterminal-open-here.sh`。

macOS 沒辦法用一個檔案就裝好——Quick Action 是 **Automator 的 workflow**（一個資料夾
形式的 bundle，裡面有 plist 與編譯過的 action），用文字檔沒辦法完整表達。所以這裡給
**建立步驟**與要貼的腳本，由使用者自己在 Automator 裡按幾下（一次就好）。

⚠️ 這份步驟**還沒在真的 mac 上跑過**（開發機是 Windows）。待驗證項目見
`docs/PLATFORM-UNIX.md`。

---

## 建立（一次就好）

1. 開 **Automator** → 新增文件 → 選 **快速動作**（Quick Action）。
2. 上方三個下拉設成：
   - 工作流程收到 **檔案夾**（folders）
   - 位於 **Finder**
   - 圖像：終端機（隨意）
3. 左邊搜尋 **執行 Shell 指令碼**（Run Shell Script），拖到右邊。
4. 那個動作的設定：
   - Shell：`/bin/zsh`
   - 傳遞輸入：**作為引數**（as arguments）
5. 把下面這段貼進去：

```zsh
#!/bin/zsh
# 參數名和 Windows／Linux 一樣：--open-dir
# 單一執行個體（tauri-plugin-single-instance）會把路徑交給已經在跑的視窗。
APP="/Applications/AwayTerminal.app/Contents/MacOS/AwayTerminal"
for d in "$@"; do
  if [[ -x "$APP" ]]; then
    "$APP" --open-dir "$d"
  else
    # 沒裝在 /Applications 的話交給 open(1) 找（-n 會開新實例，所以只在找不到 exe 時用）
    open -a AwayTerminal --args --open-dir "$d"
  fi
done
```

6. 存檔，名稱打 **用 AwayTerminal 開啟**（這就是右鍵選單上顯示的文字）。

之後在 Finder 對任何資料夾按右鍵 → **快速動作** → 用 AwayTerminal 開啟。

## 移除

系統設定 → 延伸功能 → Finder，把它取消勾選；或把
`~/Library/Services/用 AwayTerminal 開啟.workflow` 刪掉。

## 為什麼不做成安裝檔的一部分

和 Windows 那邊同一個理由（`src-tauri/installer/hooks.nsh` 的註解）：
安裝時自動寫會和設定視窗的勾選打對台，而且 Quick Action 是**逐使用者**的東西，
安裝程式（可能提權）不該替使用者決定。設定視窗在 mac／Linux 上會把
「檔案總管右鍵選單」那一項**隱藏**，並指向這份文件。

## `open -a` 與 `--args` 的坑（待真機驗證）

- `open -a AwayTerminal --args …` 只在 app **還沒在跑**的時候會把參數傳進去；
  已經在跑時參數會被丟掉。所以上面優先直接執行 exe。
- 直接執行 `Contents/MacOS/AwayTerminal` 會多一個 dock 圖示嗎？
  單一執行個體 plugin 應該讓第二個實例立刻結束，但這一點**要在真機確認**。
