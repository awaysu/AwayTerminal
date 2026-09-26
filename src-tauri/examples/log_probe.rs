//! `Logger` 的最小驗證：在指定路徑寫一份 log 並把結果印出來。
//!
//! 用途：把「log 格式對不對」與「這台機器的資料夾能不能寫」分開。
//! 用法：`cargo run --example log_probe [路徑]`（預設寫到 %TEMP%\awayterm-log-probe.log）
use std::time::Instant;

use awayterminal_lib::logging::Logger;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        std::env::temp_dir()
            .join("awayterm-log-probe.log")
            .to_string_lossy()
            .to_string()
    });
    println!("path = {path}");

    let t0 = Instant::now();
    let lg = match Logger::open(std::path::Path::new(&path), true, false) {
        Ok(l) => l,
        Err(e) => {
            println!("FAIL open ({:?})：{e}", t0.elapsed());
            std::process::exit(1);
        }
    };
    println!("open ok ({:?})", t0.elapsed());

    // 彩色中文 + OSC 視窗標題 + CRLF，和真實 PTY 輸出同一類
    lg.write(b"\x1b]0;away\x07\x1b[36m\xe4\xb8\xad\xe6\x96\x87 AWAY_LOG_OK\x1b[0m\r\n");
    lg.write(b"second line\r\n");
    lg.close();
    println!("write ok ({:?})", t0.elapsed());

    let bytes = std::fs::read(&path).expect("read back");
    println!("bytes = {}", bytes.len());
    println!("BOM   = {}", bytes[..3.min(bytes.len())] == [0xEF, 0xBB, 0xBF]);
    println!("has CR  = {}", bytes.contains(&b'\r'));
    println!("has ESC = {}", bytes.contains(&0x1b));
    for line in String::from_utf8_lossy(&bytes[3.min(bytes.len())..]).lines() {
        println!("| {line}");
    }
}
