//! 設定画集 (spec 25): パッケージの `images/settings_sheets/` に置いた画像を、挿絵生成の
//! **参照画像**として添付するための選別。
//!
//! entity 照合はしない — 1 枚に複数人の立ち絵と名前、背景は分割して 1 枚、という「設定資料集」を
//! 作者/プレイヤーが作り、ファイル名順で先頭 `max` 枚を送る。無ければ添付なし (spec 24 と不変)。
//! [`pick_settings_sheets`] は**純関数** (ディレクトリ読みは [`load_settings_sheets`] の薄い包み)。
//! 4MB 超・未対応拡張子は `Skipped` として**理由つきで返す**だけで、トーストは呼び出し側。

use std::path::{Path, PathBuf};

/// 設定画集の置き場 (`images/` 配下の固定サブフォルダ)。
pub const SETTINGS_SHEETS_DIR: &str = "settings_sheets";
/// 1 枚の上限 (bytes)。超えたらスキップ (契約 settings_sheets)。
pub const SHEET_MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// [`SHEET_MAX_BYTES`] 超。
    Oversize,
    /// png/jpg/jpeg/webp 以外 (拡張子は大文字小文字不問)。
    Unsupported,
    /// 上限枚数に入らなかった (ファイル名順で後ろ)。
    OverLimit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedSheet {
    pub name: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PickedSheets {
    /// 送る順 (ファイル名順)。
    pub picked: Vec<String>,
    pub skipped: Vec<SkippedSheet>,
}

/// 拡張子 → mime (大文字小文字不問)。対応外は None。
pub fn sheet_mime(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// 選別 (純関数)。`entries` = (ファイル名, サイズ)。ファイル名順に並べ、対応拡張子かつ上限以下を
/// 先頭から `max` 枚。それ以外は理由つきで `skipped` へ。
pub fn pick_settings_sheets(entries: &[(String, u64)], max: usize) -> PickedSheets {
    let mut sorted: Vec<&(String, u64)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = PickedSheets::default();
    for (name, size) in sorted {
        if sheet_mime(name).is_none() {
            out.skipped.push(SkippedSheet { name: name.clone(), reason: SkipReason::Unsupported });
        } else if *size > SHEET_MAX_BYTES {
            out.skipped.push(SkippedSheet { name: name.clone(), reason: SkipReason::Oversize });
        } else if out.picked.len() >= max {
            out.skipped.push(SkippedSheet { name: name.clone(), reason: SkipReason::OverLimit });
        } else {
            out.picked.push(name.clone());
        }
    }
    out
}

/// 読み込んだ 1 枚。
#[derive(Debug, Clone)]
pub struct SheetImage {
    pub name: String,
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

/// `package_root/images/settings_sheets/` を読み、[`pick_settings_sheets`] で選んだ分を bytes ごと返す。
/// フォルダが無ければ空 (エラーにしない = 大半のパッケージは持たない)。
pub fn load_settings_sheets(package_root: &Path, max: usize) -> (Vec<SheetImage>, Vec<SkippedSheet>) {
    let dir: PathBuf = package_root.join("images").join(SETTINGS_SHEETS_DIR);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return (Vec::new(), Vec::new());
    };
    let mut entries: Vec<(String, u64)> = Vec::new();
    for e in rd.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        entries.push((e.file_name().to_string_lossy().into_owned(), meta.len()));
    }
    let picked = pick_settings_sheets(&entries, max);
    let mut images = Vec::new();
    for name in picked.picked {
        let Some(mime) = sheet_mime(&name) else { continue };
        if let Ok(bytes) = std::fs::read(dir.join(&name)) {
            images.push(SheetImage { name, mime, bytes });
        }
    }
    (images, picked.skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 【選別】ファイル名順・拡張子 4 種 (大文字小文字不問)・4MB 超は Oversize・未対応は Unsupported・
    /// 上限超は OverLimit・空なら空。
    #[test]
    fn pick_orders_filters_and_explains_skips() {
        let e = |n: &str, s: u64| (n.to_string(), s);
        let entries = vec![
            e("b_cast.PNG", 1000),
            e("a_bg.webp", 2000),
            e("c_huge.jpg", SHEET_MAX_BYTES + 1),
            e("d_notes.txt", 10),
            e("e_extra.jpeg", 500),
            e("f_more.png", 500),
        ];
        let p = pick_settings_sheets(&entries, 3);
        assert_eq!(p.picked, vec!["a_bg.webp", "b_cast.PNG", "e_extra.jpeg"], "{p:?}");
        assert!(p.skipped.contains(&SkippedSheet { name: "c_huge.jpg".into(), reason: SkipReason::Oversize }));
        assert!(p.skipped.contains(&SkippedSheet { name: "d_notes.txt".into(), reason: SkipReason::Unsupported }));
        assert!(p.skipped.contains(&SkippedSheet { name: "f_more.png".into(), reason: SkipReason::OverLimit }));
        assert_eq!(sheet_mime("X.JPG"), Some("image/jpeg"));
        assert_eq!(sheet_mime("x.gif"), None);
        assert_eq!(pick_settings_sheets(&[], 3), PickedSheets::default());
    }

    /// 【IO 包み】フォルダ無しは空・有れば bytes ごと (tempdir)。
    #[test]
    fn load_reads_folder_or_returns_empty() {
        let root = std::env::temp_dir().join(format!("kataribe_sheets_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(load_settings_sheets(&root, 3).0.is_empty());
        let dir = root.join("images").join(SETTINGS_SHEETS_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("02_bg.webp"), b"RIFFxxxxWEBP").unwrap();
        std::fs::write(dir.join("01_cast.png"), b"\x89PNG....").unwrap();
        std::fs::write(dir.join("readme.txt"), b"x").unwrap();
        let (imgs, skipped) = load_settings_sheets(&root, 3);
        assert_eq!(imgs.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), vec!["01_cast.png", "02_bg.webp"]);
        assert_eq!(imgs[0].mime, "image/png");
        assert_eq!(imgs[1].bytes, b"RIFFxxxxWEBP");
        assert_eq!(skipped.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
