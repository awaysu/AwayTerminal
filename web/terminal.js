// AwayTerminal 前端：單一 WebView2 內管理多個 xterm。
// 兩種模式：tab（只顯示 active）/ split（grid 全部顯示，可拖曳 pane 標題重排）。
//
// 訊息協定（字串；US = \x1f）：
//   JS -> C# :  i{id}US{text} 輸入、r{id}US{cols},{rows} 尺寸、
//               a{id}US{kind}US{text} 查詢回覆、p{id} 選取某 pane、
//               k{id1},{id2},... 拖曳後的新順序、z{size} Ctrl+滾輪縮放後的字級、ready、
//               U{url} 點了終端機裡的連結（C# 跳「從瀏覽器開啟／複製網址」選單）、D{text} 診斷記錄、
//               y{id}US{text} 程式以 OSC 52 要求寫剪貼簿（1.1.10）、m{id} 下一個空選取回覆是因為程式接管滑鼠（1.1.10）
//   C# -> JS :  o{id}US{base64} 輸出、n{id}US{title}[US{flags}] 建立（flags 含 c=claude 貼上）、t{id}US{title} 改名、
//               s{id} 選取、x{id} 關閉、c{id} 清畫面、L{tab|split} 切換模式、
//               S{id}US{up|down|top|bottom} 捲動檢視（工具列「翻頁」；不送輸入）、
//               q{id}US{sel|selpaste|all|text|file|cwd|save}（selpaste 與 sel 同樣回選取文字，C# 端多做一次貼回；
//               save=關閉程式時取 scrollback 序列化文字（含顏色）供下次恢復）、
//               T{json} 套用字型顏色、P{id}US{fg}US{bg} 單一分頁配色（空=回設定預設）、
//               A{id} 全選、F 開搜尋列、v{id}US{base64} 貼上（走 xterm.paste，支援 bracketed paste）、
//               b{id}US{base64 舊內容}US{base64 分隔行}（1.0.45：把舊內容寫進分頁、再整頁推進 scrollback 並把游標歸位左上，
//               之後才啟動新 session——兩欄皆可空＝只做「推進 scrollback」，斷線重連前用；見 applyRestore 註解）、
//               g{下方id}US{上列比例}US{上列id,…}US{標籤|…}US{外框顏色,…}（1.2.0 Multi-Agent：把 2～4 個 pane 排成「下一上 N−1」、建立或更新）、
//               u{id}（1.2.0 拆掉 id 所在的 Multi-Agent 外框）、E{id}US{0|1|2|3|4}（1.2.0 pane 標題的狀態標籤：閒置／忙碌／有信待送／已結束／忙碌且有信待送）
//   JS -> C#（1.2.0）：G{下方id}US{上列比例} Multi-Agent 上下分隔線拖完的新比例
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
    fontSize: 14, foreground: "#e0e0e0", background: "#1e1e1e",
    agentStates: ["閒置", "忙碌", "有信待送", "已結束", "忙碌 · 有信待送"]   // Multi-Agent pane 狀態標籤文字（T 協定 agentStates 覆寫，隨語言）
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
    // 連結：點一下交給 C#（U 協定）而非 window.open（WebView2 會開內嵌視窗）。1.1.6 起用系統預設瀏覽器開；
    // 1.1.10 起 C# 先跳選單「從瀏覽器開啟／複製網址」
    term.loadAddon(new WebLinksAddon.WebLinksAddon(function (ev, uri) { ws.postMessage("U" + uri); }));
    // OSC 52 剪貼簿寫入（1.1.10）：程式接管滑鼠（DECSET 1000/1002/1003，例 claude 全螢幕介面＝在 claude 裡用 /tui 切換後，
    // 它重啟自己時會丟掉 CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN）時，xterm 的選取整個停用、getSelection() 永遠是空的——
    // 拖曳是 claude 自己畫反白，再用 OSC 52 送出選到的文字要終端機放進剪貼簿；xterm.js 沒載 clipboard addon 會直接忽略，
    // 於是「看得到反白、按複製卻說沒有選取文字、剪貼簿也沒東西」（scratchpad probe 以 ConPTY 實錄確認）。
    // 收到就交給 C# 寫剪貼簿（y 協定），並記在 rec.appSel 讓工具列「複製」／右鍵「複製且貼上」拿得到。只接受寫入：
    // 讀取查詢（Pd="?"）不回應、清空（Pd 空）忽略、超過 OSC52_MAX 字不收。
    term.parser.registerOscHandler(52, function (data) {
      var r52 = terms[id]; if (!r52) return true;
      var semi = data.indexOf(";");
      var pd = semi < 0 ? data : data.slice(semi + 1);
      if (!pd || pd === "?") return true;
      var txt52 = "";
      try { txt52 = new TextDecoder().decode(b64ToBytes(pd)); } catch (_) { return true; }
      if (!txt52 || txt52.length > OSC52_MAX) return true;
      r52.appSel = txt52;
      ws.postMessage("y" + id + US + txt52);
      return true;
    });
    var ser = new SerializeAddon.SerializeAddon();
    term.loadAddon(ser);
    term.open(body);
    term.onData(function (d) {
      if (IMEDBG) dbgLog(id, "onData " + JSON.stringify(d));
      var rec0 = terms[id];
      if (!rec0) { ws.postMessage("i" + id + US + d); return; }
      // IME 水位（1.1.9，見 setupImeGuard）：claude/一般分頁的 IME 文字都經這裡。d 若對得上 textarea「尚未送出」的
      // 內容＝xterm 正常從 textarea 送出→推進水位後照送；它前面若還夾著沒送過的字（前一筆漏送、使用者又接著打）先補送、保序。
      // d 若「已在水位後方」＝補救計時器已搶先送過同一段（xterm 的延遲送出比 40ms 補救更晚才落地）→ 抑制，避免重複送兩次。
      if (rec0.ta && isTypedText(d)) {
        var rest0 = taUnsent(rec0), k0 = rest0.indexOf(d);
        if (k0 < 0) { dbgLog(id, "ime-already-sent " + JSON.stringify(d)); return; } // 補救已送過：不重複
        if (k0 > 0 && !rec0.composing) { var missed = rest0.slice(0, k0); dbgLog(id, "ime-rescue ondata " + JSON.stringify(missed)); emitTyped(rec0, id, missed, true); }
        rec0.taMark += k0 + d.length;
      }
      emitTyped(rec0, id, d, false);
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

    el.addEventListener("mousedown", function (e) {
      // 左鍵按下＝程式那邊的選取會重來（拖曳結束後它會再送一次 OSC 52）或被點掉 → 舊的 appSel 作廢；右鍵要留著給右鍵選單的「複製」
      if (e.button === 0 && terms[id]) terms[id].appSel = "";
      setActivePane(id);
    });
    // 使用者自己捲動的意圖（1.2.0，見 settleBottoms）：滾輪、拖捲軸、Shift+PgUp/PgDn 之後記下「是不是停在最底」
    el.addEventListener("wheel", function () { noteScrollIntent(id); }, { passive: true });
    el.addEventListener("mouseup", function () { noteScrollIntent(id); });
    el.addEventListener("keyup", function (e) { if (e.key === "PageUp" || e.key === "PageDown" || e.key === "Home" || e.key === "End") noteScrollIntent(id); });

    header.addEventListener("dragstart", function (e) {
      dragId = id; e.dataTransfer.effectAllowed = "move";
      try { e.dataTransfer.setData("text/plain", id); } catch (_) {}
    });
    header.addEventListener("dragend", function () { suppressClick = true; });
    // 點標題：放大成整頁 / 再點一次回到分割
    header.addEventListener("click", function () {
      if (suppressClick) { suppressClick = false; return; }   // 拖曳結束的那一下不算點擊——分頁模式的組標題也可拖，旗標得在 mode 判斷前清掉
      if (mode === "tab") return;
      // Multi-Agent 的各格同一個顯示單位：點任一格的標題都是放大／還原整組
      zoomed = (zoomed && terms[zoomed] && terms[id] && unitEl(terms[zoomed]) === unitEl(terms[id])) ? null : id;
      layout();
    });
    el.addEventListener("dragover", function (e) {
      if (dragId != null && mode !== "tab") { e.preventDefault(); el.classList.add("drag-over"); }
    });
    el.addEventListener("dragleave", function () { el.classList.remove("drag-over"); });
    el.addEventListener("drop", function (e) {
      e.preventDefault(); el.classList.remove("drag-over"); onDrop(id);
    });

    terms[id] = { term: term, fit: fit, ser: ser, el: el, body: body, header: header, titleSpan: titleSpan, title: title || ("#" + id),
                  group: null, label: "", pill: null, agentState: -1,   // Multi-Agent（1.2.0，見 applyGroup）
                  atBottom: true,                                         // 使用者沒有往上捲（1.2.0，見 settleBottoms）
                  claudePaste: !!(flags && flags.indexOf("c") >= 0),
                  sendQ: [], sending: false, lastOutMs: 0,
                  keySeq: 0, imeLast: null,                 // IME 去重（1.0.45，見 sendTyped）
                  fitted: false, pendingRestore: null, pendingSep: null, held: null, restoreTimer: null, // 緩衝區恢復（1.0.45，見 applyRestore）
                  ta: null, taMark: 0, composing: false, compEndPending: false, rescueTimer: null, // IME 漏送補救（1.1.9，見 setupImeGuard）
                  appSel: "" };                             // 程式以 OSC 52 送來的選取文字（1.1.10，見 registerOscHandler(52) 與 selectionFor）
    setupImeGuard(id, terms[id], body);

    // 每個 keydown 遞增序號（capture 於 body 層＝比 xterm 掛在 textarea 上的 handler 更早跑）。
    // sendTyped 的 IME 去重靠它分辨「同字串兩次」是使用者真的再按一次（序號變了）還是 xterm 內部重複交付（序號沒變）。
    body.addEventListener("keydown", function (e) {
      var rk = terms[id]; if (!rk) return;
      rk.keySeq++;
      noteScrollIntent(id);   // 打字時 xterm 會捲回最底（scrollOnUserInput）→ 捲動意圖跟著更新（見 settleBottoms）
      if (!/^(Shift|Control|Alt|Meta|CapsLock)$/.test(e.key)) rk.appSel = "";   // 打字＝程式那邊的反白通常已消失，別再拿舊選取去複製
    }, true);

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
  // v1.0.45 IME 去重：使用者回報「注音開了獨立小視窗（非 inline 組字）時仍會重複兩次」。
  //   讀 vendored xterm 6.0.0 原始碼找到一條會重複交付的路徑：IME 以浮動視窗組字時，提交是走 textarea 的
  //   `input`(inputType=insertText) 事件——xterm `_inputEvent` 在 keyup 之後（_keyDownSeen=false）收到就直接
  //   triggerDataEvent 一次；同時 `compositionend` 排的 setTimeout（_finalizeComposition）再從 textarea.value
  //   取一次同樣的字串又送一次，而 `_dataAlreadySent` 只有 `_handleAnyTextareaChanges` 會設、`_inputEvent` 不會，
  //   所以兩份都送出＝「重複輸入二次」。inline 組字時走 insertCompositionText、沒有這條路，所以平常不發生。
  //   對策：同一分頁、同一字串、期間**沒有任何 keydown**、IME_DUP_MS 內再來一次＝xterm 重複交付 → 丟掉。
  //   有 keydown 就一定放行——使用者真的連按兩次「！」、或按住鍵盤自動重複，都有各自的 keydown，不會被誤殺。
  //   命中時寫一筆 diag.log（D 協定），之後看 log 就能確認重複到底發生在 JS 層還是 claude 端。
  var IME_DUP_MS = 100;
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
  function sendTyped(rec, id, d, noDedup) {
    if (isTypedText(d)) {
      var now = performance.now(), last = rec.imeLast;
      if (!noDedup && last && last.text === d && last.keySeq === rec.keySeq && (now - last.t) < IME_DUP_MS) {
        dbgLog(id, "ime-dup dropped " + JSON.stringify(d) + " dt=" + Math.round(now - last.t) + "ms");
        return;                                 // xterm 重複交付（見 IME_DUP_MS 註解）：第二份不送
      }
      rec.imeLast = { text: d, t: now, keySeq: rec.keySeq };
      qPush(rec, id, "\x1b[I", PACE_MS, true);  // 犧牲事件：吸收懸置的 ESC / Ctrl+C 待確認狀態
      qPush(rec, id, d, 0, true);               // 整個片語一次送（gate：等 claude 靜止再送，避免二倍/殘影）
    } else if (d === "\x7f" || d === "\x08") {
      qPush(rec, id, d, 0, true);               // backspace(DEL)／Ctrl+Backspace(BS)：跨全形/半形時 gate，等 claude 畫完上一格再刪
    } else {
      qPush(rec, id, d, 0, false);              // 一般 ASCII／控制鍵（含 Enter）：即時、不 gate
    }
  }
  // 統一送出入口：claude 分頁走佇列（sendTyped），其餘直接 i 協定。noDedup＝補救送出（依水位判定、不可能是重複）不套 IME 去重。
  function emitTyped(rec, id, d, noDedup) {
    if (rec.claudePaste) sendTyped(rec, id, d, noDedup);
    else ws.postMessage("i" + id + US + d);
  }

  // ── IME 漏送補救（1.1.9）──
  // 使用者回報「注音打完偶爾畫面沒字、再打一次才出來」。用 CDP 對真正的 index.html 重現（scratchpad imeweb.js）：
  //   注音以「獨立小視窗」組字時，提交是走 textarea 的 input(insertText) 事件、沒有 composition 事件。xterm 6.0.0 只有兩處會送它：
  //   ① _inputEvent——但按鍵仍按著（_keyDownSeen=true）時直接略過；② keydown(229) 排的 setTimeout(0)
  //   （_handleAnyTextareaChanges）比對 textarea 前後差異——但 IME 的提交常比這個 0ms timer 晚落地（IPC 另一個 task）。
  //   兩邊都沒接到＝文字留在 textarea、永遠不送：keydown229 → 30ms → insertText → keyup ＝ 什麼都沒送（S4）；
  //   insertText 只差 1ms 趕在 timer 前落地就正常（S2）＝時序競賽＝「偶爾」。inline 組字（compositionend）路徑本身沒問題。
  // 對策＝自己記 textarea「已送出到哪」（rec.taMark；xterm 從不清 textarea，只在 Enter/Ctrl+C 時清成空）：
  //   ① onData 送出的片語對得上水位之後的內容→推進水位（前面夾著的沒送字先補送）；
  //   ② 任何 IME/鍵盤事件後 IME_RESCUE_MS 內再無事件、且不在組字中→水位之後還有字＝xterm 漏送→補送並記 diag（ime-rescue）；
  //   ③ 真正的 Enter／Ctrl+C（非 229）在 xterm 清 textarea 之前先補送，順序＝文字→Enter。
  //   compEndPending：compositionend 排的 xterm 延遲送出還沒跑（主執行緒忙時 Enter 可能同批進來）→ 這時不補救，讓 xterm 自己同步送、不重複。
  var IME_RESCUE_MS = 40;
  function taUnsent(rec) {
    var v = rec.ta ? rec.ta.value : "";
    if (rec.taMark > v.length) rec.taMark = v.length;   // xterm 清過 textarea（Enter/Ctrl+C）或 IME 取消組字
    return v.substring(rec.taMark);
  }
  function rescueUnsent(rec, id, why) {
    if (!rec.ta || rec.composing || rec.compEndPending || !terms[id]) return;
    var rest = taUnsent(rec);
    if (!rest) return;
    rec.taMark = rec.ta.value.length;
    if (!isTypedText(rest)) return;                      // 單一 ASCII 殘值（dead key 之類）不補、只推水位
    dbgLog(id, "ime-rescue " + why + " " + JSON.stringify(rest));
    emitTyped(rec, id, rest, true);
  }
  function setupImeGuard(id, rec, body) {
    var ta = body.querySelector(".xterm-helper-textarea");
    if (!ta) return;
    rec.ta = ta; rec.taMark = ta.value.length;
    function arm() {
      clearTimeout(rec.rescueTimer);
      rec.rescueTimer = setTimeout(function () { rec.rescueTimer = null; rescueUnsent(rec, id, "timer"); }, IME_RESCUE_MS);
    }
    ta.addEventListener("compositionstart", function () { rec.composing = true; arm(); }, true);
    ta.addEventListener("compositionupdate", arm, true);
    ta.addEventListener("compositionend", function () {
      rec.composing = false; rec.compEndPending = true;
      setTimeout(function () { rec.compEndPending = false; }, 0);  // 排在 xterm 的延遲送出（同為 setTimeout 0）之後
      arm();
    }, true);
    ta.addEventListener("input", arm, true);
    ta.addEventListener("keyup", arm, true);
    // body capture 層＝比 xterm 掛在 textarea 上的 keydown 先跑：真 Enter／Ctrl+C 會讓 xterm 清空 textarea，先把沒送的字送掉
    body.addEventListener("keydown", function (e) {
      if (e.keyCode !== 229 && (e.keyCode === 13 || (e.ctrlKey && (e.key === "c" || e.key === "C")))) rescueUnsent(rec, id, "enter");
      arm();
    }, true);
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

  // 把一個可見的 pane fit 到容器大小；量得到尺寸（DOM 已排版）才算「fitted」。回傳量到的尺寸或 null。
  // quiet＝尺寸沒變就不回報 r（量隱藏的 Multi-Agent 組時每次 refit 都會跑，別一直對 ConPTY 送同尺寸 resize）
  function fitOne(k, rec, quiet) {
    var dims = null, c0 = rec.term.cols, r0 = rec.term.rows;
    try { dims = rec.fit.proposeDimensions(); } catch (e) {}
    try { rec.fit.fit(); } catch (e) {}
    if (!quiet || rec.term.cols !== c0 || rec.term.rows !== r0) sendResize(k);
    if (dims && dims.cols > 0 && dims.rows > 0) { markFitted(k, rec); return dims; }
    return null;
  }
  function markFitted(k, rec) {
    rec.fitted = true;
    if (rec.pendingRestore !== null) applyRestore(rec, k);
  }
  // 隱藏中的顯示單位（一般分頁或 Multi-Agent 外框）暫時「有排版但看不見」（visibility:hidden＋display:flex）量一次尺寸再藏回去
  function measureHidden(unit, ids) {
    var vis = unit.style.visibility, first = null;
    unit.style.visibility = "hidden"; unit.style.display = "flex";
    for (var i = 0; i < ids.length; i++) { var d = terms[ids[i]] ? fitOne(ids[i], terms[ids[i]], true) : null; if (i === 0) first = d; }
    unit.style.display = "none"; unit.style.visibility = vis;
    return first;
  }
  var lastSingleDims = null;   // 分頁模式最近一次量到的「整頁一般分頁」尺寸（Multi-Agent 在前景時，隱藏的一般分頁照它同步）
  function refit() {
    var k, rec;
    if (mode !== "tab") {
      for (k in terms) fitOne(k, terms[k]);
      return;
    }
    if (!active || !terms[active]) return;
    var ar = terms[active];
    if (ar.group) groupIds(ar.group).forEach(function (gk) { if (terms[gk]) fitOne(gk, terms[gk]); });   // Multi-Agent：各格各自 fit
    else { var d0 = fitOne(active, ar); if (d0) lastSingleDims = d0; }
    // 分頁模式：隱藏的一般分頁與作用中分頁共用同一個容器，尺寸直接同步成整頁量到的值
    // （隱藏的 display:none 量不到，fit 對它無效）。1.0.45 起這樣做有兩個好處：
    // ① 切到隱藏分頁時不再「先以舊尺寸顯示、再 fit 重排」閃一下；
    // ② 恢復緩衝區（applyRestore）必須在最終寬度下寫入——若在預設 80 欄寫、之後變寬時 xterm reflow 會把
    //    接回的長行從 scrollback 拉回可視區，接著被新 session 首幀的 ESC[2J 清掉＝舊訊息消失。
    // Multi-Agent 的格只占一部分，不能照整頁同步 → 用 measureHidden 實際量；還沒量過整頁尺寸時也先量一個隱藏的一般分頁。
    var dims = lastSingleDims, seen = [];
    for (k in terms) {
      if (k === active) continue;
      rec = terms[k];
      if (rec.group) {
        if (rec.group !== ar.group && seen.indexOf(rec.group) < 0) { seen.push(rec.group); measureHidden(rec.group.el, groupIds(rec.group)); }
        continue;
      }
      if (!dims) { dims = measureHidden(rec.el, [k]); if (dims) lastSingleDims = dims; continue; }
      if (rec.term.cols !== dims.cols || rec.term.rows !== dims.rows) {
        try { rec.term.resize(dims.cols, dims.rows); } catch (e) {}
        sendResize(k);
      }
      markFitted(k, rec);
    }
  }
  var refitQueued = false;
  function scheduleRefit() {
    if (refitQueued) return;
    refitQueued = true;
    requestAnimationFrame(function () { refitQueued = false; refit(); settleBottoms(); });
  }

  // ── 視窗「黏在最底」（1.2.0 修）──
  // 實錄（CDP 看 buffer）：恢復分頁時 Multi-Agent 的某一格 viewportY 75／baseY 96——pane 在 display:none 期間被 resize、
  // 之後搬進組外框才顯示，xterm 的捲動狀態卡在「使用者往上捲了」，之後的新輸出都不會跟著捲、畫面停在分隔行下面的空白＝整格看起來空白。
  // 不是每次都發生（時序競賽）。對策＝自己記「使用者的捲動意圖」（滾輪／拖捲軸／PgUp 等／翻頁／搜尋之後才更新 atBottom），
  // 每次排版 fit 完，看得見的 pane 只要使用者沒有刻意往上捲、卻不在最底，就捲回最底。
  function isAtBottom(rec) { try { var b = rec.term.buffer.active; return b.viewportY >= b.baseY; } catch (e) { return true; } }
  function noteScrollIntent(id) {
    setTimeout(function () { var r = terms[id]; if (r) r.atBottom = isAtBottom(r); }, 60);   // 等 xterm 處理完這次捲動
  }
  function settleBottoms() {
    for (var k in terms) {
      var r = terms[k];
      if (!r.atBottom || isAtBottom(r)) continue;
      var u = unitEl(r);
      if (u.style.display === "none" || r.el.style.display === "none") continue;   // 看不見的等顯示時再處理
      try { r.term.scrollToBottom(); } catch (e) {}
    }
  }

  // ── Multi-Agent 分頁（1.2.0；源自 1.1.11 協作分頁的兩半版本）──
  // C# 端是 2～4 個獨立分頁綁成一組（Models/AgentGroup）；這裡把它們的 pane 放進同一個 .agents 外框：
  //   上列 .agents-top 放格 2～4（由左到右平分）、中間 .ag-divider 可上下拖（拖完 G 協定回報比例、雙擊回 0.5）、下面放格 1（全寬）。
  // 只有一格時沒有上列。分頁模式整個外框當一個「顯示單位」切換，分割／分欄模式外框佔一格；拖曳排序、放大、K 重排都以顯示單位為準（unitEl）。
  // 每格外框顏色由 C# 給（1 淡紅、2 淡藍、3 淡綠、4 淡紫），作用中 pane 用 header 亮色標示、外框顏色不變。
  function unitEl(rec) { return rec.group ? rec.group.el : rec.el; }
  function unitList() {
    var u = [];
    for (var i = 0; i < container.children.length; i++) {
      var c = container.children[i];
      if (c.classList.contains("term") || c.classList.contains("agents")) u.push(c);
    }
    return u;
  }
  function groupIds(g) { return [g.bottom].concat(g.tops); }
  function clampRatio(r) { r = parseFloat(r); return isNaN(r) ? 0.5 : Math.min(0.85, Math.max(0.15, r)); }
  function paneTitle(rec) { rec.titleSpan.textContent = (rec.group && rec.label) ? rec.label : rec.title; }
  function updatePill(rec) {
    if (!rec.group || rec.agentState < 0) { if (rec.pill) { rec.pill.remove(); rec.pill = null; } return; }
    if (!rec.pill) { rec.pill = document.createElement("span"); rec.header.appendChild(rec.pill); }
    var names = ["idle", "busy", "queued", "exited", "busy"];   // 4＝忙碌且有信待送（樣式同忙碌，文字不同）
    rec.pill.className = "ph-pill st-" + (names[rec.agentState] || "idle");
    rec.pill.textContent = cfg.agentStates[rec.agentState] || "";
  }
  function applyGroupRatio(g) {
    var rb = terms[g.bottom];
    if (g.tops.length) {
      g.topEl.style.flex = g.ratio + " 1 0px";
      if (rb) rb.el.style.flex = (1 - g.ratio) + " 1 0px";
    } else if (rb) rb.el.style.flex = "1 1 0px";
  }
  // 依 g.bottom / g.tops 重新擺放外框裡的元素（appendChild 會把已在 DOM 裡的元素移過來、xterm 狀態不受影響）
  function arrangeGroup(g) {
    g.tops.forEach(function (k) { var r = terms[k]; if (r) { r.el.style.flex = ""; g.topEl.appendChild(r.el); } });
    g.el.appendChild(g.topEl);
    g.el.appendChild(g.divider);
    if (terms[g.bottom]) g.el.appendChild(terms[g.bottom].el);
    var hasTop = g.tops.length > 0;
    g.topEl.style.display = hasTop ? "flex" : "none";
    g.divider.style.display = hasTop ? "block" : "none";
    g.el.dataset.ids = groupIds(g).join(",");
  }
  function detachPane(rec) {
    rec.group = null; rec.label = ""; rec.agentState = -1;
    rec.el.style.flex = ""; rec.el.style.borderColor = ""; rec.el.classList.remove("agent-pane");
    updatePill(rec); paneTitle(rec);
  }
  function applyGroup(bottom, ratio, tops, labels, colors) {
    var all = [bottom].concat(tops), ids = [], lb = [], cl = [], i;
    for (i = 0; i < all.length; i++) if (all[i] && terms[all[i]]) { ids.push(all[i]); lb.push(labels[i] || ""); cl.push(colors[i] || ""); }
    if (!ids.length) return;
    var g = null;
    for (i = 0; i < ids.length && !g; i++) g = terms[ids[i]].group;
    if (g) {
      // 不在新名單裡的舊成員退回一般分頁；新名單裡屬於別組的先拉出來
      groupIds(g).forEach(function (k) {
        var r = terms[k];
        if (r && r.group === g && ids.indexOf(k) < 0) { if (g.el.parentNode === container) container.insertBefore(r.el, g.el); detachPane(r); }
      });
      ids.forEach(function (k) { if (terms[k].group && terms[k].group !== g) removeFromGroup(k, true); });
    } else {
      g = { el: document.createElement("div"), topEl: document.createElement("div"), divider: document.createElement("div"),
            bottom: ids[0], tops: [], ratio: 0.5 };
      g.el.className = "agents"; g.topEl.className = "agents-top"; g.divider.className = "ag-divider";
      container.insertBefore(g.el, unitEl(terms[ids[0]]));   // 佔下方那格原本的位置
      setupDivider(g);
    }
    g.bottom = ids[0]; g.tops = ids.slice(1); g.ratio = clampRatio(ratio);
    ids.forEach(function (k, j) {
      var r = terms[k];
      r.group = g; r.label = lb[j];
      r.el.classList.add("agent-pane");
      r.el.style.borderColor = cl[j];
      paneTitle(r); updatePill(r);
    });
    arrangeGroup(g);
    applyGroupRatio(g);
    layout();
  }
  // 一格離開組（被關掉／重新啟動）：組裡還有別格就重排（下方那格被拿掉時上列第一格補上），沒有就拆掉外框
  function removeFromGroup(id, noLayout) {
    var r = terms[id], g = r && r.group;
    if (!g) return;
    var rest = groupIds(g).filter(function (k) { return k !== id && terms[k] && terms[k].group === g; });
    if (g.el.parentNode === container) container.insertBefore(r.el, g.el);
    detachPane(r);
    if (!rest.length) g.el.remove();
    else { g.bottom = rest[0]; g.tops = rest.slice(1); arrangeGroup(g); applyGroupRatio(g); }
    if (!noLayout) layout();
  }
  function ungroupAll(id) {
    var r = terms[id], g = r && r.group;
    if (!g) return;
    groupIds(g).forEach(function (k) {
      var rk = terms[k];
      if (!rk || rk.group !== g) return;
      if (g.el.parentNode === container) container.insertBefore(rk.el, g.el);
      detachPane(rk);
    });
    g.el.remove();
    layout();
  }
  function setupDivider(g) {
    g.divider.addEventListener("mousedown", function (e) {
      if (e.button !== 0) return;
      e.preventDefault(); e.stopPropagation();
      var rect = g.el.getBoundingClientRect();
      g.el.classList.add("dragging");
      function mv(ev) {
        g.ratio = clampRatio((ev.clientY - rect.top) / rect.height);
        applyGroupRatio(g);
        scheduleRefit();
      }
      function up() {
        document.removeEventListener("mousemove", mv, true);
        document.removeEventListener("mouseup", up, true);
        g.el.classList.remove("dragging");
        ws.postMessage("G" + g.bottom + US + g.ratio.toFixed(3));
        scheduleRefit();
      }
      document.addEventListener("mousemove", mv, true);
      document.addEventListener("mouseup", up, true);
    });
    g.divider.addEventListener("dblclick", function () {
      g.ratio = 0.5; applyGroupRatio(g); scheduleRefit();
      ws.postMessage("G" + g.bottom + US + "0.5");
    });
  }

  // ── 緩衝區恢復 / 推進 scrollback（1.0.45）──
  // 用途：① 關閉程式勾「恢復分頁」→ 下次開啟先把上次存的 scrollback（q…save 序列化、含顏色）倒回分頁，再啟動連線；
  //       ② SSH/Telnet/COM 斷線重連前把舊畫面保住。
  // 為什麼要「推進 scrollback」：ConPTY 每個新 session 的第一幀一定送 ESC[2J（conhost XtermEngine 首次 StartPaint
  // 會 _ClearScreen），而 xterm 的 ED 2 是原地清掉可視區、不會把它推進 scrollback → 重連/恢復後「最後一頁」憑空消失。
  // 對策＝啟動新 session 之前，先把游標放到最底列、送 rows 個換行（每個都把最上面一列推進 scrollback）、再 ESC[H 回左上：
  // 可視區只剩空白列，2J 清的是空白；游標在最上列且底下全空白，之後視窗變高也不會把 scrollback 拉回來（xterm resize 規則）。
  // 前置 ESC[?1049l／ESC[r／ESC[0m：舊 session 若死在 alt screen（vim/top）或留下捲動區域，先回正常畫面、解除區域，
  // 否則換行只在區域內捲、不會進 scrollback。
  // 時序：這裡的 write 與後續 o 輸出都走 xterm 的寫入佇列、保序；但恢復內容要等 pane fitted（最終寬度）才能寫，
  // 期間收到的 o 輸出先存進 held、寫完舊內容再依序補寫。fit 一直量不到（視窗最小化啟動）時 4 秒後照寫，分頁不會卡死。
  function applyRestore(rec, id) {
    var payload = rec.pendingRestore, sep = rec.pendingSep || null;
    rec.pendingRestore = null; rec.pendingSep = null;
    clearTimeout(rec.restoreTimer); rec.restoreTimer = null;
    var term = rec.term, rows = term.rows;
    if (payload && payload.length) term.write(payload);
    var seq = "\x1b[?1049l\x1b[r\x1b[0m\x1b[" + rows + ";1H";
    if (sep && sep.length) { term.write(seq + "\r\n"); term.write(sep); seq = ""; }  // 分隔行寫在最底列（先捲一行，別壓到舊內容）
    term.write(seq + "\r\n".repeat(rows) + "\x1b[H");
    var held = rec.held; rec.held = null;
    if (held) for (var i = 0; i < held.length; i++) term.write(held[i]);
    rec.lastOutMs = performance.now();
    rec.atBottom = true;   // 恢復後本來就該停在最底（1.2.0，見 settleBottoms）
  }

  // 版面以「顯示單位」為準（1.2.0）：一般分頁＝它的 pane、Multi-Agent＝整個 .agents 外框（裡面各格一律顯示）
  function layout() {
    var k, i, units = unitList();
    for (k in terms) if (terms[k].group) terms[k].el.style.display = "flex";
    if (mode === "split" || mode === "columns") {
      container.classList.add("split");
      if (zoomed && !terms[zoomed]) zoomed = null;
      var n = units.length, zu = zoomed ? unitEl(terms[zoomed]) : null;
      if (zu) {
        // 放大模式：只顯示該 pane（Multi-Agent＝整組；標題仍在，可再點一次還原）
        container.style.gridTemplateColumns = "1fr";
        for (i = 0; i < n; i++) units[i].style.display = (units[i] === zu) ? "flex" : "none";
      } else if (mode === "columns") {
        // 分欄：全部橫向並排成一列（超寬螢幕用）
        container.style.gridTemplateColumns = "repeat(" + Math.max(1, n) + ", 1fr)";
        for (i = 0; i < n; i++) units[i].style.display = "flex";
      } else {
        // 分割：接近正方的 grid
        var cols = n <= 1 ? 1 : Math.ceil(Math.sqrt(n));
        container.style.gridTemplateColumns = "repeat(" + cols + ", 1fr)";
        for (i = 0; i < n; i++) units[i].style.display = "flex";
      }
    } else {
      container.classList.remove("split");
      container.style.gridTemplateColumns = "";
      var au = active && terms[active] ? unitEl(terms[active]) : null;
      for (i = 0; i < units.length; i++) units[i].style.display = (units[i] === au) ? "flex" : "none";
    }
    updateHighlight();
    requestAnimationFrame(function () { refit(); settleBottoms(); });
  }

  function onDrop(targetId) {
    if (mode === "tab" || dragId == null || dragId === targetId) { dragId = null; return; }
    var dragEl = terms[dragId] && unitEl(terms[dragId]);
    var targetEl = terms[targetId] && unitEl(terms[targetId]);
    if (dragEl && targetEl && dragEl !== targetEl) container.insertBefore(dragEl, targetEl);
    dragId = null;
    layout();
    notifyOrder();
  }

  function notifyOrder() {
    var order = [], units = unitList();
    for (var i = 0; i < units.length; i++) {
      var c = units[i];
      if (c.classList.contains("agents")) order = order.concat((c.dataset.ids || "").split(",").filter(Boolean));   // Multi-Agent：各格相鄰、下方那格在前
      else if (c.dataset && c.dataset.id) order.push(c.dataset.id);
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

  // 關閉程式時保留的 scrollback 行數（可視區另計；T 協定 restoreLines 可調）
  var SAVE_LINES = 2000;
  // q…save：序列化最後 SAVE_LINES 行 scrollback＋可視區（含顏色）。只取正常畫面（alt screen 裡的 vim/top 不存）、
  // 不帶模式切換序列（bracketed paste／應用程式游標鍵等模式應由新 session 自己設定，別替它預設）。
  function saveBuffer(rec) {
    try {
      // 舊內容還沒倒回 xterm（視窗還沒量到尺寸就關程式）：要存的就是那一份，別把空的 xterm 序列化蓋掉上次的 scrollback
      if (rec.pendingRestore !== null) return new TextDecoder("utf-8").decode(rec.pendingRestore);
      return rec.ser.serialize({ scrollback: SAVE_LINES, excludeAltBuffer: true, excludeModes: true });
    }
    catch (e) { return ""; }
  }
  function b64ToBytes(b64) {
    if (!b64) return new Uint8Array(0);
    var bin = atob(b64), out = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }

  // 工具列「複製」／右鍵「複製、複製且貼上」的選取文字（q…sel/selpaste；1.1.10）：
  // ① xterm 自己的選取優先（一般拖曳；接管滑鼠的程式裡按住 Shift 拖曳也是這個）；
  // ② 程式接管滑鼠時 xterm 選取停用 → 改用程式最近一次 OSC 52 送來的選取（claude 全螢幕介面拖曳反白）；
  // ③ 都沒有、且正在接管滑鼠 → 先送 m{id}，讓 C# 的「沒有選取文字」提示改說「按住 Shift 再拖曳」。
  var OSC52_MAX = 1000000;
  function selectionFor(rec, id) {
    var s = rec.term.getSelection();
    if (s) return s;
    if (rec.term.modes.mouseTrackingMode === "none") return "";
    if (rec.appSel) return rec.appSel;
    ws.postMessage("m" + id);
    return "";
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
  var sHits = [], sIdx = -1, sTimer = null, sPane = null;   // sPane＝這批命中屬於哪個 pane（切了 pane 要重找）

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
    sHits = []; sIdx = -1; sPane = active;
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
        var chs = cell.getChars() || " ";
        for (var u = 0; u < chs.length; u++) map.push(c);   // emoji／組合字元一格佔多個 UTF-16 單位：每個單位都對回同一欄，後面的命中才不會偏
        str += chs;
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
    if (sPane !== active) runSearch();   // 命中是別的 pane 的（分割模式點了另一格再按 Enter）：對現在的 pane 重找，別拿舊座標去選
    var rec = active && terms[active];
    if (!rec || !sHits.length) { sbCount.textContent = sbInput.value ? "0/0" : ""; return; }
    if (sIdx === -1 && delta < 0) sIdx = 0;   // 第一次就按「上一個」→ 從最後一筆開始
    sIdx = ((sIdx + delta) % sHits.length + sHits.length) % sHits.length;
    var h = sHits[sIdx], term = rec.term;
    term.select(h.col, h.row, h.len);
    term.scrollToLine(Math.max(0, h.row - Math.floor(term.rows / 2)));   // 命中行置中
    rec.atBottom = isAtBottom(rec);   // 搜尋跳到的位置＝使用者要看的地方（見 settleBottoms）
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
  // 1.1.2：提示行太長被折行（長路徑的 PS C:\…> 常見）時，游標所在列只是邏輯行的後半段、C# 端 regex 對不到；
  // 往上接回整條邏輯行再回傳。判斷「上一列是同一條邏輯行」＝本列 isWrapped（xterm 自動折行）或上一列最後一格有字
  // （ConPTY 逐列重繪、折行處送的是硬換行、isWrapped 不會設，只能看上一列是否填滿）。中間列不 trim，
  // 路徑含空白剛好切在列尾也不會被吃掉。
  function promptLine(term) {
    var buf = term.buffer.active;
    var start = buf.baseY + buf.cursorY;
    var lastCol = term.cols - 1;
    function joinedAbove(j) {
      var cur = buf.getLine(j), prev = j > 0 ? buf.getLine(j - 1) : null;
      if (!cur || !prev) return false;
      if (cur.isWrapped) return true;
      var c = prev.getCell(lastCol);
      var ch = c ? c.getChars() : "";
      return !!ch && ch !== " ";
    }
    for (var i = start; i >= 0 && i > start - 30; i--) {
      var line = buf.getLine(i);
      if (!line) continue;
      var s = line.translateToString(true).trim();
      if (!s) continue;
      var j = i;
      while (j > 0 && i - j < 8 && joinedAbove(j)) j--;
      if (j === i) return s;
      var parts = [];
      for (var k = j; k <= i; k++) { var l = buf.getLine(k); if (l) parts.push(l.translateToString(k === i)); }
      return parts.join("").trim();
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
      if (rec.pendingRestore !== null) { rec.held.push(bytes); return; } // 舊內容還沒倒回去：先扣住，applyRestore 後依序補寫
      rec.term.write(bytes);
      rec.lastOutMs = performance.now(); // 靜止閘門用：記錄 claude 最後一次輸出（重繪）時間
    } else if (kind === "b") {
      // 恢復緩衝區／推進 scrollback：b{id}US{base64 舊內容}US{base64 分隔行}（兩欄皆可空＝只推）。見 applyRestore。
      var b1 = rest.indexOf(US); var bid = rest.slice(0, b1);
      var rb = terms[bid]; if (!rb) return;
      if (rb.pendingRestore !== null) applyRestore(rb, bid);   // 上一份還沒倒回去（恢復後馬上重連）：先把它和扣住的輸出寫進去，不能直接丟掉
      var bRest = rest.slice(b1 + 1), b2 = bRest.indexOf(US);
      var bPayload = b2 < 0 ? bRest : bRest.slice(0, b2), bSep = b2 < 0 ? "" : bRest.slice(b2 + 1);
      rb.pendingRestore = b64ToBytes(bPayload);
      rb.pendingSep = bSep ? b64ToBytes(bSep) : null;
      rb.held = [];
      if (rb.fitted) applyRestore(rb, bid);
      else rb.restoreTimer = setTimeout(function () { if (rb.pendingRestore !== null) applyRestore(rb, bid); }, 4000);
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
      var rt = terms[id]; if (rt) { rt.title = rest.slice(i + 1); paneTitle(rt); }   // Multi-Agent 的 pane 標題仍顯示 Agent 標籤
    } else if (kind === "s") {
      selectId(rest);
    } else if (kind === "g") {
      // 1.2.0 Multi-Agent：g{下方id}US{上列比例}US{上列id,…}US{標籤|…}US{外框顏色,…}
      var gp = rest.split(US);
      applyGroup(gp[0], gp[1], (gp[2] || "").split(",").filter(Boolean), (gp[3] || "").split("|"), (gp[4] || "").split(","));
    } else if (kind === "u") {
      ungroupAll(rest);   // 1.2.0：拆組，各格變回一般分頁
    } else if (kind === "E") {
      // 1.2.0 Multi-Agent pane 狀態標籤：E{id}US{0 閒置|1 忙碌|2 有信待送|3 已結束}
      var e1 = rest.indexOf(US), re = terms[rest.slice(0, e1)];
      if (re) { re.agentState = parseInt(rest.slice(e1 + 1), 10); updatePill(re); }
    } else if (kind === "x") {
      var rx = terms[rest];
      if (rx) {
        if (rx.group) removeFromGroup(rest, true);   // Multi-Agent 的一格被關：先從組裡拿出來（其他格重排）
        clearTimeout(rx.restoreTimer); try { rx.term.dispose(); } catch (e) {} rx.el.remove(); delete terms[rest]; if (active === rest) active = null; layout();
      }
    } else if (kind === "c") {
      var rc = terms[rest]; if (rc) rc.term.clear();
    } else if (kind === "L") {
      mode = (rest === "split" || rest === "columns") ? rest : "tab";
      zoomed = null;
      layout();
    } else if (kind === "K") {
      // 右側分頁列拖曳後的新順序（1.1.8，C#→JS）：依 id 順序把 pane 元素重排，分割/分欄模式的 pane 才與分頁列一致
      var korder = rest.length ? rest.split(",") : [];
      for (var ki = 0; ki < korder.length; ki++) { var kr = terms[korder[ki]]; if (kr && kr.el) container.appendChild(unitEl(kr)); }   // Multi-Agent 整組一起移
      layout();
    } else if (kind === "T") {
      try {
        var t = JSON.parse(rest);
        if (t.fontFamily) cfg.fontFamily = t.fontFamily;
        if (t.fontSize) cfg.fontSize = t.fontSize;
        if (t.search) {   // 搜尋列文字隨語言（index.html 裡的預設是中文）
          try {
            sbInput.placeholder = t.search.placeholder || sbInput.placeholder;
            document.getElementById("search-prev").title = t.search.prev || "";
            document.getElementById("search-next").title = t.search.next || "";
            document.getElementById("search-close").title = t.search.close || "";
          } catch (_) {}
        }
        if (t.foreground) cfg.foreground = t.foreground;
        if (t.background) cfg.background = t.background;
        // 靜止閘門門檻（設定可調；0=關閉閘門，立即送）。用 typeof 判斷，允許 0。
        if (typeof t.imeQuietMs === "number" && t.imeQuietMs >= 0) QUIET_MS = t.imeQuietMs;
        // 關閉程式時每個分頁保留的 scrollback 行數（q…save；AppSettings.RestoreBufferLines）
        if (typeof t.restoreLines === "number" && t.restoreLines >= 0) SAVE_LINES = t.restoreLines;
        // Multi-Agent pane 狀態標籤文字（隨語言）
        if (t.agentStates && t.agentStates.length >= 4) { cfg.agentStates = t.agentStates; for (var ak in terms) updatePill(terms[ak]); }
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
               : (qk === "save") ? saveBuffer(r2)                    // 關閉程式：scrollback 序列化（含顏色）供下次恢復
               : selectionFor(r2, id2);                              // sel / selpaste（含程式接管滑鼠時的 OSC 52 選取）
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
      rs.atBottom = isAtBottom(rs);   // 使用者用「翻頁」捲的＝捲動意圖（見 settleBottoms）
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

  window.addEventListener("resize", function () { requestAnimationFrame(function () { refit(); settleBottoms(); }); });
  var rt;
  new ResizeObserver(function () { clearTimeout(rt); rt = setTimeout(function () { refit(); settleBottoms(); }, 30); }).observe(container);

  ws.postMessage("ready");
})();
