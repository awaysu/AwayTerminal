use std::path::{Path, PathBuf};

fn main() {
    copy_conpty_next_to_exe();
    tauri_build::build()
}

/// 把 `resources/conpty/` 複製到 `target/<profile>/conpty/`。
///
/// `tauri.conf.json` 的 `bundle.resources` 只會把檔案放進安裝檔（MSI / NSIS），
/// **不會**放到 `target/release/` 的 exe 旁邊，所以直接跑 `cargo run` /
/// `cargo run --example pty_probe` / `npm run tauri dev` / 直接執行 release exe
/// 都會找不到 conpty.dll 而退回 Win10 內建 conhost。這裡補上那份複製，
/// 讓「exe 旁的 conpty\」在所有情境下都成立（行為同舊版 .csproj 的 build 複製）。
fn copy_conpty_next_to_exe() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = manifest.join("resources").join("conpty");
    println!("cargo:rerun-if-changed={}", src.display());
    if !src.is_dir() {
        return;
    }

    // OUT_DIR = target/<profile>/build/<pkg>-<hash>/out → 往上三層＝target/<profile>
    let Some(out_dir) = std::env::var_os("OUT_DIR").map(PathBuf::from) else {
        return;
    };
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        return;
    };
    let dst = profile_dir.join("conpty");

    if let Err(e) = copy_dir(&src, &dst) {
        println!("cargo:warning=無法複製 conpty 到 {}：{e}", dst.display());
    }
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        if !from.is_file() {
            continue;
        }
        let to = dst.join(entry.file_name());
        // 已經是同樣大小就不重複複製（避免每次 build 都動到正在使用中的 dll）
        let same = std::fs::metadata(&to)
            .ok()
            .zip(std::fs::metadata(&from).ok())
            .map(|(a, b)| a.len() == b.len())
            .unwrap_or(false);
        if !same {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
