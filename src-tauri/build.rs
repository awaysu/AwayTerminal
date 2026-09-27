use std::path::{Path, PathBuf};

fn main() {
    copy_conpty_next_to_exe();
    emit_xterm_version();
    tauri_build::build()
}

/// 把**實際安裝的** xterm.js 版本編進程式（`env!("XTERM_VERSION")`，「關於」頁顯示）。
///
/// 舊版的「關於」把版本寫死成字串，結果一直印 5.5.0 而實際是 6.0.0
///（`CLAUDE.md` 的「xterm.js 版本要顯示**實際**版本」就是這條雷）。
/// 這裡改成 build 時去讀 `node_modules/@xterm/xterm/package.json`；
/// 沒有 node_modules（例如只 clone 了 src-tauri）就退回 `package.json` 的版本範圍。
fn emit_xterm_version() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let installed = root.join("node_modules/@xterm/xterm/package.json");
    let pkg = root.join("package.json");
    println!("cargo:rerun-if-changed={}", installed.display());
    println!("cargo:rerun-if-changed={}", pkg.display());

    // 只要 `"version": "x"`／`"@xterm/xterm": "^x"` 那一段，不值得為此拉 serde_json 進 build script
    let version = std::fs::read_to_string(&installed)
        .ok()
        .and_then(|s| json_str(&s, "\"version\""))
        .or_else(|| {
            std::fs::read_to_string(&pkg)
                .ok()
                .and_then(|s| json_str(&s, "\"@xterm/xterm\""))
                .map(|v| v.trim_start_matches(['^', '~']).to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=XTERM_VERSION={version}");
}

/// 從 JSON 文字裡撈出 `"<key>": "<值>"` 的值（build script 專用的土炮解析）。
fn json_str(src: &str, key: &str) -> Option<String> {
    let after = src.split_once(key)?.1;
    let after = after.split_once(':')?.1;
    let start = after.find('"')? + 1;
    let rest = &after[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
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
