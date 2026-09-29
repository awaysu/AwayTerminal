//! `cargo run --example font_probe`：印出這台機器掃到的字型家族與掃描耗時。
//! `cargo run --example font_probe -- <檔案…>`：只印這幾個字型檔裡的家族名與等寬旗標
//!（挑內建字型時用的——家族名要和 CSS `font-family` 寫的一模一樣才選得到）。
//!
//! 和其他 `*_probe` 一樣是「拿真實環境對答案」用的，不參與 `--verify`
//!（字型清單本來就每台機器不同，沒有可以寫死的期望值）。
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        for path in &args {
            match std::fs::read(path) {
                Ok(data) => {
                    let fams = awayterminal_lib::fonts::families_in(&data);
                    println!(
                        "{:>10} bytes  {}",
                        data.len(),
                        std::path::Path::new(path).file_name().unwrap_or_default().to_string_lossy()
                    );
                    for (name, mono) in fams {
                        println!("            家族「{name}」等寬={mono}");
                    }
                }
                Err(e) => println!("讀不到 {path}：{e}"),
            }
        }
        return;
    }
    let t0 = std::time::Instant::now();
    let list = awayterminal_lib::fonts::families();
    let ms = t0.elapsed().as_millis();
    let mono = list.iter().filter(|f| f.mono).count();
    println!("共 {} 個家族（等寬 {}），掃描 {} ms", list.len(), mono, ms);
    for f in list {
        println!("{} {}", if f.mono { "[等寬]" } else { "      " }, f.name);
    }
}
