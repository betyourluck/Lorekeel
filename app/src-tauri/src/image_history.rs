//! 挿絵の履歴 (spec 37)。生成した絵を `<ログフォルダ>/generated_images/` へ
//! `{stem}_{unix_ms}.{ext}` で書き、同じ stem のものだけを新しい順に一覧し、削除する。
//!
//! **フォルダそのものが正本**で、一覧は毎回フォルダを読んで組む (セーブにも localStorage にも
//! 索引を持たない = 第二の真実源を作らない)。stem はセーブ・参照ストックと同じ
//! 「パッケージパスの FNV-1a 16 桁 hex」(`package_save_stem`) で、呼び出し側が渡す。
//!
//! 純関数 (名前の組み立て・解析・削除の可否) と fs 操作を分け、PoC は両方を踏む。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// ログフォルダ直下のサブフォルダ名 (契約 `ImageGeneration.history`)。
pub const DIR_NAME: &str = "generated_images";

/// 一覧の 1 枚 (frontend へ)。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Entry {
    /// ファイル名 (削除の鍵。パス要素を含まない)。
    pub file_name: String,
    /// 絶対パス (frontend が `asset://` へ変換する)。
    pub path: String,
    /// 生成完了時刻 (unix ミリ秒) — ファイル名から読む。
    pub created_ms: u64,
    /// ファイルサイズ (バイト)。
    pub bytes: u64,
}

/// mime → 拡張子。知らない mime は png に倒す (spec 24 の保存名と同じ分岐)。
pub fn ext_for(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    }
}

/// `{stem}_{ms}.{ext}`。stem は hex なのでそのまま、ms は数字だけ = パス要素は構造的に入らない。
pub fn file_name(stem: &str, unix_ms: u64, mime: &str) -> String {
    let stem: String = stem.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    format!("{stem}_{unix_ms}.{}", ext_for(mime))
}

/// この stem の履歴ファイルなら生成時刻 (ms) を返す。他 stem・パス要素入り・画像以外の
/// 拡張子・ms が数字でないものは None (= 一覧にも削除にも乗らない)。
pub fn parse(stem: &str, name: &str) -> Option<u64> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return None;
    }
    let rest = name.strip_prefix(stem)?.strip_prefix('_')?;
    let (ms, ext) = rest.rsplit_once('.')?;
    if !matches!(ext, "png" | "jpg" | "webp") {
        return None;
    }
    if ms.is_empty() || !ms.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    ms.parse().ok()
}

/// 履歴フォルダ (`<log_dir>/generated_images`)。
pub fn dir_under(log_dir: &Path) -> PathBuf {
    log_dir.join(DIR_NAME)
}

/// 1 枚書く (フォルダ不在は作る)。返りは書いた絶対パス。
pub fn save(dir: &Path, stem: &str, unix_ms: u64, mime: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(file_name(stem, unix_ms, mime));
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// この stem のファイルだけを**新しい順** (ms 降順・同点は名前降順) で返す。フォルダ不在は空。
pub fn list(dir: &Path, stem: &str) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Entry> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let created_ms = parse(stem, &name)?;
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some(Entry {
                path: e.path().to_string_lossy().into_owned(),
                file_name: name,
                created_ms,
                bytes: meta.len(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then_with(|| b.file_name.cmp(&a.file_name)));
    out
}

/// 1 枚消す。**この stem の履歴ファイルの名前**でなければ触らない (他パッケージの絵や
/// 無関係なファイルを消す経路を作らない)。無いものを消すのは成功 (冪等)。
pub fn delete(dir: &Path, stem: &str, file_name: &str) -> Result<(), String> {
    if parse(stem, file_name).is_none() {
        return Err(format!("この盤面の履歴ではありません: {file_name}"));
    }
    let path = dir.join(file_name);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("削除に失敗しました: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "kataribe_image_history_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// 【名前】`{stem}_{ms}.{ext}` で ext は mime から。解析は同じ stem・画像拡張子・数字の ms
    /// だけを拾い、他 stem / パス要素 / 別拡張子 / 非数字は None。
    #[test]
    fn history_file_name_round_trips_and_parse_rejects_foreign_names() {
        let stem = "0123456789abcdef";
        assert_eq!(file_name(stem, 1_761_900_000_123, "image/png"), "0123456789abcdef_1761900000123.png");
        assert_eq!(file_name(stem, 7, "image/jpeg"), "0123456789abcdef_7.jpg");
        assert_eq!(file_name(stem, 7, "image/webp"), "0123456789abcdef_7.webp");
        assert_eq!(file_name(stem, 7, "application/octet-stream"), "0123456789abcdef_7.png");
        // stem にパス要素を混ぜても名前には入らない。
        assert_eq!(file_name("../x", 1, "image/png"), "x_1.png");

        assert_eq!(parse(stem, "0123456789abcdef_1761900000123.png"), Some(1_761_900_000_123));
        assert_eq!(parse(stem, "0123456789abcdef_7.webp"), Some(7));
        assert_eq!(parse(stem, "fedcba9876543210_7.png"), None, "他 stem");
        assert_eq!(parse(stem, "0123456789abcdef_7.txt"), None, "画像でない拡張子");
        assert_eq!(parse(stem, "0123456789abcdef_abc.png"), None, "ms が数字でない");
        assert_eq!(parse(stem, "0123456789abcdef_.png"), None, "ms が空");
        assert_eq!(parse(stem, "0123456789abcdef7.png"), None, "区切りの _ が無い");
        assert_eq!(parse(stem, "../0123456789abcdef_7.png"), None, "パス要素");
        assert_eq!(parse(stem, "0123456789abcdef_7.png/../x.png"), None, "パス要素 (後ろ)");
    }

    /// 【フォルダの往復】書く → 一覧は新しい順で他 stem は出ない → 削除 → 他 stem・無関係な名前の
    /// 削除は拒否されてファイルが残る → 無いものの削除は成功 (冪等)。
    #[test]
    fn history_dir_lists_newest_first_per_stem_and_deletes_only_own_files() {
        let dir = tmp("roundtrip");
        let mine = "0123456789abcdef";
        let other = "fedcba9876543210";
        assert!(list(&dir, mine).is_empty(), "フォルダ不在は空");

        save(&dir, mine, 100, "image/png", b"a").unwrap();
        save(&dir, mine, 300, "image/webp", b"ccc").unwrap();
        save(&dir, mine, 200, "image/jpeg", b"bb").unwrap();
        save(&dir, other, 400, "image/png", b"zzzz").unwrap();
        std::fs::write(dir.join("notes.txt"), b"not an image").unwrap();

        let names: Vec<(String, u64, u64)> =
            list(&dir, mine).into_iter().map(|e| (e.file_name, e.created_ms, e.bytes)).collect();
        assert_eq!(
            names,
            vec![
                ("0123456789abcdef_300.webp".to_string(), 300, 3),
                ("0123456789abcdef_200.jpg".to_string(), 200, 2),
                ("0123456789abcdef_100.png".to_string(), 100, 1),
            ]
        );
        assert_eq!(list(&dir, other).len(), 1);
        assert!(list(&dir, mine)[0].path.ends_with("0123456789abcdef_300.webp"));

        // 他 stem のファイル・無関係なファイルは消せない (エラー + 残る)。
        assert!(delete(&dir, mine, "fedcba9876543210_400.png").is_err());
        assert!(dir.join("fedcba9876543210_400.png").is_file());
        assert!(delete(&dir, mine, "notes.txt").is_err());
        assert!(dir.join("notes.txt").is_file());
        assert!(delete(&dir, mine, "../notes.txt").is_err());

        delete(&dir, mine, "0123456789abcdef_200.jpg").unwrap();
        assert!(!dir.join("0123456789abcdef_200.jpg").exists());
        assert_eq!(list(&dir, mine).len(), 2);
        // 二度消しは成功 (冪等)。
        delete(&dir, mine, "0123456789abcdef_200.jpg").unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }
}
