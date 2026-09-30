//! TTL 的檔案指令——照 `ttl.cpp` 的 `TTLFile*`／`TTLFind*`／`TTLFolder*`。
//!
//! 幾個照原碼的重點：
//!
//! - **檔案控制代碼是整數**（原碼有一張 `HandlePut`／`HandleGet` 的表，`-1` ＝失敗）。
//! - `fileopen` 沒有 readonly 旗標時是「開啟或建立」（`OPEN_ALWAYS`）＋可讀可寫。
//! - `fileread`／`filereadln` 的 `result`：**1 ＝碰到檔尾**、0 ＝還有資料（和直覺相反，照原碼）。
//! - 相對路徑是相對於**巨集自己的「目前目錄」**（原碼的 `CurrentDir`／`GetAbsPath`），
//!   不是行程的工作目錄；`getdir`／`setdir`／`changedir` 改的就是它。
//!   ⚠️ 我們**只改直譯器自己的**目前目錄，不呼叫 `SetCurrentDirectory`——那會影響整個
//!   行程（我們是多分頁的 app，別的分頁正在用）。
//! - `filereadln` 的換行處理照原碼：`CR LF`／`CR`／`LF` 都算一行結束，單獨的 `CR` 後面
//!   如果不是 `LF` 會把檔案指標退回去一格。

use std::io::{Read, Seek, SeekFrom, Write};

use super::error::{Err, Result};
use super::exec::Interp;
use super::vars::VarType;
use super::words::Word;

/// 一個開著的檔案（原碼的 `HANDLE` 表）。
pub struct OpenFile {
    pub file: std::fs::File,
    /// `filemarkptr` 記下的位置（`fileseekback` 用）。
    pub mark: u64,
    pub path: std::path::PathBuf,
}

impl Interp {
    /// 檔案類指令的分派。
    pub(super) fn dispatch_files(&mut self, w: Word) -> Result<()> {
        match w {
            Word::FileOpen => self.cmd_fileopen(),
            Word::FileClose => self.cmd_fileclose(),
            Word::FileRead => self.cmd_fileread(),
            Word::FileReadln => self.cmd_filereadln(),
            Word::FileWrite => self.cmd_filewrite(false),
            Word::FileWriteLn => self.cmd_filewrite(true),
            Word::FileSeek => self.cmd_fileseek(),
            Word::FileSeekBack => self.cmd_fileseekback(),
            Word::FileMarkPtr => self.cmd_filemarkptr(),
            Word::FileStrSeek => self.cmd_filestrseek(true),
            Word::FileStrSeek2 => self.cmd_filestrseek(false),
            Word::FileTruncate => self.cmd_filetruncate(),
            Word::FileSearch => self.cmd_filesearch(),
            Word::FileCreate => self.cmd_filecreate(),
            Word::FileDelete => self.cmd_filedelete(),
            Word::FileRename => self.cmd_filerename(),
            Word::FileCopy => self.cmd_filecopy(),
            Word::FileStat => self.cmd_filestat(),
            Word::FolderCreate => self.cmd_foldercreate(),
            Word::FolderDelete => self.cmd_folderdelete(),
            Word::FolderSearch => self.cmd_foldersearch(),
            Word::FindFirst => self.cmd_findfirst(),
            Word::FindNext => self.cmd_findnext(),
            Word::FindClose => self.cmd_findclose(),
            Word::GetDir => self.cmd_getdir(),
            Word::SetDir | Word::ChangeDir => self.cmd_setdir(),
            _ => Err(Err::NotSupported),
        }
    }

    /// 相對路徑 → 絕對路徑（相對於巨集的目前目錄，同原碼 `GetAbsPath`）。
    pub(super) fn abs_path(&self, name: &[u8]) -> std::path::PathBuf {
        let s = String::from_utf8_lossy(name).into_owned();
        let p = std::path::PathBuf::from(&s);
        if p.is_absolute() {
            p
        } else {
            self.current_dir().join(p)
        }
    }

    // ---------------------------------------------------------------- 開關讀寫

    /// `fileopen <整數變數> <檔名> <append> [readonly]`。開不起來 → 變數是 -1。
    fn cmd_fileopen(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::Integer)?;
        let name = self.str_val()?;
        let append = self.int_val()?;
        let readonly = if self.parameter_given() {
            self.int_val()?
        } else {
            0
        };
        self.end_of_args()?;
        if name.is_empty() {
            return Err(Err::Syntax);
        }
        let path = self.abs_path(&name);
        let mut opts = std::fs::OpenOptions::new();
        if readonly != 0 {
            opts.read(true);
        } else {
            // 原碼的 OPEN_ALWAYS：有就開、沒有就建
            opts.read(true).write(true).create(true);
        }
        let file = match opts.open(&path) {
            Ok(f) => f,
            Err(_) => {
                self.set_int_ref(&target, -1)?;
                return Ok(());
            }
        };
        let mut of = OpenFile {
            file,
            mark: 0,
            path,
        };
        if append != 0 {
            let _ = of.file.seek(SeekFrom::End(0));
        }
        let h = self.files_put(of);
        self.set_int_ref(&target, h)
    }

    /// `fileclose <控制代碼>`。
    fn cmd_fileclose(&mut self) -> Result<()> {
        let h = self.int_val()?;
        self.end_of_args()?;
        self.files_take(h);
        Ok(())
    }

    /// `fileread <控制代碼> <幾個位元組> <字串變數>`。`result` ＝ 1 碰到檔尾。
    fn cmd_fileread(&mut self) -> Result<()> {
        let h = self.int_val()?;
        let n = self.int_val()?;
        let target = self.var_ref(VarType::String)?;
        self.end_of_args()?;
        if n < 1 || n > super::lex::MAX_STR_LEN as i32 - 1 {
            return Err(Err::Syntax);
        }
        let mut buf = vec![0u8; n as usize];
        let (got, eof) = self
            .files_with(h, |f| {
            let mut total = 0usize;
            let mut eof = false;
            while total < buf.len() {
                match f.file.read(&mut buf[total..]) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(k) => total += k,
                    Err(_) => {
                        eof = true;
                        break;
                    }
                }
            }
                (total, eof)
            })
            // 控制代碼不對 → 當成「什麼都沒讀到＋檔尾」（同原碼：無效 handle 不是錯誤）
            .unwrap_or((0, true));
        buf.truncate(got);
        self.vars.set_result(i32::from(eof));
        self.set_str_ref(&target, &buf)
    }

    /// `filereadln <控制代碼> <字串變數>`：讀一行。`result` ＝ 1 ＝**什麼都沒讀到**（檔尾）。
    fn cmd_filereadln(&mut self) -> Result<()> {
        let h = self.int_val()?;
        let target = self.var_ref(VarType::String)?;
        self.end_of_args()?;

        let (line, end_file) = match self.files_with(h, read_line) {
            Some(v) => v,
            None => (Vec::new(), true),
        };
        self.vars.set_result(i32::from(end_file));
        self.set_str_ref(&target, &line)
    }

    /// `filewrite <控制代碼> <參數…>`／`filewriteln`（後者補 `CR LF`）。
    ///
    /// 參數列和 `send` 一樣：字串照寫、整數寫一個位元組。
    fn cmd_filewrite(&mut self, newline: bool) -> Result<()> {
        let h = self.int_val()?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            if let Some(s) = self.lex_string()? {
                out.extend_from_slice(&s);
                continue;
            }
            match self.try_expression()? {
                Some(super::expr::Val::Str(s)) => out.extend_from_slice(&s),
                Some(super::expr::Val::Int(v)) => out.push((v & 0xff) as u8),
                None => break,
            }
        }
        if newline {
            out.extend_from_slice(b"\r\n");
        }
        self.files_with(h, |f| {
            let _ = f.file.write_all(&out);
            let _ = f.file.flush();
        });
        Ok(())
    }

    /// `fileseek <控制代碼> <位移> <起點>`（0＝開頭、1＝目前、2＝結尾，同 C 的 `SEEK_*`）。
    fn cmd_fileseek(&mut self) -> Result<()> {
        let h = self.int_val()?;
        let offset = self.int_val()?;
        let origin = self.int_val()?;
        self.end_of_args()?;
        self.files_with(h, |f| {
            let from = match origin {
                1 => SeekFrom::Current(offset as i64),
                2 => SeekFrom::End(offset as i64),
                _ => SeekFrom::Start(offset.max(0) as u64),
            };
            let _ = f.file.seek(from);
        });
        Ok(())
    }

    /// `filemarkptr <控制代碼>`：記下目前位置（`fileseekback` 會回到這裡）。
    fn cmd_filemarkptr(&mut self) -> Result<()> {
        let h = self.int_val()?;
        self.end_of_args()?;
        self.files_with(h, |f| {
            f.mark = f.file.stream_position().unwrap_or(0);
        });
        Ok(())
    }

    /// `fileseekback <控制代碼>`：回到 `filemarkptr` 記下的位置。
    fn cmd_fileseekback(&mut self) -> Result<()> {
        let h = self.int_val()?;
        self.end_of_args()?;
        self.files_with(h, |f| {
            let mark = f.mark;
            let _ = f.file.seek(SeekFrom::Start(mark));
        });
        Ok(())
    }

    /// `filestrseek <控制代碼> <字串>`：從目前位置往後找。
    /// 找到 → `result=1`，檔案指標停在**字串之後**；找不到 → `result=0`，指標不動。
    ///
    /// `filestrseek2` 是往**前**找，照原碼逐位元組倒著讀（見 [`strseek_backward`]）：
    /// **含目前這個位元組**，找到後指標停在**命中位置的前一個位元組**（命中在檔頭時是 0）。
    fn cmd_filestrseek(&mut self, forward: bool) -> Result<()> {
        let h = self.int_val()?;
        let needle = self.str_val()?;
        self.end_of_args()?;
        if needle.is_empty() {
            self.vars.set_result(0);
            return Ok(());
        }
        let found = self
            .files_with(h, |f| {
                let start = f.file.stream_position().unwrap_or(0);
                let mut data = Vec::new();
                if f.file.seek(SeekFrom::Start(0)).is_err() || f.file.read_to_end(&mut data).is_err()
                {
                    return false;
                }
                let hit = if forward {
                    let from = start as usize;
                    if from > data.len() {
                        None
                    } else {
                        find_bytes(&data[from..], &needle).map(|i| from + i)
                    }
                } else {
                    // 往回找：找到時的指標位置由原碼的讀法決定，直接回傳「要停在哪」
                    return match strseek_backward(&data, start, &needle) {
                        Some(p) => {
                            let _ = f.file.seek(SeekFrom::Start(p));
                            true
                        }
                        None => {
                            let _ = f.file.seek(SeekFrom::Start(start));
                            false
                        }
                    };
                };
                match hit {
                    Some(i) => {
                        let _ = f.file.seek(SeekFrom::Start((i + needle.len()) as u64));
                        true
                    }
                    None => {
                        let _ = f.file.seek(SeekFrom::Start(start));
                        false
                    }
                }
            })
            .unwrap_or(false);
        self.vars.set_result(i32::from(found));
        Ok(())
    }

    /// `filetruncate <檔名> <長度>`。
    fn cmd_filetruncate(&mut self) -> Result<()> {
        let name = self.str_val()?;
        let len = self.int_val()?;
        self.end_of_args()?;
        let path = self.abs_path(&name);
        let ok = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .and_then(|f| f.set_len(len.max(0) as u64))
            .is_ok();
        self.vars.set_result(i32::from(ok));
        Ok(())
    }

    // ---------------------------------------------------------------- 檔案操作

    /// `filesearch <檔名>` → `result` ＝ 1 存在、0 不存在。
    fn cmd_filesearch(&mut self) -> Result<()> {
        let name = self.str_val()?;
        self.end_of_args()?;
        let exists = self.abs_path(&name).is_file();
        self.vars.set_result(i32::from(exists));
        Ok(())
    }

    /// `filecreate <整數變數> <檔名>`：建一個新檔（已存在會被清空），失敗時變數是 -1。
    fn cmd_filecreate(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::Integer)?;
        let name = self.str_val()?;
        self.end_of_args()?;
        let path = self.abs_path(&name);
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
        {
            Ok(file) => {
                let h = self.files_put(OpenFile {
                    file,
                    mark: 0,
                    path,
                });
                self.set_int_ref(&target, h)
            }
            Err(_) => self.set_int_ref(&target, -1),
        }
    }

    fn cmd_filedelete(&mut self) -> Result<()> {
        let name = self.str_val()?;
        self.end_of_args()?;
        let ok = std::fs::remove_file(self.abs_path(&name)).is_ok();
        self.vars.set_result(i32::from(ok));
        Ok(())
    }

    fn cmd_filerename(&mut self) -> Result<()> {
        let from = self.str_val()?;
        let to = self.str_val()?;
        self.end_of_args()?;
        let ok = std::fs::rename(self.abs_path(&from), self.abs_path(&to)).is_ok();
        self.vars.set_result(i32::from(ok));
        Ok(())
    }

    fn cmd_filecopy(&mut self) -> Result<()> {
        let from = self.str_val()?;
        let to = self.str_val()?;
        self.end_of_args()?;
        let ok = std::fs::copy(self.abs_path(&from), self.abs_path(&to)).is_ok();
        self.vars.set_result(i32::from(ok));
        Ok(())
    }

    /// `filestat <檔名> <整數變數（大小）> [字串變數（時間）]`。
    ///
    /// 原碼還會回傳更多欄位（屬性、時間格式可選）；我們給**大小**與
    /// `yyyy-mm-dd hh:mm:ss` 的修改時間，其餘沒做（見 `docs/TTL.md`）。
    /// `result` ＝ 0 成功、1 失敗（同原碼那組指令的慣例）。
    fn cmd_filestat(&mut self) -> Result<()> {
        let name = self.str_val()?;
        let size_var = self.var_ref(VarType::Integer)?;
        let time_var = if self.parameter_given() {
            Some(self.var_ref(VarType::String)?)
        } else {
            None
        };
        // 原碼之後還有「時間格式」參數，讀掉不用
        while self.parameter_given() {
            let _ = self.str_val()?;
        }
        self.end_of_args()?;

        let path = self.abs_path(&name);
        match std::fs::metadata(&path) {
            Ok(md) => {
                self.set_int_ref(&size_var, md.len().min(i32::MAX as u64) as i32)?;
                if let Some(tv) = time_var {
                    let text = md
                        .modified()
                        .map(|t| {
                            chrono::DateTime::<chrono::Local>::from(t)
                                .format("%Y-%m-%d %H:%M:%S")
                                .to_string()
                        })
                        .unwrap_or_default();
                    self.set_str_ref(&tv, text.as_bytes())?;
                }
                self.vars.set_result(0);
                Ok(())
            }
            Err(_) => {
                self.set_int_ref(&size_var, 0)?;
                self.vars.set_result(1);
                Ok(())
            }
        }
    }

    // ---------------------------------------------------------------- 資料夾

    fn cmd_foldercreate(&mut self) -> Result<()> {
        let name = self.str_val()?;
        self.end_of_args()?;
        let ok = std::fs::create_dir_all(self.abs_path(&name)).is_ok();
        self.vars.set_result(i32::from(ok));
        Ok(())
    }

    fn cmd_folderdelete(&mut self) -> Result<()> {
        let name = self.str_val()?;
        self.end_of_args()?;
        // 原碼用 RemoveDirectory：**只能刪空的**
        let ok = std::fs::remove_dir(self.abs_path(&name)).is_ok();
        self.vars.set_result(i32::from(ok));
        Ok(())
    }

    fn cmd_foldersearch(&mut self) -> Result<()> {
        let name = self.str_val()?;
        self.end_of_args()?;
        let exists = self.abs_path(&name).is_dir();
        self.vars.set_result(i32::from(exists));
        Ok(())
    }

    /// `findfirst <整數變數> <樣式> <字串變數>`：找第一個符合的檔名。
    ///
    /// 樣式只支援 `*` 與 `?`（原碼是 Windows 的 `FindFirstFile`，語意一樣）。
    /// 找不到 → 控制代碼 -1、`result=0`。
    fn cmd_findfirst(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::Integer)?;
        let pattern = self.str_val()?;
        let name_var = self.var_ref(VarType::String)?;
        self.end_of_args()?;

        let pat_path = self.abs_path(&pattern);
        let dir = pat_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let pat = pat_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        let mut names: Vec<String> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if wildcard_match(&pat, &n) {
                    names.push(n);
                }
            }
        }
        names.sort();
        if names.is_empty() {
            self.set_int_ref(&target, -1)?;
            self.set_str_ref(&name_var, b"")?;
            self.vars.set_result(0);
            return Ok(());
        }
        let first = names.remove(0);
        let h = self.finds_put(names);
        self.set_int_ref(&target, h)?;
        self.set_str_ref(&name_var, first.as_bytes())?;
        self.vars.set_result(1);
        Ok(())
    }

    /// `findnext <控制代碼> <字串變數>` → `result` ＝ 1 還有、0 沒有了。
    fn cmd_findnext(&mut self) -> Result<()> {
        let h = self.int_val()?;
        let name_var = self.var_ref(VarType::String)?;
        self.end_of_args()?;
        match self.finds_next(h) {
            Some(n) => {
                self.set_str_ref(&name_var, n.as_bytes())?;
                self.vars.set_result(1);
            }
            None => {
                self.set_str_ref(&name_var, b"")?;
                self.vars.set_result(0);
            }
        }
        Ok(())
    }

    fn cmd_findclose(&mut self) -> Result<()> {
        let h = self.int_val()?;
        self.end_of_args()?;
        self.finds_take(h);
        Ok(())
    }

    /// `getdir <字串變數>`：巨集的目前目錄。
    fn cmd_getdir(&mut self) -> Result<()> {
        let target = self.var_ref(VarType::String)?;
        self.end_of_args()?;
        let dir = self.current_dir().to_string_lossy().into_owned();
        self.set_str_ref(&target, dir.as_bytes())
    }

    /// `setdir <路徑>`／`changedir <路徑>`：改巨集的目前目錄。
    ///
    /// ⚠️ **不動行程的工作目錄**（原碼是 `SetCurrentDirectory`，會影響整個行程；
    /// 我們是多分頁的 app，別的分頁正在用）。
    fn cmd_setdir(&mut self) -> Result<()> {
        let name = self.str_val()?;
        self.end_of_args()?;
        let path = self.abs_path(&name);
        if path.is_dir() {
            self.set_current_dir(path);
            self.vars.set_result(1);
        } else {
            self.vars.set_result(0);
        }
        Ok(())
    }
}

/// 讀一行：回 (那一行不含換行, 是不是什麼都沒讀到)。
///
/// 換行處理照原碼 `TTLFileReadln`：`CR LF`／`CR`／`LF` 都算結束；
/// 單獨的 `CR` 後面不是 `LF` 時要把指標退回去一格。
fn read_line(f: &mut OpenFile) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut got_any = false;
    let mut b = [0u8; 1];
    loop {
        match f.file.read(&mut b) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                got_any = true;
                match b[0] {
                    0x0d => {
                        // 看下一個是不是 LF；不是就退回去
                        match f.file.read(&mut b) {
                            Ok(1) if b[0] != 0x0a => {
                                let _ = f.file.seek(SeekFrom::Current(-1));
                            }
                            _ => {}
                        }
                        break;
                    }
                    0x0a => break,
                    c => {
                        if out.len() < super::lex::MAX_STR_LEN - 1 {
                            out.push(c);
                        }
                    }
                }
            }
        }
    }
    (out, !got_any)
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// `TTLFileStrSeek2` 逐步照抄：從目前指標 `start` **含這個位元組**開始倒著讀
/// （讀一個、`llseek(-2)`），用「從字串尾巴往前對」的簡單比對（對不上只重試最後一個字元，
/// 不做 KMP 退回——原碼就是這樣）。回 `Some(找到後的指標)`：命中起點的前一個位元組；
/// 命中在檔頭時 `llseek` 會失敗，原碼把它調成 0。
///
/// 兩個原碼的細節也一起照抄：指標在檔尾（EOF）時第一次讀不到，`-2` 之後**最後一個位元組
/// 不會被看到**；`start == 0` 只看第 0 個位元組。
fn strseek_backward(data: &[u8], start: u64, needle: &[u8]) -> Option<u64> {
    let len = needle.len();
    if len == 0 {
        return None;
    }
    let mut p = start.min(i64::MAX as u64) as i64; // 真正的檔案指標
    // 指標被 `fileseek` 移到檔尾之後很遠：那一段每圈都讀不到、只是 -2，一次跳過
    // （不然 `fileseek fh 2000000000` 之後這裡要空轉十億圈）
    let dlen = data.len() as i64;
    if p >= dlen + 2 {
        p -= (p - dlen) / 2 * 2;
    }
    let mut pos2 = p; // 原碼的 pos2（llseek 的回傳值，失敗是 -1）
    let mut i = 0usize;
    loop {
        let last = pos2 <= 0;
        // win16_lread(FH, &b, 1)
        let c = if p >= 0 && (p as usize) < data.len() {
            let b = data[p as usize];
            p += 1;
            Some(b)
        } else {
            None
        };
        // win16_llseek(FH, -2, 1)：跑到負的位置會失敗（INVALID_SET_FILE_POINTER），指標不動
        if p - 2 < 0 {
            pos2 = -1;
        } else {
            p -= 2;
            pos2 = p;
        }
        if let Some(b) = c {
            if b == needle[len - 1 - i] {
                i += 1;
            } else if i > 0 {
                i = 0;
                if b == needle[len - 1] {
                    i = 1;
                }
            }
        }
        if last || i == len {
            break;
        }
    }
    if i != len {
        return None;
    }
    Some(if pos2 == -1 { 0 } else { p as u64 })
}

/// `*` 與 `?` 的萬用字元比對（Windows 的 `FindFirstFile` 語意，大小寫不敏感）。
pub fn wildcard_match(pat: &str, name: &str) -> bool {
    let p: Vec<char> = pat.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    fn go(p: &[char], n: &[char]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some('*') => (0..=n.len()).any(|i| go(&p[1..], &n[i..])),
            Some('?') => !n.is_empty() && go(&p[1..], &n[1..]),
            Some(c) => !n.is_empty() && n[0] == *c && go(&p[1..], &n[1..]),
        }
    }
    go(&p, &n)
}

#[cfg(test)]
mod tests {
    use super::super::exec::Interp;
    use super::*;

    /// 在暫存目錄裡跑一段巨集（相對路徑會落在那裡）。
    fn run_in_temp(src: &str) -> (super::super::vars::Vars, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "away-ttl-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut it = Interp::from_text("t.ttl", src).unwrap();
        it.set_current_dir(dir.clone());
        it.run(100_000).expect("巨集應該跑得完");
        (std::mem::take(&mut it.vars), dir)
    }

    /// `filestrseek2` 照原碼：含目前位元組、找到後停在命中起點的前一個位元組。
    #[test]
    fn filestrseek2_matches_original() {
        //           0123456789...
        let data = b"abcXYZdefXYZghi";
        // 指標在第二個 Z 上：含目前位元組 → 命中第二個 XYZ（9），停在 8
        assert_eq!(strseek_backward(data, 11, b"XYZ"), Some(8));
        // 指標在第二個 Y 上：第二個湊不齊 → 命中第一個（3），停在 2
        assert_eq!(strseek_backward(data, 10, b"XYZ"), Some(2));
        // 命中在檔頭：llseek 失敗 → 調成 0
        assert_eq!(strseek_backward(b"XYZabc", 2, b"XYZ"), Some(0));
        assert_eq!(strseek_backward(b"XYZabc", 5, b"XYZ"), Some(0));
        // 找不到
        assert_eq!(strseek_backward(data, 14, b"QQ"), None);
        assert_eq!(strseek_backward(data, 0, b"XYZ"), None);
        // 指標在 EOF：第一次讀不到、-2 → 最後一個位元組看不到（原碼的行為）
        assert_eq!(strseek_backward(b"abXYZ", 5, b"XYZ"), None);
        assert_eq!(strseek_backward(b"abXYZ!", 6, b"XYZ"), Some(1));
        // 指標遠在檔尾之後：不能空轉
        assert_eq!(strseek_backward(b"XYZ", 1_000_000_000, b"XYZ"), Some(0));
    }

    /// 巨集層：`filestrseek2` 找到之後 `filereadln` 從「命中前一個位元組」開始讀。
    #[test]
    fn filestrseek2_then_read() {
        let (v, dir) = run_in_temp(
            "fileopen fh 'a.txt' 0\nfilewrite fh 'abcXYZdef'\nfileclose fh\n\
             fileopen fh 'a.txt' 0\nfileseek fh 0 2\nfilestrseek2 fh 'XYZ'\nr = result\n\
             filereadln fh s\nfileclose fh",
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(v.int_of("r"), Some(1));
        assert_eq!(v.str_of("s").unwrap(), b"cXYZdef");
    }

    /// 寫檔 → 讀回來（`filewriteln` 補 CR LF、`filereadln` 去掉）。
    #[test]
    fn write_then_read_lines() {
        let (v, dir) = run_in_temp(
            "fileopen fh 'a.txt' 0\nfilewriteln fh 'one'\nfilewriteln fh 'two'\nfileclose fh\n\
             fileopen fh 'a.txt' 0\nfilereadln fh s1\nr1 = result\nfilereadln fh s2\n\
             filereadln fh s3\nr3 = result\nfileclose fh",
        );
        assert_eq!(v.str_of("s1").unwrap(), b"one");
        assert_eq!(v.str_of("s2").unwrap(), b"two");
        assert_eq!(v.int_of("r1"), Some(0), "還有資料時 result＝0");
        assert_eq!(v.str_of("s3").unwrap(), b"");
        assert_eq!(v.int_of("r3"), Some(1), "檔尾時 result＝1（照原碼）");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `fileopen` 開不起來（readonly 但檔案不存在）→ 控制代碼 -1。
    #[test]
    fn fileopen_failure_gives_minus_one() {
        let (v, dir) = run_in_temp("fileopen fh 'nosuch.txt' 0 1");
        assert_eq!(v.int_of("fh"), Some(-1));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `fileread` 讀固定位元組數；`result` ＝ 1 表示碰到檔尾。
    #[test]
    fn fileread_counts_bytes() {
        let (v, dir) = run_in_temp(
            "fileopen fh 'b.txt' 0\nfilewrite fh 'abcdef'\nfileclose fh\n\
             fileopen fh 'b.txt' 0\nfileread fh 3 s1\nr1 = result\nfileread fh 99 s2\nr2 = result\nfileclose fh",
        );
        assert_eq!(v.str_of("s1").unwrap(), b"abc");
        assert_eq!(v.int_of("r1"), Some(0));
        assert_eq!(v.str_of("s2").unwrap(), b"def");
        assert_eq!(v.int_of("r2"), Some(1), "讀不滿＝碰到檔尾");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `append` 旗標：開檔時跳到結尾。
    #[test]
    fn append_flag_seeks_to_end() {
        let (v, dir) = run_in_temp(
            "fileopen fh 'c.txt' 0\nfilewrite fh 'aa'\nfileclose fh\n\
             fileopen fh 'c.txt' 1\nfilewrite fh 'bb'\nfileclose fh\n\
             fileopen fh 'c.txt' 0\nfileread fh 10 s\nfileclose fh",
        );
        assert_eq!(v.str_of("s").unwrap(), b"aabb");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `fileseek`／`filemarkptr`／`fileseekback`。
    #[test]
    fn seek_and_mark() {
        let (v, dir) = run_in_temp(
            "fileopen fh 'd.txt' 0\nfilewrite fh '0123456789'\nfileclose fh\n\
             fileopen fh 'd.txt' 0\nfileseek fh 3 0\nfilemarkptr fh\nfileread fh 2 s1\n\
             fileseekback fh\nfileread fh 2 s2\nfileclose fh",
        );
        assert_eq!(v.str_of("s1").unwrap(), b"34");
        assert_eq!(v.str_of("s2").unwrap(), b"34", "回到 mark 的位置");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `filestrseek`：找到之後指標停在字串之後。
    #[test]
    fn filestrseek_positions_after_match() {
        let (v, dir) = run_in_temp(
            "fileopen fh 'e.txt' 0\nfilewrite fh 'hello world'\nfileclose fh\n\
             fileopen fh 'e.txt' 0\nfilestrseek fh 'wor'\nr1 = result\nfileread fh 2 s1\n\
             filestrseek fh 'zzz'\nr2 = result\nfileclose fh",
        );
        assert_eq!(v.int_of("r1"), Some(1));
        assert_eq!(v.str_of("s1").unwrap(), b"ld");
        assert_eq!(v.int_of("r2"), Some(0));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 建立／搜尋／改名／複製／刪除。
    #[test]
    fn file_operations() {
        let (v, dir) = run_in_temp(
            "fileopen fh 'f1.txt' 0\nfilewrite fh 'x'\nfileclose fh\n\
             filesearch 'f1.txt'\nr1 = result\n\
             filecopy 'f1.txt' 'f2.txt'\nr2 = result\nfilesearch 'f2.txt'\nr3 = result\n\
             filerename 'f2.txt' 'f3.txt'\nr4 = result\nfilesearch 'f3.txt'\nr5 = result\n\
             filedelete 'f3.txt'\nr6 = result\nfilesearch 'f3.txt'\nr7 = result\n\
             filesearch 'nosuch.txt'\nr8 = result",
        );
        for (k, want) in [
            ("r1", 1),
            ("r2", 1),
            ("r3", 1),
            ("r4", 1),
            ("r5", 1),
            ("r6", 1),
            ("r7", 0),
            ("r8", 0),
        ] {
            assert_eq!(v.int_of(k), Some(want), "{k}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `filestat`：大小與時間。
    #[test]
    fn filestat_reports_size() {
        let (v, dir) = run_in_temp(
            "fileopen fh 'g.txt' 0\nfilewrite fh '12345'\nfileclose fh\n\
             filestat 'g.txt' sz tm\nr = result\nfilestat 'nosuch' sz2\nr2 = result",
        );
        assert_eq!(v.int_of("sz"), Some(5));
        assert_eq!(v.int_of("r"), Some(0), "成功是 0（照原碼那組的慣例）");
        assert!(v.str_of("tm").unwrap().len() >= 19, "時間字串");
        assert_eq!(v.int_of("r2"), Some(1), "失敗是 1");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 資料夾與 `findfirst`／`findnext`。
    #[test]
    fn folders_and_find() {
        let (v, dir) = run_in_temp(
            "foldercreate 'sub'\nr1 = result\nfoldersearch 'sub'\nr2 = result\n\
             fileopen fh 'x1.dat' 0\nfileclose fh\nfileopen fh 'x2.dat' 0\nfileclose fh\n\
             findfirst fh2 '*.dat' n1\nr3 = result\nfindnext fh2 n2\nr4 = result\n\
             findnext fh2 n3\nr5 = result\nfindclose fh2\n\
             folderdelete 'sub'\nr6 = result",
        );
        assert_eq!(v.int_of("r1"), Some(1));
        assert_eq!(v.int_of("r2"), Some(1));
        assert_eq!(v.int_of("r3"), Some(1));
        assert_eq!(v.str_of("n1").unwrap(), b"x1.dat");
        assert_eq!(v.int_of("r4"), Some(1));
        assert_eq!(v.str_of("n2").unwrap(), b"x2.dat");
        assert_eq!(v.int_of("r5"), Some(0), "沒有第三個了");
        assert_eq!(v.int_of("r6"), Some(1));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `getdir`／`setdir`：改的是**巨集自己的**目前目錄。
    #[test]
    fn getdir_and_setdir() {
        let (v, dir) = run_in_temp("foldercreate 'sub2'\ngetdir d1\nsetdir 'sub2'\nr = result\ngetdir d2");
        assert_eq!(v.int_of("r"), Some(1));
        let d1 = String::from_utf8_lossy(v.str_of("d1").unwrap()).into_owned();
        let d2 = String::from_utf8_lossy(v.str_of("d2").unwrap()).into_owned();
        assert!(d2.ends_with("sub2"), "d2={d2}");
        assert_ne!(d1, d2);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 萬用字元比對。
    #[test]
    fn wildcards() {
        assert!(wildcard_match("*.txt", "a.txt"));
        assert!(wildcard_match("*.TXT", "a.txt"), "大小寫不敏感");
        assert!(wildcard_match("a?.dat", "ab.dat"));
        assert!(!wildcard_match("a?.dat", "abc.dat"));
        assert!(wildcard_match("*", "anything"));
        assert!(!wildcard_match("*.txt", "a.dat"));
    }

    /// 讀一行的換行處理：`CR LF`／`CR`／`LF` 都算一行。
    #[test]
    fn line_endings() {
        let (v, dir) = run_in_temp(
            "fileopen fh 'h.txt' 0\nfilewrite fh 'a' #13 #10 'b' #13 'c' #10 'd'\nfileclose fh\n\
             fileopen fh 'h.txt' 0\nfilereadln fh s1\nfilereadln fh s2\nfilereadln fh s3\n\
             filereadln fh s4\nr4 = result\nfileclose fh",
        );
        assert_eq!(v.str_of("s1").unwrap(), b"a");
        assert_eq!(v.str_of("s2").unwrap(), b"b", "單獨的 CR 也算一行");
        assert_eq!(v.str_of("s3").unwrap(), b"c");
        assert_eq!(v.str_of("s4").unwrap(), b"d");
        assert_eq!(v.int_of("r4"), Some(0), "最後一行沒有換行但有內容");
        let _ = std::fs::remove_dir_all(dir);
    }
}
