// AwayTerminal 前端：單一 WebView2 內管理多個 xterm。
// 兩種模式：tab（只顯示 active）/ split（grid 全部顯示，可拖曳 pane 標題重排）。
//
// 訊息協定（字串；US = \x1f）：
//   JS -> C# :  i{id}US{text} 輸入、r{id}US{cols},{rows} 尺寸、
//               a{id}US{kind}US{text} 查詢回覆、p{id} 選取某 pane、
//               k{id1},{id2},... 拖曳後的新順序、z{size} Ctrl+滾輪縮放後的字級、ready
//   C# -> JS :  o{id}US{base64} 輸出、n{id}US{title}[US{flags}] 建立（flags 含 c=claude 貼上）、t{id}US{title} 改名、
//               s{id} 選取、x{id} 關閉、c{id} 清畫面、L{tab|split} 切換模式、
//               S{id}US{up|down|top|bottom} 捲動檢視（工具列「翻頁」；不送輸入）、
//               q{id}US{sel|selpaste|all|text|file|cwd}（selpaste 與 sel 同樣回選取文字，C# 端多做一次貼回）、
//               T{json} 套用字型顏色、P{id}US{fg}US{bg} 單一分頁配色（空=回設定預設）、
//               A{id} 全選、F 開搜尋列、v{id}US{base64} 貼上（走 xterm.paste，支援 bracketed paste）
(function () {
  "use strict";
  var ws = window.chrome.webview;
  var US = "\x1f";
  var terms = {};       // id -> {term, fit, ser, el, body, titleSpan, title}
  var active = null;
  var mode = "tab";
  var dragId = null;
  var zoomed = null;        // 分割模式：點標題放大成整頁的 pane id
  var suppressClick = false; // 拖曳結束後抑制隨之而來的 click
  var container = document.getElementById("terminals");

  var cfg = {
    fontFamily: '"Cascadia Mono", Consolas, "Microsoft JhengHei", "微軟正黑體", monospace',
    fontSize: 14, foreground: "#e0e0e0", background: "#1e1e1e"
  };
  // IME 診斷開關（追注音輸入問題用；D 協定 → C# Diag → diag.log。平時關閉）
  var IMEDBG = false;
  function dbgLog(id, s) {
    try { ws.postMessage("D[" + id + " " + (Math.round(performance.now()) % 1000000) + "] " + s); } catch (_) {}
  }
  function themeOf() {
    return { background: cfg.background, foreground: cfg.foreground, cursor: "#ffffff", selectionBackground: "#264f78" };
  }
  // 單一分頁可覆寫顏色（rec.fg / rec.bg）；未設定則跟隨全域設定 cfg
  function themeFor(rec) {
    return {
      background: (rec && rec.bg) || cfg.background,
      foreground: (rec && rec.fg) || cfg.foreground,
      cursor: "#ffffff", selectionBackground: "#264f78"
    };
  }
  function updateBodyBg() {
    var abg = (active && terms[active]) ? (terms[active].bg || cfg.background) : cfg.background;
    document.body.style.background = abg;
  }

  function makeTerm(id, title, flags) {
    if (terms[id]) return terms[id];
    var el = document.createElement("div");
    el.className = "term";
    el.dataset.id = id;

    var header = document.createElement("div");
    header.className = "pane-header";
    header.draggable = true;
    var titleSpan = document.createElement("span"); titleSpan.className = "ph-title";
    titleSpan.textContent = title || ("#" + id);
    header.appendChild(titleSpan);

    var body = document.createElement("div");
    body.className = "term-body";

    el.appendChild(header); // 標題列放在 pane 上方
    el.appendChild(body);
    container.appendChild(el);

    var term = new Terminal({
      fontFamily: cfg.fontFamily, fontSize: cfg.fontSize, cursorBlink: true,
      allowProposedApi: true, scrollback: 50000, theme: themeOf()
      // 注意：勿加 windowsPty conpty 模式——在此機器的 ConPTY 上反而造成輸入列殘字（v0.9.15 教訓）
    });
    var fit = new FitAddon.FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new Unicode11Addon.Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.loadAddon(new WebLinksAddon.WebLinksAddon());
    var ser = new SerializeAddon.SerializeAddon();
    term.loadAddon(ser);
    term.open(body);
    term.onData(function (d) {
      if (IMEDBG) dbgLog(id, "onData " + JSON.stringify(d));
      var rec0 = terms[id];
      if (rec0 && rec0.claudePaste) sendTyped(rec0, id, d);
      else ws.postMessage("i" + id + US + d);
    });

    // ── IME 診斷（IMEDBG=true 時）：組字/輸入事件送 C# 寫 diag.log，追注音重複問題 ──
    if (IMEDBG) (function () {
      var ta = body.querySelector(".xterm-helper-textarea");
      if (!ta) return;
      function tail(s) { s = s || ""; return s.length > 24 ? "…" + s.slice(-24) : s; }
      ["compositionstart", "compositionupdate", "compositionend"].forEach(function (ev) {
        ta.addEventListener(ev, function (e) {
          dbgLog(id, ev + " data=" + JSON.stringify(e.data == null ? null : String(e.data)) +
                     " val=" + JSON.stringify(tail(ta.value)));
        }, true);
      });
      ["beforeinput", "input"].forEach(function (ev) {
        ta.addEventListener(ev, function (e) {
          dbgLog(id, ev + " it=" + e.inputType + " data=" + JSON.stringify(e.data == null ? null : String(e.data)) +
                     " comp=" + !!e.isComposing + " val=" + JSON.stringify(tail(ta.value)));
        }, true);
      });
      ["keydown", "keyup"].forEach(function (ev) {
        ta.addEventListener(ev, function (e) {
          dbgLog(id, ev + " key=" + JSON.stringify(e.key) + " kc=" + e.keyCode + " comp=" + !!e.isComposing);
        }, true);
      });
      ["focus", "blur"].forEach(function (ev) {
        ta.addEventListener(ev, function () { dbgLog(id, ev + " val=" + JSON.stringify(tail(ta.value))); }, true);
      });
    })();

    // 組字預覽去殘影：微軟注音每個按鍵會先回報「原始鍵值」（h=ㄏ、8=ㄚ）再更新成注音，
    // xterm 把每次回報都畫進 .composition-view 就會閃出英數字。只動顯示層（不碰輸入流）。
    // v1.0.22 改法：依「目前內容」決定顯示——內容含英數字＝鍵值殘影 → 隱藏等轉換；
    // 純注音/中文 → 立刻顯示。舊法「每次變動先藏 30ms」讓預覽在打字時不斷閃爍消失；
    // 「含字母就永久隱藏」則讓英數組字（嘸蝦米/倉頡/拼音、注音內嵌英文）整段看不見。
    // 補一個 250ms 後備顯示：殘影必在幾 ms 內被轉換蓋掉，過了 250ms 還在的英數字＝真的內容。
    var compView = body.querySelector(".composition-view");
    if (compView) {
      var compTimer = null;
      new MutationObserver(function () {
        clearTimeout(compTimer);
        if (/[A-Za-z0-9]/.test(compView.textContent || "")) {
          compView.style.visibility = "hidden";
          compTimer = setTimeout(function () { compView.style.visibility = ""; }, 250);
        } else {
          compView.style.visibility = "";
        }
      }).observe(compView, { characterData: true, childList: true, subtree: true });
    }

    // Ctrl+F：在 xterm 處理前攔截 → 開搜尋列（回 false 不送進終端機）
    term.attachCustomKeyEventHandler(function (e) {
      if (e.type === "keydown" && e.ctrlKey && !e.shiftKey && !e.altKey &&
          (e.key === "f" || e.key === "F")) { openSearch(); return false; }
      return true;
    });

    el.addEventListener("mousedown", function () { setActivePane(id); });

    header.addEventListener("dragstart", function (e) {
      dragId = id; e.dataTransfer.effectAllowed = "move";
      try { e.dataTransfer.setData("text/plain", id); } catch (_) {}
    });
    header.addEventListener("dragend", function () { suppressClick = true; });
    // 點標題：放大成整頁 / 再點一次回到分割
    header.addEventListener("click", function () {
      if (mode === "tab") return;
      if (suppressClick) { suppressClick = false; return; }
      zoomed = (zoomed === id) ? null : id;
      layout();
    });
    el.addEventListener("dragover", function (e) {
      if (dragId != null && mode !== "tab") { e.preventDefault(); el.classList.add("drag-over"); }
    });
    el.addEventListener("dragleave", function () { el.classList.remove("drag-over"); });
    el.addEventListener("drop", function (e) {
      e.preventDefault(); el.classList.remove("drag-over"); onDrop(id);
    });

    terms[id] = { term: term, fit: fit, ser: ser, el: el, body: body, titleSpan: titleSpan, title: title || ("#" + id),
                  claudePaste: !!(flags && flags.indexOf("c") >= 0),
                  sendQ: [], sending: false, lastOutMs: 0 };

    // claude 分頁：瀏覽器原生貼上（Ctrl+V）也要走 doPaste（capture 階段先於 xterm 的 textarea 監聽）。
    // 搜尋列在 document 層級、不在 el 內，不受影響。
    el.addEventListener("paste", function (e) {
      var rp = terms[id];
      if (!rp || !rp.claudePaste) return; // 非 claude 分頁照舊交給 xterm
      e.preventDefault(); e.stopPropagation();
      var txt = "";
      try { txt = e.clipboardData.getData("text/plain") || ""; } catch (_) {}
      if (txt) doPaste(id, txt);
    }, true);

    layout();
    return terms[id];
  }

  // ── claude 分頁輸入佇列（v1.0.28 建立、v1.0.32 改整段送）──
  // 問題：IME 片語提交（注音一次送出「一二三」）到達 claude 是「一個多字元塊」，
  // claude 的按鍵解析把整塊當成單一事件——若前面有懸置的 ESC（按過 Esc）或 Ctrl+C 待確認
  // 等「等下一個按鍵」的狀態，整句會被當成未知跳脫序列整段吞掉。
  // v1.0.28 為此改「逐字、每 25ms」送；但逐字有個新副作用（使用者 2026-08 回報並定位）：
  //   **claude 每收一個字就重繪整條輸入列＋建議文字，逐字送等於把 claude 自身「輸入列回顯
  //   off-by-one」那個暫時殘影一次拉長成好幾個可見畫格** → 累積型注音（一次組「一二三」再提交）
  //   看起來就是「文字亂位」；單字即時上字只有一次重繪、太快看不到，所以不會發生（完全對上回報）。
  // v1.0.32 修法＝**改回整段一次送**，但保留 ESC[I 犧牲事件擋懸置狀態：
  //   ① 先送 focus-in 回報 ESC[I ——有懸置 ESC/Ctrl+C 就由它吸收；沒有時是合法 no-op
  //      （claude 開了 DECSET 1004），不像 NUL 會佔一格。
  //   ② 再把整個片語一次送出（delay 0）＝claude 只重繪一次，殘影太快看不到。
  //   與已驗證正常的貼上路徑（doPaste）同款；claude 2.1.237 + headless xterm 重播實測
  //   block/blockesc/char25 最終畫面都正確，但只有整段送不會在過程中攤開殘影
  //   （scratchpad ptyprobe raw-*.bin 重播，2026-08-20）。
  // 所有輸入（含貼上）走同一佇列保序，避免緊接的 Enter 超車；單一 ASCII 鍵／控制鍵即時送。
  //
  // v1.0.43 靜止閘門（quiet-gate）：實測與 diag/replay 證據都指出「二倍字串」與「半形+全形
  //   backspace 游標偏／殘影」都在 claude 端重繪時發生——xterm 每次 compositionend 只發一次
  //   onData（不重複），送進 ConPTY 的位元組重播出來最終畫面也正確。差別只在「送出的那一刻
  //   claude 是否正在重繪上一筆輸入」：若在忙（agents 執行中、建議文字在跳）時插入下一筆，
  //   claude 的差量渲染器偶爾會把剛插入的字複製一份或算錯跨全形/半形的游標欄位。
  //   對策＝只 gate「容易撞重繪」的輸入（IME 整段、貼上、backspace），等 claude 輸出靜止
  //   QUIET_MS 才送；但每筆從入列起最多等 QUIET_MAX_MS，claude 若持續重繪也不會卡死。
  //   一般 ASCII 打字／Enter／Ctrl 鍵不 gate、維持即時（否則打字手感變鈍）。逐字節流的 ghost
  //   教訓（v1.0.30→32）不適用：這裡仍是「整段一次送」，只是延後送出時機，不逐字。
  var PACE_MS = 25;
  var QUIET_MS = 20;       // claude 輸出靜止這麼久＝視為畫完上一筆輸入（設定可調：AppSettings.ImeQuietMs；0=關閉閘門）
  var QUIET_MAX_MS = 150;  // 入列後最多等這麼久就一定送（claude 持續重繪時的保險上限）
  function isTypedText(d) {
    if (!d.length) return false;
    var nonAscii = false;
    for (var i = 0; i < d.length; i++) {
      var c = d.charCodeAt(i);
      if (c < 0x20 || c === 0x7f) return false; // 控制字元＝按鍵/序列，不是 IME 文字
      if (c > 0x7f) nonAscii = true;
    }
    return d.length > 1 || nonAscii;
  }
  function qPush(rec, id, data, delay, gate) {
    rec.sendQ.push({ d: data, t: delay, g: !!gate, enq: performance.now() });
    if (rec.sending) return;
    rec.sending = true;
    (function step() {
      var it = rec.sendQ.shift();
      if (!it) { rec.sending = false; return; }
      // 靜止閘門：gate 的項目在 claude 仍在重繪（近 QUIET_MS 內有輸出）時先退回佇列稍等，
      // 但從入列算起超過 QUIET_MAX_MS 就一定送出，避免 claude 持續重繪時卡死。保序：退回用 unshift。
      if (it.g) {
        var now = performance.now();
        if ((now - rec.lastOutMs) < QUIET_MS && (now - it.enq) < QUIET_MAX_MS) {
          rec.sendQ.unshift(it);
          setTimeout(step, QUIET_MS);
          return;
        }
      }
      ws.postMessage("i" + id + US + it.d);
      if (it.t > 0) setTimeout(step, it.t);
      else step();
    })();
  }
  function sendTyped(rec, id, d) {
    if (isTypedText(d)) {
      qPush(rec, id, "\x1b[I", PACE_MS, true);  // 犧牲事件：吸收懸置的 ESC / Ctrl+C 待確認狀態
      qPush(rec, id, d, 0, true);               // 整個片語一次送（gate：等 claude 靜止再送，避免二倍/殘影）
    } else if (d === "\x7f" || d === "\x08") {
      qPush(rec, id, d, 0, true);               // backspace(DEL)／Ctrl+Backspace(BS)：跨全形/半形時 gate，等 claude 畫完上一格再刪
    } else {
      qPush(rec, id, d, 0, false);              // 一般 ASCII／控制鍵（含 Enter）：即時、不 gate
    }
  }

  // 統一貼上入口。claude 分頁不能靠 bracketed paste：Win10 conhost 會把 ESC[200~/201~
  // 從輸入流整組丟棄（實測 19045），claude 只能用「輸入叢發時序」猜是不是貼上，
  // 而 ConPTY 轉譯分塊時序不穩 → 多行有時被拆開/提前送出。
  // 改送 claude 自己的軟換行鍵 ESC+CR（= Shift+Enter，/terminal-setup 同款），
  // 每個換行都確定「插入新行、不送出」，不受分塊影響（ESC+CR 實測可完整穿透 ConPTY）。
  // 其餘分頁維持 xterm.paste()（\r\n 正規化 + 依程式的 bracketed paste 設定包 ESC[200~/201~）。
  // 貼上走佇列但整段原樣一次送（delay 0）：大量文字逐字送會拖數十秒，且貼上塊
  // 由 claude 的貼上偵測處理、實測正常，不套逐字節流。
  function doPaste(id, text) {
    var rec = terms[id];
    if (!rec || !text) return;
    if (rec.claudePaste) {
      var t = text.replace(/\r\n/g, "\r").replace(/\n/g, "\r").replace(/\r/g, "\x1b\r");
      qPush(rec, id, "\x1b[I", PACE_MS, true); // 同樣先吸收懸置狀態（Ctrl+C 待確認時貼上整段被吞，實測）
      qPush(rec, id, t, 0, true);              // 貼上整段一次送（gate：等 claude 靜止再送）
    } else {
      rec.term.paste(text);
    }
  }

  function selectId(id) {
    if (!terms[id]) return;
    active = id;
    layout();
    updateBodyBg();
    var rec = terms[id];
    setTimeout(function () { rec.term.focus(); }, 0);
  }

  function setActivePane(id) {
    if (!terms[id]) return;
    if (active !== id) { active = id; ws.postMessage("p" + id); }
    updateHighlight();
    updateBodyBg();
    terms[id].term.focus();
  }

  function updateHighlight() {
    for (var k in terms) terms[k].el.classList.toggle("active-pane", k === active);
  }

  function sendResize(id) {
    var rec = terms[id];
    if (rec) ws.postMessage("r" + id + US + rec.term.cols + "," + rec.term.rows);
  }

  function refit() {
    if (mode !== "tab") {
      for (var k in terms) { try { terms[k].fit.fit(); } catch (e) {} sendResize(k); }
    } else if (active && terms[active]) {
      try { terms[active].fit.fit(); } catch (e) {} sendResize(active);
    }
  }

  function layout() {
    var k;
    if (mode === "split" || mode === "columns") {
      container.classList.add("split");
      if (zoomed && !terms[zoomed]) zoomed = null;
      var n = Object.keys(terms).length;
      if (zoomed) {
        // 放大模式：只顯示該 pane（標題仍在，可再點一次還原）
        container.style.gridTemplateColumns = "1fr";
        for (k in terms) terms[k].el.style.display = (k === zoomed) ? "flex" : "none";
      } else if (mode === "columns") {
        // 分欄：全部橫向並排成一列（超寬螢幕用）
        container.style.gridTemplateColumns = "repeat(" + Math.max(1, n) + ", 1fr)";
        for (k in terms) terms[k].el.style.display = "flex";
      } else {
        // 分割：接近正方的 grid
        var cols = n <= 1 ? 1 : Math.ceil(Math.sqrt(n));
        container.style.gridTemplateColumns = "repeat(" + cols + ", 1fr)";
        for (k in terms) terms[k].el.style.display = "flex";
      }
      updateHighlight();
    } else {
      container.classList.remove("split");
      container.style.gridTemplateColumns = "";
      for (k in terms) terms[k].el.style.display = (k === active ? "flex" : "none");
    }
    requestAnimationFrame(refit);
  }

  function onDrop(targetId) {
    if (mode === "tab" || dragId == null || dragId === targetId) { dragId = null; return; }
    var dragEl = terms[dragId] && terms[dragId].el;
    var targetEl = terms[targetId] && terms[targetId].el;
    if (dragEl && targetEl) container.insertBefore(dragEl, targetEl);
    dragId = null;
    layout();
    notifyOrder();
  }

  function notifyOrder() {
    var order = [];
    for (var i = 0; i < container.children.length; i++) {
      var c = container.children[i];
      if (c.dataset && c.dataset.id) order.push(c.dataset.id);
    }
    ws.postMessage("k" + order.join(","));
  }

  function applyTheme() {
    for (var k in terms) {
      var rec = terms[k];
      rec.term.options.fontFamily = cfg.fontFamily;
      rec.term.options.fontSize = cfg.fontSize;
      rec.term.options.theme = themeFor(rec); // 保留各分頁自訂配色
      rec.el.style.background = rec.bg || cfg.background;
      rec.body.style.background = rec.bg || cfg.background;
    }
    updateBodyBg();
    requestAnimationFrame(refit);
  }

  // 遠端 /last 用（q…text）：取 buffer 最後 maxLines 個「邏輯行」純文字。
  // xterm 已把 TUI 原地重繪全部合成完畢，這裡拿到的是乾淨整行；isWrapped 的行接回上一行。
  function lastPlainText(term, maxLines) {
    var buf = term.buffer.active;
    var out = [];
    for (var i = Math.max(0, buf.length - maxLines); i < buf.length; i++) {
      var line = buf.getLine(i);
      if (!line) { out.push(""); continue; }
      var s = line.translateToString(true);
      if (line.isWrapped && out.length) out[out.length - 1] += s;
      else out.push(s);
    }
    return out.join("\n");
  }

  // ---------- Ctrl+F 搜尋（vendor 無 search addon → 自製 buffer 掃描）----------
  var sbEl = document.getElementById("searchbar");
  var sbInput = document.getElementById("search-input");
  var sbCount = document.getElementById("search-count");
  var sHits = [], sIdx = -1, sTimer = null;

  function openSearch() {
    sbEl.style.display = "flex";
    sbInput.focus(); sbInput.select();
    if (sbInput.value) { runSearch(); gotoHit(1); }
  }
  function closeSearch() {
    sbEl.style.display = "none";
    sHits = []; sIdx = -1; sbCount.textContent = "";
    var rec = active && terms[active];
    if (rec) { rec.term.clearSelection(); rec.term.focus(); }
  }
  // 收集 active pane 的全部命中（不分大小寫）。先用 translateToString 快篩，
  // 命中的行再逐 cell 建「字串索引 ↔ 欄位」對映（中文等寬字佔 2 欄，直接用字串索引選取會偏）。
  function runSearch() {
    sHits = []; sIdx = -1;
    var q = sbInput.value;
    var rec = active && terms[active];
    if (!q || !rec) { sbCount.textContent = ""; return; }
    var term = rec.term, buf = term.buffer.active, lq = q.toLowerCase();
    for (var row = 0; row < buf.length; row++) {
      var line = buf.getLine(row);
      if (!line) continue;
      if (line.translateToString(true).toLowerCase().indexOf(lq) < 0) continue;
      var map = [], str = "";
      for (var c = 0; c < line.length; c++) {
        var cell = line.getCell(c);
        if (!cell || cell.getWidth() === 0) continue;   // 寬字第二欄的佔位 cell
        map.push(c);
        str += (cell.getChars() || " ");
      }
      var low = str.toLowerCase(), from = 0, at;
      while ((at = low.indexOf(lq, from)) >= 0) {
        var endI = at + lq.length - 1;
        var startCol = map[at];
        var endCol = endI < map.length ? map[endI] : line.length - 1;
        var endW = 1;
        try { endW = Math.max(1, line.getCell(endCol).getWidth()); } catch (_) {}
        sHits.push({ row: row, col: startCol, len: endCol + endW - startCol });
        from = at + Math.max(1, lq.length);
      }
    }
  }
  function gotoHit(delta) {
    var rec = active && terms[active];
    if (!rec || !sHits.length) { sbCount.textContent = sbInput.value ? "0/0" : ""; return; }
    if (sIdx === -1 && delta < 0) sIdx = 0;   // 第一次就按「上一個」→ 從最後一筆開始
    sIdx = ((sIdx + delta) % sHits.length + sHits.length) % sHits.length;
    var h = sHits[sIdx], term = rec.term;
    term.select(h.col, h.row, h.len);
    term.scrollToLine(Math.max(0, h.row - Math.floor(term.rows / 2)));   // 命中行置中
    sbCount.textContent = (sIdx + 1) + "/" + sHits.length;
  }
  sbInput.addEventListener("input", function () {
    clearTimeout(sTimer);
    sTimer = setTimeout(function () { runSearch(); gotoHit(1); }, 250);
  });
  sbInput.addEventListener("keydown", function (e) {
    if (e.key === "Enter") { e.preventDefault(); if (!sHits.length) runSearch(); gotoHit(e.shiftKey ? -1 : 1); }
    else if (e.key === "Escape") { e.preventDefault(); closeSearch(); }
    e.stopPropagation();
  });
  document.getElementById("search-prev").addEventListener("click", function () { gotoHit(-1); });
  document.getElementById("search-next").addEventListener("click", function () { gotoHit(1); });
  document.getElementById("search-close").addEventListener("click", closeSearch);

  // 標題列目前路徑用（q…cwd）：從游標所在行往上找第一個非空行＝提示字元行，C# 端再解析路徑
  function promptLine(term) {
    var buf = term.buffer.active;
    var start = buf.baseY + buf.cursorY;
    for (var i = start; i >= 0 && i > start - 30; i--) {
      var line = buf.getLine(i);
      if (!line) continue;
      var s = line.translateToString(true).trim();
      if (s) return s;
    }
    return "";
  }

  ws.addEventListener("message", function (e) {
    var msg = e.data;
    if (typeof msg !== "string" || !msg.length) return;
    var kind = msg.charAt(0);
    var rest = msg.slice(1);
    var i, id;

    if (kind === "o") {
      i = rest.indexOf(US); id = rest.slice(0, i);
      var rec = terms[id]; if (!rec) return;
      var bin = atob(rest.slice(i + 1));
      var bytes = new Uint8Array(bin.length);
      for (var j = 0; j < bin.length; j++) bytes[j] = bin.charCodeAt(j);
      rec.term.write(bytes);
      rec.lastOutMs = performance.now(); // 靜止閘門用：記錄 claude 最後一次輸出（重繪）時間
    } else if (kind === "n") {
      i = rest.indexOf(US);
      if (i < 0) makeTerm(rest, null);
      else {
        var nid = rest.slice(0, i), nrest = rest.slice(i + 1), nj = nrest.indexOf(US);
        if (nj < 0) makeTerm(nid, nrest);
        else makeTerm(nid, nrest.slice(0, nj), nrest.slice(nj + 1));
      }
    } else if (kind === "t") {
      i = rest.indexOf(US); id = rest.slice(0, i);
      var rt = terms[id]; if (rt) { rt.title = rest.slice(i + 1); rt.titleSpan.textContent = rt.title; }
    } else if (kind === "s") {
      selectId(rest);
    } else if (kind === "x") {
      var rx = terms[rest];
      if (rx) { try { rx.term.dispose(); } catch (e) {} rx.el.remove(); delete terms[rest]; if (active === rest) active = null; layout(); }
    } else if (kind === "c") {
      var rc = terms[rest]; if (rc) rc.term.clear();
    } else if (kind === "L") {
      mode = (rest === "split" || rest === "columns") ? rest : "tab";
      zoomed = null;
      layout();
    } else if (kind === "T") {
      try {
        var t = JSON.parse(rest);
        if (t.fontFamily) cfg.fontFamily = t.fontFamily;
        if (t.fontSize) cfg.fontSize = t.fontSize;
        if (t.foreground) cfg.foreground = t.foreground;
        if (t.background) cfg.background = t.background;
        // 靜止閘門門檻（設定可調；0=關閉閘門，立即送）。用 typeof 判斷，允許 0。
        if (typeof t.imeQuietMs === "number" && t.imeQuietMs >= 0) QUIET_MS = t.imeQuietMs;
        applyTheme();
      } catch (e) {}
    } else if (kind === "q") {
      var k2 = rest.indexOf(US); var id2 = rest.slice(0, k2); var qk = rest.slice(k2 + 1);
      var r2 = terms[id2]; if (!r2) return;
      // 注意：未列出的種類（如 selpaste）一律回傳選取文字，由 C# 端決定後續處理
      var text = (qk === "all") ? r2.ser.serialize()
               : (qk === "text") ? lastPlainText(r2.term, 400)
               : (qk === "file") ? lastPlainText(r2.term, 1000000)   // 複製全部至檔案：整個 buffer 純文字（無 ANSI）
               : (qk === "cwd") ? promptLine(r2.term)                // 標題列目前路徑
               : r2.term.getSelection();
      ws.postMessage("a" + id2 + US + qk + US + text);
    } else if (kind === "v") {
      // 貼上：統一走 doPaste（一般分頁=xterm.paste；claude 分頁=ESC+CR 軟換行，見 doPaste 註解）
      i = rest.indexOf(US); id = rest.slice(0, i);
      if (!terms[id]) return;
      var vbin = atob(rest.slice(i + 1));
      var vbytes = new Uint8Array(vbin.length);
      for (var vj = 0; vj < vbin.length; vj++) vbytes[vj] = vbin.charCodeAt(vj);
      doPaste(id, new TextDecoder().decode(vbytes));
    } else if (kind === "S") {
      // 捲動檢視：S{id}US{up|down|top|bottom}（工具列「翻頁」用；只動視窗、不送任何輸入）
      i = rest.indexOf(US); id = rest.slice(0, i);
      var rs = terms[id]; if (!rs) return;
      var act = rest.slice(i + 1);
      if (act === "up") rs.term.scrollPages(-1);
      else if (act === "down") rs.term.scrollPages(1);
      else if (act === "top") rs.term.scrollToTop();
      else if (act === "bottom") rs.term.scrollToBottom();
      rs.term.focus();
    } else if (kind === "A") {
      var ra = terms[rest]; if (ra) { ra.term.focus(); ra.term.selectAll(); }
    } else if (kind === "F") {
      openSearch();
    } else if (kind === "P") {
      // 單一分頁配色：P{id}US{fg}US{bg}（fg/bg 皆空 = 清除覆寫、回到設定預設）
      var a1 = rest.indexOf(US); var pid = rest.slice(0, a1);
      var r3 = rest.slice(a1 + 1); var a2 = r3.indexOf(US);
      var pfg = r3.slice(0, a2), pbg = r3.slice(a2 + 1);
      var rp = terms[pid]; if (!rp) return;
      rp.fg = pfg || null; rp.bg = pbg || null;
      rp.term.options.theme = themeFor(rp);
      rp.el.style.background = rp.bg || cfg.background;
      rp.body.style.background = rp.bg || cfg.background;
      updateBodyBg();
      requestAnimationFrame(refit);
    }
  });

  // Ctrl + 滾輪：放大/縮小字級（全域，變更回報 C# 記住；新分頁/重開沿用）
  container.addEventListener("wheel", function (e) {
    if (!e.ctrlKey) return;
    e.preventDefault(); e.stopPropagation();
    var d = e.deltaY < 0 ? 1 : -1;
    var ns = Math.min(40, Math.max(6, (cfg.fontSize || 14) + d));
    if (ns === cfg.fontSize) return;
    cfg.fontSize = ns;
    applyTheme();
    ws.postMessage("z" + ns);
  }, { passive: false, capture: true });

  window.addEventListener("resize", function () { requestAnimationFrame(refit); });
  var rt;
  new ResizeObserver(function () { clearTimeout(rt); rt = setTimeout(refit, 30); }).observe(container);

  ws.postMessage("ready");
})();
