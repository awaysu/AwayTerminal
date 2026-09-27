//! TTL 的 CRC 與 checksum（`crc16`／`crc32`／`checksum8`／`checksum16`／`checksum32`
//! 與各自的 `*file` 版本）。
//!
//! **逐段照原碼**：`reference/teraterm/teraterm/ttpmacro/ttl.cpp` 的
//! `checksum8`／`checksum16`／`checksum32`／`crc16`／`crc32`
//! （原碼註解指向 <http://oku.edu.mie-u.ac.jp/~okumura/algo/>）。
//!
//! # 這是哪一種 CRC
//!
//! | | 多項式（反射後） | 初值 | 最後 XOR | 常見名稱 |
//! |---|---|---|---|---|
//! | `crc16` | `0x8408`（＝`0x1021` 左右反轉） | `0xFFFF` | `0xFFFF` | **CRC-16/X-25**（＝IBM-SDLC、ISO-HDLC） |
//! | `crc32` | `0xEDB88320`（＝`0x04C11DB7` 左右反轉） | `0xFFFFFFFF` | `0xFFFFFFFF` | **CRC-32**（zlib／PKZIP 的那個） |
//!
//! ⚠️ `crc16` **不是**常被叫做「CRC-16-CCITT」的那個 `0x1021`／init `0xFFFF`／不反射、
//! 不最後 XOR 的變體（CCITT-FALSE）。原碼的文件寫「CRC-16-CCITT」但實作是反射＋最後
//! XOR，所以答案和 CCITT-FALSE 不一樣。**照實作，不照文件的名字**——舊的 `.ttl` 檔
//! 期待的是實作的行為。
//!
//! # 位元組還是字元
//!
//! 原碼吃的是 `unsigned char[]` ＋ `strlen`，也就是**設定的編碼下的位元組**。
//! 我們的字串本來就是 `Vec<u8>`（TTL 的字串一律當位元組處理，見 `vars.rs`），
//! 所以直接餵進去就對了，不必也不應該先轉成 UTF-8 字元。

/// `checksum8`／`checksum16`／`checksum32` 共用的加總（原碼三個函式只差最後的遮罩）。
fn sum(data: &[u8]) -> u64 {
    data.iter().map(|&b| b as u64).sum()
}

pub fn checksum8(data: &[u8]) -> u32 {
    (sum(data) & 0xFF) as u32
}

pub fn checksum16(data: &[u8]) -> u32 {
    (sum(data) & 0xFFFF) as u32
}

pub fn checksum32(data: &[u8]) -> u32 {
    (sum(data) & 0xFFFF_FFFF) as u32
}

/// CRC-16/X-25。原碼 `crc16()`。
pub fn crc16(data: &[u8]) -> u32 {
    const POLY: u16 = 0x8408; // 0x1021 左右反轉
    let mut r: u16 = 0xFFFF;
    for &b in data {
        r ^= b as u16;
        for _ in 0..8 {
            r = if r & 1 != 0 { (r >> 1) ^ POLY } else { r >> 1 };
        }
    }
    (r ^ 0xFFFF) as u32
}

/// CRC-32（zlib）。原碼 `crc32()`。
pub fn crc32(data: &[u8]) -> u32 {
    const POLY: u32 = 0xEDB8_8320; // 0x04C11DB7 左右反轉
    let mut r: u32 = 0xFFFF_FFFF;
    for &b in data {
        r ^= b as u32;
        for _ in 0..8 {
            r = if r & 1 != 0 { (r >> 1) ^ POLY } else { r >> 1 };
        }
    }
    r ^ 0xFFFF_FFFF
}

/// 要算哪一種（`cmds.rs` 的五個指令共用一條路，同原碼的 `TTLDoChecksum`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Sum8,
    Sum16,
    Sum32,
    Crc16,
    Crc32,
}

impl Kind {
    pub fn apply(self, data: &[u8]) -> u32 {
        match self {
            Kind::Sum8 => checksum8(data),
            Kind::Sum16 => checksum16(data),
            Kind::Sum32 => checksum32(data),
            Kind::Crc16 => crc16(data),
            Kind::Crc32 => crc32(data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `"123456789"` 是 CRC 的標準檢查向量（CRC catalogue 的 `check` 欄位）。
    ///
    /// * CRC-32（zlib）＝`0xCBF43926`
    /// * CRC-16/X-25 ＝`0x906E`
    ///
    /// 這兩個值是**外部已知答案**，不是我們自己跑出來的——所以它們真的能證明
    /// 多項式、初值、反射、最後 XOR 四件事都對。
    #[test]
    fn matches_the_standard_check_vectors() {
        let v = b"123456789";
        assert_eq!(crc32(v), 0xCBF4_3926, "CRC-32 的標準檢查值");
        assert_eq!(crc16(v), 0x906E, "CRC-16/X-25 的標準檢查值");
    }

    /// checksum 就是位元組相加再遮罩。`"123456789"` 的 ASCII 和＝0x31+…+0x39＝477＝0x1DD。
    #[test]
    fn checksums_are_masked_byte_sums() {
        let v = b"123456789";
        assert_eq!(checksum32(v), 0x1DD);
        assert_eq!(checksum16(v), 0x1DD);
        assert_eq!(checksum8(v), 0xDD, "8 位元要被截掉高位");
    }

    /// 空字串：CRC 的定義是「初值 XOR 最後 XOR」＝0；checksum 是 0。
    ///
    /// 註：原碼的 `TTLDoChecksum` 遇到空字串會**直接 return、不寫變數**
    /// （`if (Str[0]==0) return Err;`），那一段行為在 `cmds.rs` 處理，
    /// 這裡的函式本身照數學定義。
    #[test]
    fn empty_input() {
        assert_eq!(crc16(b""), 0);
        assert_eq!(crc32(b""), 0);
        assert_eq!(checksum8(b""), 0);
    }

    /// 高位元組（>0x7F）要當成 unsigned 處理——原碼是 `c[i] & 0xFF`。
    /// 用 `i8` 思考的話 0xFF 會變 -1，加總就錯了。
    #[test]
    fn high_bytes_are_unsigned() {
        assert_eq!(checksum32(&[0xFF, 0xFF]), 0x1FE);
        assert_eq!(checksum8(&[0xFF, 0xFF]), 0xFE);
        // 二進位資料也要算得出來（不是只吃 ASCII）
        assert_eq!(crc32(&[0x00, 0xFF, 0x80]), crc32(&[0x00, 0xFF, 0x80]));
        assert_ne!(crc32(&[0x00, 0xFF]), crc32(&[0xFF, 0x00]), "順序要有影響");
    }

    /// 32 位元的加總不會溢位（大量 0xFF）。
    #[test]
    fn sum32_does_not_overflow() {
        let big = vec![0xFFu8; 100_000];
        assert_eq!(checksum32(&big), 100_000 * 255);
    }

    /// `Kind::apply` 和直接呼叫的結果一樣（`cmds.rs` 只走 `apply`）。
    #[test]
    fn kind_dispatch_matches() {
        let v = b"this is a test string to be CRC16ed";
        assert_eq!(Kind::Crc16.apply(v), crc16(v));
        assert_eq!(Kind::Crc32.apply(v), crc32(v));
        assert_eq!(Kind::Sum8.apply(v), checksum8(v));
        assert_eq!(Kind::Sum16.apply(v), checksum16(v));
        assert_eq!(Kind::Sum32.apply(v), checksum32(v));
    }
}
