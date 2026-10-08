//! TTL 巨集直譯器的端到端驗證：跑 `src-tauri/tests/ttl/` 底下的 `.ttl` 檔，
//! 逐個變數比對期望值。**期望值來自 TeraTerm 的原碼行為**（`ttpmacro/`），
//! 不是照我們的實作反推的——所以這支 probe 才有意義。
//!
//! 用法：`cargo run --example ttl_probe`

use std::path::{Path, PathBuf};

use awayterminal_lib::ttl::host::{DialogAnswer, MacroHost, NullHost};
use awayterminal_lib::ttl::{self, Err, Vars};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ttl")
}

/// 跑一個檔並回傳變數表。
fn run(file: &str) -> std::result::Result<Vars, ttl::TtlError> {
    ttl::run_file(&dir().join(file), 200_000)
}

fn main() {
    let mut pass = 0usize;
    let mut fail = 0usize;

    println!("== AwayTerminal ttl_probe ==");

    // ------------------------------------------------- CRC／checksum／uptime
    match run("cksum.ttl") {
        Err(e) => {
            fail += 1;
            println!("FAIL  cksum.ttl 跑不完：{e}");
        }
        Ok(v) => {
            // 標準檢查向量（外部已知答案，不是我們自己跑出來的）
            let ints: &[(&str, i32)] = &[
                ("c_crc32", 0xCBF4_3926u32 as i32),
                ("c_crc16", 0x906E),
                ("c_sum8", 0xDD),
                ("c_sum16", 0x1DD),
                ("c_sum32", 0x1DD),
                // 空字串：照原碼不寫變數 → 哨兵值還在
                ("c_empty", 12345),
                // 檔案開不了：result = -1、變數不動
                ("c_nofile", 999),
                ("c_noresult", -1),
                ("c_up_positive", 1),
            ];
            let (p, f) = check(&v, "cksum.ttl", ints, &[]);
            pass += p;
            fail += f;
        }
    }

    // ---------------------------------------------------------------- 運算式
    match run("expr.ttl") {
        Err(e) => {
            fail += 1;
            println!("FAIL  expr.ttl 跑不完：{e}");
        }
        Ok(v) => {
            let ints: &[(&str, i32)] = &[
                ("e_add", 7),
                ("e_paren", 9),
                ("e_div", 3),
                ("e_divneg2", -3),
                ("e_mod", 1),
                ("e_modneg", -1),
                ("e_hex", 255),
                ("e_hexall", -1),
                ("e_wrap", i32::MIN),
                ("e_neg", -5),
                ("e_bnot", -1),
                ("e_lnot", 0),
                ("e_lnot0", 1),
                ("e_notword", -1),
                // 位元運算比比較運算緊（和 C 相反）
                ("e_bitfirst", 1),
                // 字詞運算子是位元、符號是邏輯
                ("e_and_w", 0),
                ("e_and_s", 1),
                ("e_or_w", 3),
                ("e_or_s", 1),
                ("e_xor_w", 2),
                ("e_lt", 1),
                ("e_le", 1),
                ("e_gt", 0),
                ("e_ge", 1),
                ("e_eq", 1),
                ("e_eq2", 1),
                ("e_ne", 1),
                ("e_ne2", 1),
                ("e_shl", 16),
                ("e_shr", 16),
                ("e_shr_neg", -4),
                ("e_lshr", 15),
                ("e_shl_sat", 0),
                ("e_shr_sat", -1),
                ("e_shl_neg", 0),
                ("n_cjklen", 6),
                ("n_backsl", 4),
            ];
            let strs: &[(&str, &str)] = &[
                ("s_plain", "abc"),
                ("s_pieces", "ab!cd"),
                ("s_hexcode", "AB"),
                ("s_cjk", "中文"),
                ("s_backsl", "a\\nb"),
            ];
            let (p, f) = check(&v, "expr.ttl", ints, strs);
            pass += p;
            fail += f;
        }
    }

    // ---------------------------------------------------------------- 流程控制
    match run("flow.ttl") {
        Err(e) => {
            fail += 1;
            println!("FAIL  flow.ttl 跑不完：{e}");
        }
        Ok(v) => {
            let ints: &[(&str, i32)] = &[
                ("f_if", 1),
                ("f_else", 2),
                ("f_elseif", 2),
                ("f_nest", 3),
                ("f_single", 5),
                ("f_single0", 0),
                ("f_forsum", 15),
                ("f_fori", 5),
                ("f_once", 1),
                ("f_down", 321),
                ("f_nestfor", 6),
                ("f_while", 3),
                ("f_while0", 9),
                ("f_until", 3),
                ("f_do", 3),
                ("f_dowhile", 2),
                ("f_loopuntil", 4),
                ("f_break", 6),
                ("f_cont", 12),
                ("f_breaknest", 4),
                ("f_breakwhile", 3),
                ("f_goto", 0),
                ("f_goto2", 2),
                ("f_gotoloop", 3),
                ("f_call", 7),
                ("f_after", 1),
            ];
            let (p, f) = check(&v, "flow.ttl", ints, &[]);
            pass += p;
            fail += f;
        }
    }

    // ---------------------------------------------------------------- 字串／整數指令
    match run("strings.ttl") {
        Err(e) => {
            fail += 1;
            println!("FAIL  strings.ttl 跑不完：{e}");
        }
        Ok(v) => {
            let ints: &[(&str, i32)] = &[
                ("c_len", 5),
                ("c_cmp1", -1),
                ("c_cmp2", 1),
                ("c_cmp3", 0),
                ("c_scan", 3),
                ("c_scan0", 0),
                ("c_split", 3),
                ("c_split2", 3),
                ("c_special", 3),
                ("c_str2int", 1),
                ("c_str2int0", 0),
                ("n1", 123),
                ("n2", 255),
                ("n3", 0),
                ("n4", 0x4142),
                ("c_arr", 15),
                ("c_def1", 1),
                ("c_def3", 3),
                ("c_def0", 0),
                ("c_def5", 5),
                ("n5", 2),
                ("n6", 1),
            ];
            let strs: &[(&str, &str)] = &[
                ("s1", "abcd"),
                ("s2", "bcd"),
                ("s3", "abc"),
                ("s4", "aXYbc"),
                ("s5", "aef"),
                ("s6", "axb"),
                ("s7", "abc"),
                ("s8", "ABC"),
                ("s9", "a"),
                ("s10", "c"),
                ("s11", "a-b-c"),
                ("s12", "a\tb"),
                ("s13", "42"),
                ("s14", "AB"),
                ("s15", "7-x"),
                ("s16", "00042"),
                ("s17", "42   |"),
                ("s18", "ff/FF/0xff"),
                ("s19", "   42"),
                ("s20", "100%"),
                ("s21", "9"),
                ("s22", "hi!"),
                ("s23", "file.txt"),
                ("s24", "C:\\dir"),
                ("s25", "C:\\dir\\file.txt"),
            ];
            let (p, f) = check(&v, "strings.ttl", ints, strs);
            pass += p;
            fail += f;
            // random 的範圍
            match v.int_of("n7") {
                Some(n) if (0..=3).contains(&n) => {
                    pass += 1;
                    println!("PASS  strings.ttl random n7={n}（要在 0..=3）");
                }
                other => {
                    fail += 1;
                    println!("FAIL  strings.ttl random n7={other:?}（要在 0..=3）");
                }
            }
        }
    }

    // ---------------------------------------------------------------- include
    match run("inc_main.ttl") {
        Err(e) => {
            fail += 1;
            println!("FAIL  inc_main.ttl 跑不完：{e}");
        }
        Ok(v) => {
            let ints: &[(&str, i32)] = &[
                ("i_before", 1),
                ("lib_value", 41),
                ("i_after", 42),
                ("i_from_lib", 1),
            ];
            let (p, f) = check(&v, "inc_main.ttl", ints, &[]);
            pass += p;
            fail += f;
        }
    }

    // ---------------------------------------------------------------- 舊版相容
    match run("oldversion.ttl") {
        Err(e) => {
            fail += 1;
            println!("FAIL  oldversion.ttl 跑不完：{e}");
        }
        Ok(v) => {
            let ints: &[(&str, i32)] = &[("c", 5), ("ok", 1), ("sum", 12)];
            let strs: &[(&str, &str)] = &[("acc", "123"), ("cstr", "5")];
            let (p, f) = check(&v, "oldversion.ttl", ints, strs);
            pass += p;
            fail += f;
        }
    }

    // ---------------------------------------------------------------- 錯誤的行號
    match run("errors.ttl") {
        Ok(_) => {
            fail += 1;
            println!("FAIL  errors.ttl 應該要出錯（第 6 行除以零）");
        }
        Err(e) => {
            let ok = e.err == Err::DivByZero && e.line_no == 7 && e.file == "errors.ttl";
            report(
                &mut pass,
                &mut fail,
                "errors.ttl 的錯誤位置",
                ok,
                format!(
                    "訊息={:?} 行號={} 檔名={}（要 Divide by zero. / 7 / errors.ttl）",
                    e.err.message(),
                    e.line_no,
                    e.file
                ),
            );
            report(
                &mut pass,
                &mut fail,
                "錯誤訊息的顯示格式",
                e.to_string().contains("Divide by zero.") && e.to_string().contains("errors.ttl:7"),
                format!("{e}"),
            );
        }
    }

    // ---------------------------------------------------------------- 語法錯誤幾種
    let bad: &[(&str, Err)] = &[
        ("a = (1+2", Err::CloseParent),
        ("a = 1/0", Err::DivByZero),
        ("a = nosuch + 1", Err::VarNotInit),
        ("a = 1\na = 'x'", Err::TypeMismatch),
        ("endif", Err::InvalidCtl),
        ("break", Err::InvalidCtl),
        ("return", Err::InvalidCtl),
        ("goto nosuchlabel", Err::LabelReq),
        (":dup\n:dup", Err::LabelAlreadyDef),
        ("intdim a 0", Err::Syntax),
        ("intdim a 2\nx = a[9]", Err::OutOfRange),
        // `sendln` 這一批實作了 → 沒有連線時是 `Link macro first.`
        ("sendln 'x'", Err::LinkFirst),
        // `waitregex` 這一批實作了 → 沒有連線時也是 `Link macro first.`
        ("waitregex 'x'", Err::LinkFirst),
        ("xmodemrecv 'f' 1 0", Err::NotSupported),
        ("a = 'unterminated", Err::Syntax),
    ];
    for (src, want) in bad {
        let got = ttl_err(src);
        report(
            &mut pass,
            &mut fail,
            &format!("語法／執行錯誤：{src:?}"),
            got == Some(*want),
            format!("得到 {got:?}，要 {want:?}"),
        );
    }

    // ---------------------------------------------------------------- I/O（TASK-013）
    io_section(&mut pass, &mut fail);

    println!();
    println!("RESULT: {pass} PASS / {fail} FAIL");
    if fail > 0 {
        std::process::exit(1);
    }
}

fn ttl_err(src: &str) -> Option<Err> {
    // ⚠️ 載入期就會發現的錯誤（重複的標籤是在 RegisterLabels 那一遍抓到的）
    // 也要回報——第一版用 `.ok()?` 把它吞掉了，結果「重複標籤」看起來像沒錯。
    let mut it = match ttl::Interp::from_text("inline.ttl", src) {
        Ok(it) => it,
        Err(e) => return Some(e.err),
    };
    match it.run(10_000) {
        Ok(()) => None,
        Err(e) => Some(e.err),
    }
}

fn report(pass: &mut usize, fail: &mut usize, name: &str, ok: bool, detail: String) {
    if ok {
        *pass += 1;
        println!("PASS  {name}：{detail}");
    } else {
        *fail += 1;
        println!("FAIL  {name}：{detail}");
    }
}

/// 比對一份變數表。回傳 (pass, fail)。
fn check(v: &Vars, file: &str, ints: &[(&str, i32)], strs: &[(&str, &str)]) -> (usize, usize) {
    let mut pass = 0;
    let mut fail = 0;
    let mut bad: Vec<String> = Vec::new();
    for (name, want) in ints {
        match v.int_of(name) {
            Some(got) if got == *want => {}
            other => bad.push(format!("{name}: 得到 {other:?}，要 {want}")),
        }
    }
    for (name, want) in strs {
        match v.str_of(name) {
            Some(got) if got == want.as_bytes() => {}
            other => bad.push(format!(
                "{name}: 得到 {:?}，要 {want:?}",
                other.map(|b| String::from_utf8_lossy(b).into_owned())
            )),
        }
    }
    let total = ints.len() + strs.len();
    if bad.is_empty() {
        pass += 1;
        println!("PASS  {file}：{total} 個檢查全對");
    } else {
        fail += 1;
        println!("FAIL  {file}：{} / {total} 個不對", bad.len());
        for b in &bad {
            println!("      | {b}");
        }
    }
    (pass, fail)
}

// ==================================================================== I/O 段

/// 用**程式內的 TCP echo server** 當對端，跑一支完整的巨集：
/// `wait` 提示 → `sendln` → `recvln` 拿回應 → 逾時 → 對話框。
///
/// 不用真的連線：`MacroHost` 就是那個接縫（見 `src/ttl/host.rs`）。
fn io_section(pass: &mut usize, fail: &mut usize) {
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    // --- 一台會回話的測試 server（127.0.0.1，臨時埠）---
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let got: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let got = got.clone();
        std::thread::spawn(move || {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            // 先送提示字元（含 ANSI 顏色：要能被 wait 比對到）
            let _ = sock.write_all(b"[32mlogin:[0m ");
            let mut buf = [0u8; 1024];
            loop {
                match sock.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        got.lock().unwrap().extend_from_slice(&buf[..n]);
                        // 收到一行就回一行（把收到的字回給對方 + OK）
                        if buf[..n].contains(&b'\r') {
                            let _ = sock.write_all(b"\r\nOK-DONE\r\n");
                        }
                    }
                }
            }
        });
    }

    // --- 把 socket 接成一個 MacroHost（讀取執行緒把資料丟進 RecvBuffer）---
    struct SockHost {
        inner: NullHost,
        sock: Mutex<std::net::TcpStream>,
    }
    impl MacroHost for SockHost {
        fn send(&self, data: &[u8]) {
            let mut g = self.sock.lock().unwrap();
            let _ = g.write_all(data);
            let _ = g.flush();
        }
        fn read_byte(&self) -> Option<u8> {
            self.inner.recv.read_byte()
        }
        fn flush_recv(&self) {
            self.inner.recv.clear();
        }
        fn stopped(&self) -> bool {
            self.inner.stopped()
        }
        fn connected(&self) -> bool {
            true
        }
        fn echo(&self, text: &str) {
            self.inner.echo(text);
        }
        fn dialog(&self, req: ttl::DialogRequest) -> DialogAnswer {
            self.inner.dialog(req)
        }
        fn sleep(&self, ms: u64) {
            std::thread::sleep(std::time::Duration::from_millis(ms.min(50)));
        }
    }

    let sock = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let reader = sock.try_clone().unwrap();
    let host = Arc::new(SockHost {
        inner: NullHost::default(),
        sock: Mutex::new(sock),
    });
    {
        let host2 = host.clone();
        let mut reader = reader;
        std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => host2.inner.recv.push(&buf[..n]),
                }
            }
        });
    }
    *host.inner.answer.lock().unwrap() = DialogAnswer {
        number: 1,
        text: "從對話框來的".into(),
        cancelled: false,
    };

    let src = "timeout = 3
wait 'login:'
w1 = result
sendln 'hello'
wait 'OK-DONE' 'never'
w2 = result
mtimeout = 100
timeout = 0
wait 'this will never come'
w3 = result
yesnobox '要繼續嗎' '巨集'
y = result
inputbox '輸入' '巨集' '預設'
s = inputstr
end
";
    let mut it = ttl::Interp::from_text("io.ttl", src).unwrap();
    it.set_host(Some(host.clone()));
    match it.run(100_000) {
        Err(e) => {
            *fail += 1;
            println!("FAIL  I/O 巨集跑不完：{e}");
        }
        Ok(()) => {
            let v = std::mem::take(&mut it.vars);
            let checks: &[(&str, i32)] = &[
                ("w1", 1),  // 有顏色的提示字元也比對得到（去 ANSI）
                ("w2", 1),  // 多候選：第一個命中
                ("w3", 0),  // 逾時
                ("y", 1),   // yesnobox
            ];
            let mut bad = Vec::new();
            for (k, want) in checks {
                if v.int_of(k) != Some(*want) {
                    bad.push(format!("{k}: 得到 {:?}，要 {want}", v.int_of(k)));
                }
            }
            if v.str_of("s").map(|b| b.to_vec()) != Some("從對話框來的".as_bytes().to_vec()) {
                bad.push("inputbox 的 inputstr 不對".to_string());
            }
            let sent = String::from_utf8_lossy(&got.lock().unwrap()).into_owned();
            if !sent.contains("hello\r") {
                bad.push(format!("server 沒收到 sendln 的內容（收到 {sent:?}）"));
            }
            if bad.is_empty() {
                *pass += 1;
                println!(
                    "PASS  I/O 巨集：wait（含 ANSI 提示字元）／多候選／逾時／sendln／對話框 6 項全對"
                );
                println!("      | server 收到：{sent:?}");
            } else {
                *fail += 1;
                println!("FAIL  I/O 巨集：{} 項不對", bad.len());
                for b in bad {
                    println!("      | {b}");
                }
            }
        }
    }

    // --- 舊版 samples/sample.ttl 整檔（含連線部分）---
    {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let got: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let got = got.clone();
            std::thread::spawn(move || {
                let Ok((mut sock, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 1024];
                loop {
                    match sock.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            got.lock().unwrap().extend_from_slice(&buf[..n]);
                            // 每收到一行就回一行（讓 `wait 'OK-DONE'` 能命中）
                            if buf[..n].contains(&b'\r') {
                                let _ = sock.write_all(b"OK-DONE\r\n");
                            }
                        }
                    }
                }
            });
        }
        let sock = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let reader = sock.try_clone().unwrap();
        let host = Arc::new(SockHost {
            inner: NullHost::default(),
            sock: Mutex::new(sock),
        });
        {
            let host2 = host.clone();
            let mut reader = reader;
            std::thread::spawn(move || {
                let mut buf = [0u8; 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => host2.inner.recv.push(&buf[..n]),
                    }
                }
            });
        }
        let path = dir().join("sample_full.ttl");
        let mut it = ttl::Interp::from_file(&path).expect("讀得到 sample_full.ttl");
        it.set_host(Some(host.clone()));
        it.vars.set_int("timeout", 3);
        match it.run(100_000) {
            Err(e) => {
                *fail += 1;
                println!("FAIL  舊版 sample.ttl 整檔：{e}");
            }
            Ok(()) => {
                let want = [
                    "echo === macro start ===",
                    "echo loop 1",
                    "echo loop 2",
                    "echo loop 3",
                    "echo a+b = 5",
                    "echo c equals 5 (correct)",
                    "echo === macro done ===",
                ];
                // ⚠️ 要等 server 的讀取執行緒真的把最後幾行收完（原本讀完就比 → 偶發假失敗）
                let mut sent;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                loop {
                    sent = String::from_utf8_lossy(&got.lock().unwrap()).into_owned();
                    if want.iter().all(|w| sent.contains(w)) || std::time::Instant::now() > deadline {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                let missing: Vec<&str> = want.iter().copied().filter(|w| !sent.contains(w)).collect();
                let asked = host.inner.asked.lock().unwrap().len();
                if missing.is_empty() && asked == 1 {
                    *pass += 1;
                    println!(
                        "PASS  舊版 sample.ttl 整檔：7 行輸出都送出去了、messagebox 跳了 1 次"
                    );
                } else {
                    *fail += 1;
                    println!(
                        "FAIL  舊版 sample.ttl 整檔：少了 {missing:?}、對話框 {asked} 次（要 1）"
                    );
                }
            }
        }
    }

    // --- waitregex：對程式內 server 跑一次（含群組與逾時）---
    {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            // 三行：第一行不該命中、第二行才命中（有顏色，驗去 ANSI）
            let _ = sock.write_all(b"noise line
");
            let _ = sock.write_all(b"[32mLogin incorrect[0m
");
            let _ = sock.write_all(b"user=bob uid=1001
");
            std::thread::sleep(std::time::Duration::from_millis(500));
        });
        let sock = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let reader = sock.try_clone().unwrap();
        let host = Arc::new(SockHost {
            inner: NullHost::default(),
            sock: Mutex::new(sock),
        });
        {
            let host2 = host.clone();
            let mut reader = reader;
            std::thread::spawn(move || {
                let mut buf = [0u8; 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => host2.inner.recv.push(&buf[..n]),
                    }
                }
            });
        }
        let src = concat!(
            "timeout = 3", "
",
            // 第 2 個 pattern 才命中（第 1 個是雜訊），而且來源有 ANSI 顏色
            r"waitregex 'zzz' '^Login (\w+)'", "
",
            "w1 = result", "
",
            "g1 = groupmatchstr1", "
",
            "l1 = inputstr", "
",
            r"waitregex 'uid=(\d+)'", "
",
            "w2 = result", "
",
            "g2 = groupmatchstr1", "
",
            "m2 = matchstr", "
",
            "mtimeout = 200", "
",
            "timeout = 0", "
",
            "waitregex 'never-ever'", "
",
            "w3 = result", "
",
            "end", "
",
        );
        let mut it = ttl::Interp::from_text("rx.ttl", src).unwrap();
        it.set_host(Some(host.clone()));
        match it.run(100_000) {
            Err(e) => {
                *fail += 1;
                println!("FAIL  waitregex 巨集跑不完：{e}");
            }
            Ok(()) => {
                let v = std::mem::take(&mut it.vars);
                let mut bad = Vec::new();
                if v.int_of("w1") != Some(2) {
                    bad.push(format!("w1（命中的 pattern 編號）={:?}，要 2", v.int_of("w1")));
                }
                if v.str_of("g1").map(|b| b.to_vec()) != Some(b"incorrect".to_vec()) {
                    bad.push(format!(
                        "g1={:?}，要 incorrect",
                        v.str_of("g1").map(|b| String::from_utf8_lossy(b).into_owned())
                    ));
                }
                if v.str_of("l1").map(|b| b.to_vec()) != Some(b"Login incorrect".to_vec()) {
                    bad.push(format!(
                        "inputstr={:?}，要 `Login incorrect`（去掉 ANSI 與 CR）",
                        v.str_of("l1").map(|b| String::from_utf8_lossy(b).into_owned())
                    ));
                }
                if v.int_of("w2") != Some(1) {
                    bad.push(format!("w2={:?}，要 1", v.int_of("w2")));
                }
                if v.str_of("g2").map(|b| b.to_vec()) != Some(b"1001".to_vec()) {
                    bad.push("g2（第二次的群組）不對".to_string());
                }
                if v.str_of("m2").map(|b| b.to_vec()) != Some(b"uid=1001".to_vec()) {
                    bad.push("matchstr 不對".to_string());
                }
                if v.int_of("w3") != Some(0) {
                    bad.push(format!("w3（逾時）={:?}，要 0", v.int_of("w3")));
                }
                if bad.is_empty() {
                    *pass += 1;
                    println!("PASS  waitregex：多 pattern／群組／inputstr（去 ANSI）／第二次比對／逾時 7 項全對");
                } else {
                    *fail += 1;
                    println!("FAIL  waitregex：{} 項不對", bad.len());
                    for b in bad {
                        println!("      | {b}");
                    }
                }
            }
        }
    }

    // --- exec：非沙盒分頁拿 exit code（假 host 記下請求，真的執行在 --verify 驗）---
    {
        let host = Arc::new(NullHost {
            exec_result: 7,
            ..Default::default()
        });
        let mut it = ttl::Interp::from_text(
            "exec.ttl",
            "exec 'cmd /c exit 7' 'hide' 1
r = result
execcmnd 'a = 40 + 2'
",
        )
        .unwrap();
        it.set_host(Some(host.clone()));
        match it.run(10_000) {
            Err(e) => {
                *fail += 1;
                println!("FAIL  exec 巨集跑不完：{e}");
            }
            Ok(()) => {
                let v = std::mem::take(&mut it.vars);
                let reqs = host.execs.lock().unwrap().clone();
                let ok = v.int_of("r") == Some(7)
                    && v.int_of("a") == Some(42)
                    && reqs.len() == 1
                    && reqs[0].cmdline == "cmd /c exit 7"
                    && reqs[0].hide
                    && reqs[0].wait;
                if ok {
                    *pass += 1;
                    println!(
                        "PASS  exec／execcmnd：result={:?}（exit code）、hide／wait 有傳下去、execcmnd 執行了 TTL 指令（a={:?}）",
                        v.int_of("r"),
                        v.int_of("a")
                    );
                } else {
                    *fail += 1;
                    println!(
                        "FAIL  exec／execcmnd：result={:?} a={:?} 請求={reqs:?}",
                        v.int_of("r"),
                        v.int_of("a")
                    );
                }
            }
        }
    }

    // --- 中斷：正在 wait 的巨集要能被叫停 ---
    let host2 = NullHost::shared();
    let mut it = ttl::Interp::from_text("stop.ttl", "timeout = 30
wait 'never'
a = 1").unwrap();
    it.set_host(Some(host2.clone()));
    let stop_flag = host2.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        stop_flag
            .stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let t0 = std::time::Instant::now();
    let r = it.run(1_000_000);
    let interrupted = matches!(&r, Err(e) if e.err == Err::Interrupted);
    let quick = t0.elapsed().as_secs() < 5;
    if interrupted && quick {
        *pass += 1;
        println!(
            "PASS  中斷正在 wait 的巨集：{}ms 就停了（逾時設 30 秒）",
            t0.elapsed().as_millis()
        );
    } else {
        *fail += 1;
        println!("FAIL  中斷正在 wait 的巨集：interrupted={interrupted} 花了 {:?}", t0.elapsed());
    }

    // --- 連線斷掉時 wait 不可以一直等 ---
    let host3 = Arc::new(NullHost {
        link: false,
        ..Default::default()
    });
    let mut it = ttl::Interp::from_text("dead.ttl", "sendln 'x'").unwrap();
    it.set_host(Some(host3));
    let got_link_err = matches!(it.run(1000), Err(e) if e.err == Err::LinkFirst);
    if got_link_err {
        *pass += 1;
        println!("PASS  沒有連線時 send 回 `Link macro first.`");
    } else {
        *fail += 1;
        println!("FAIL  沒有連線時 send 應該回 Link macro first.");
    }
}
