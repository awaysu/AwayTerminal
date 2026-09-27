#!/usr/bin/env bash
# 「用 AwayTerminal 開啟」——Nautilus（GNOME）／Dolphin（KDE）的腳本。
#
# 對應 Windows 的檔案總管右鍵選單（`src-tauri/src/shellmenu.rs` 寫的那兩個 HKCU key）。
# Linux 沒有統一的做法，所以是一個腳本 ＋ 各桌面自己的安裝位置。
#
# 安裝（Nautilus）：
#   mkdir -p ~/.local/share/nautilus/scripts
#   cp awayterminal-open-here.sh ~/.local/share/nautilus/scripts/"用 AwayTerminal 開啟"
#   chmod +x ~/.local/share/nautilus/scripts/"用 AwayTerminal 開啟"
#   # 檔名就是選單上顯示的文字。Nautilus 會在「腳本」子選單裡列出來。
#
# 安裝（Dolphin，做成 ServiceMenu 會更像原生右鍵項）：
#   mkdir -p ~/.local/share/kio/servicemenus
#   cp awayterminal-open-here.desktop ~/.local/share/kio/servicemenus/
#   chmod +x ~/.local/share/kio/servicemenus/awayterminal-open-here.desktop
#
# 移除就是把檔案刪掉。**這個腳本自己不寫任何設定**，和 Windows 那邊一樣
# （安裝檔不寫、只有使用者自己裝）。

set -euo pipefail

# Nautilus 會用這個環境變數給「目前資料夾」；沒有的話用第一個參數，再不然用 $PWD。
dir="${NAUTILUS_SCRIPT_CURRENT_URI:-}"
if [[ "$dir" == file://* ]]; then
  # URI 解碼（%20 之類）
  dir=$(printf '%b' "${dir#file://}" | sed 's/%\([0-9A-Fa-f]\{2\}\)/\x\1/g')
  dir=$(printf '%b' "$dir")
elif [[ $# -gt 0 && -d "$1" ]]; then
  dir="$1"
else
  dir="$PWD"
fi

# 找 exe：PATH → AppImage 常見位置 → /opt
exe=""
for cand in awayterminal AwayTerminal "$HOME/.local/bin/awayterminal" \
            "$HOME/Applications/AwayTerminal.AppImage" /opt/awayterminal/awayterminal; do
  if command -v "$cand" >/dev/null 2>&1; then exe="$cand"; break; fi
  if [[ -x "$cand" ]]; then exe="$cand"; break; fi
done

if [[ -z "$exe" ]]; then
  # 沒有圖形化的錯誤視窗可用時就寫 stderr（Nautilus 會丟掉，但 journalctl 看得到）
  command -v notify-send >/dev/null 2>&1 && \
    notify-send "AwayTerminal" "找不到 AwayTerminal 執行檔" || true
  echo "找不到 AwayTerminal 執行檔" >&2
  exit 1
fi

# 參數名和 Windows 那邊一樣：`--open-dir`（單一執行個體會把它交給已經在跑的視窗）
exec "$exe" --open-dir "$dir"
