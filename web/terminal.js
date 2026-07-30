// AwayTerminal 前端：單一 WebView2 內管理多個 xterm。
// 兩種模式：tab（只顯示 active）/ split（grid 全部顯示，可拖曳 pane 標題重排）。
//
// 訊息協定（字串；US = \x1f）：
//   JS -> C# :  i{id}US{text} 輸入、r{id}US{cols},{rows} 尺寸、
//               a{id}US{kind}US{text} 查詢回覆、p{id} 選取某 pane、
//               k{id1},{id2},... 拖曳後的新順序、z{size} Ctrl+滾輪縮放後的字級、ready
//   C# -> JS :  o{id}US{base64} 輸出、n{id}US{title} 建立、t{id}US{title} 改名、
//               s{id} 選取、x{id} 關閉、c{id} 清畫面、L{tab|split} 切換模式、q{id}US{sel|all|text|file|cwd}、
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

  function makeTerm(id, title) {
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
    term.onData(function (d) { ws.postMessage("i" + id + US + d); });

    // 組字預覽去殘影：微軟注音每個按鍵會先回報「原始英文鍵值」（h=ㄏ）再更新成注音，
    // xterm 把每次回報都畫進 .composition-view 就會閃出英文字。
    // 只動顯示層（不碰輸入流）：內容一變先藏 30ms 吸收閃爍；內容含英文字母＝鍵值殘影，
    // 持續隱藏等下一次更新。注音組字不會有英文字母；若日後改用拼音輸入法需拿掉字母過濾。
    var compView = body.querySelector(".composition-view");
    if (compView) {
      var compTimer = null;
      new MutationObserver(function () {
        compView.style.visibility = "hidden";
        clearTimeout(compTimer);
        if (/[A-Za-z]/.test(compView.textContent || "")) return;
        compTimer = setTimeout(function () { compView.style.visibility = ""; }, 30);
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

    terms[id] = { term: term, fit: fit, ser: ser, el: el, body: body, titleSpan: titleSpan, title: title || ("#" + id) };
    layout();
    return terms[id];
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
    } else if (kind === "n") {
      i = rest.indexOf(US);
      if (i < 0) makeTerm(rest, null);
      else makeTerm(rest.slice(0, i), rest.slice(i + 1));
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
        applyTheme();
      } catch (e) {}
    } else if (kind === "q") {
      var k2 = rest.indexOf(US); var id2 = rest.slice(0, k2); var qk = rest.slice(k2 + 1);
      var r2 = terms[id2]; if (!r2) return;
      var text = (qk === "all") ? r2.ser.serialize()
               : (qk === "text") ? lastPlainText(r2.term, 400)
               : (qk === "file") ? lastPlainText(r2.term, 1000000)   // 複製全部至檔案：整個 buffer 純文字（無 ANSI）
               : (qk === "cwd") ? promptLine(r2.term)                // 標題列目前路徑
               : r2.term.getSelection();
      ws.postMessage("a" + id2 + US + qk + US + text);
    } else if (kind === "v") {
      // 貼上：交給 xterm.paste() —— 它會把 \r\n 正規化成 \r，並在程式啟用 bracketed paste
      // (DECSET 2004，如 claude/vim/新版 PSReadLine) 時包上 ESC[200~ / ESC[201~，
      // 多行才不會被逐行當成 Enter 送出（只剩最後一行）。之後照常經 onData 回送 i…
      i = rest.indexOf(US); id = rest.slice(0, i);
      var rv = terms[id]; if (!rv) return;
      var vbin = atob(rest.slice(i + 1));
      var vbytes = new Uint8Array(vbin.length);
      for (var vj = 0; vj < vbin.length; vj++) vbytes[vj] = vbin.charCodeAt(vj);
      rv.term.paste(new TextDecoder().decode(vbytes));
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
