//! TTL 巨集直譯器的端到端驗證：跑 `src-tauri/tests/ttl/` 底下的 `.ttl` 檔，
//! 逐個變數比對期望值。**期望值來自 TeraTerm 的原碼行為**（`ttpmacro/`），
//! 不是照我們的實作反推的——所以這支 probe 才有意義。
//!
//! 用法：`cargo run --example ttl_probe`

use std::path::{Path, PathBuf};

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

    println!("== AwayTerminal2 ttl_probe ==");

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
        ("sendln 'x'", Err::NotSupported),
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
