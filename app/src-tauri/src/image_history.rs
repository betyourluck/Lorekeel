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

/// 書き出し先の名前を**中身に正直な拡張子**へ揃える (spec 37 追補, 2026-11-26)。
///
/// ダイアログで選ばれた名前の拡張子が元のもの (大小は問わず・jpg と jpeg は同じ) ならそのまま、
/// 違うか無ければ元の拡張子を**後ろに足す** (`foo.png` + webp → `foo.png.webp`)。置き換えないのは
/// `scene.v2` のような点を含む名前を壊さないため。中身を変換しないのに拡張子だけ変えると、名前が
/// 中身について嘘をつく (エディタのメディアの改名で拡張子を変えさせないのと同じ線)。
pub fn export_dest(chosen: &Path, ext: &str) -> PathBuf {
    let same = |a: &str| {
        let a = a.to_ascii_lowercase();
        a == ext || (matches!(ext, "jpg" | "jpeg") && matches!(a.as_str(), "jpg" | "jpeg"))
    };
    match chosen.extension().and_then(|e| e.to_str()) {
        Some(e) if same(e) => chosen.to_path_buf(),
        _ => {
            let mut name = chosen.file_name().map(|n| n.to_os_string()).unwrap_or_default();
            name.push(".");
            name.push(ext);
            chosen.with_file_name(name)
        }
    }
}

/// 履歴の 1 枚を `chosen` (保存ダイアログで選ばれたパス) へ**コピー**する (spec 37 追補)。
/// 元のファイルは残す (移動しない)。返りは実際に書いたパス。
///
/// - 読むのは**この stem の履歴ファイル**だけ (`delete` と同じ検査 = 履歴フォルダの外も
///   他パッケージの絵も読ませない)。
/// - 拡張子を足した結果がダイアログで選ばれた名前と違い、かつ既に在るなら書かない — ダイアログの
///   上書き確認はユーザーが選んだ名前にしか掛かっていないので、足した名前の既存ファイルを黙って
///   上書きしない。
/// - 書き出し先が元のファイルそのものなら何もしない (コピーは書き先を切り詰めてから書くので、
///   同じファイルへのコピーは元の絵を空にしうる)。
pub fn export(dir: &Path, stem: &str, file_name: &str, chosen: &Path) -> Result<PathBuf, String> {
    if parse(stem, file_name).is_none() {
        return Err(format!("この盤面の履歴ではありません: {file_name}"));
    }
    let src = dir.join(file_name);
    if !src.is_file() {
        return Err(format!("書き出す挿絵が見つかりません: {file_name}"));
    }
    let ext = file_name.rsplit_once('.').map(|(_, e)| e).unwrap_or("png");
    let dest = export_dest(chosen, ext);
    if dest != chosen && dest.exists() {
        return Err(format!("同じ名前のファイルが既にあります: {}", dest.display()));
    }
    let same_file = match (std::fs::canonicalize(&src), std::fs::canonicalize(&dest)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !same_file {
        std::fs::copy(&src, &dest).map_err(|e| format!("書き出しに失敗しました: {e}"))?;
    }
    Ok(dest)
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

    /// 【書き出しの名前】拡張子が元と同じ (大小・jpg/jpeg を問わず) ならそのまま、違うか無ければ
    /// 元の拡張子を後ろに足す (置き換えない = 点を含む名前を壊さない)。
    #[test]
    fn export_dest_keeps_the_source_extension_honest() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(export_dest(&p("C:/out/a.webp"), "webp"), p("C:/out/a.webp"));
        assert_eq!(export_dest(&p("C:/out/A.WEBP"), "webp"), p("C:/out/A.WEBP"));
        assert_eq!(export_dest(&p("C:/out/a.jpeg"), "jpg"), p("C:/out/a.jpeg"));
        assert_eq!(export_dest(&p("C:/out/a"), "png"), p("C:/out/a.png"));
        assert_eq!(export_dest(&p("C:/out/a.png"), "webp"), p("C:/out/a.png.webp"));
        assert_eq!(export_dest(&p("C:/out/scene.v2"), "webp"), p("C:/out/scene.v2.webp"));
    }

    /// 【書き出し】この stem の履歴ファイルだけを、選ばれた場所へコピーする。元は残る・中身は同じ・
    /// 他 stem / 無関係な名前 / パス要素 / 無いファイルは拒否 / 足した名前の既存ファイルは上書きしない /
    /// 元のファイルそのものへの書き出しは元を壊さない。
    #[test]
    fn export_copies_only_own_history_files_and_never_clobbers_silently() {
        let dir = tmp("export_src");
        let out = tmp("export_out");
        std::fs::create_dir_all(&out).unwrap();
        let mine = "0123456789abcdef";
        let other = "fedcba9876543210";
        save(&dir, mine, 100, "image/webp", b"webp-bytes").unwrap();
        save(&dir, other, 200, "image/png", b"other").unwrap();
        std::fs::write(dir.join("notes.txt"), b"not an image").unwrap();
        let name = "0123456789abcdef_100.webp";

        // 選ばれた名前そのままへ。元は残り、中身は同じ。
        let w = export(&dir, mine, name, &out.join("気に入った一枚.webp")).unwrap();
        assert_eq!(w, out.join("気に入った一枚.webp"));
        assert_eq!(std::fs::read(&w).unwrap(), b"webp-bytes");
        assert!(dir.join(name).is_file(), "移動ではなくコピー");

        // ダイアログで選ばれた名前の上書きは確認済みなので書く。
        std::fs::write(out.join("over.webp"), b"old").unwrap();
        export(&dir, mine, name, &out.join("over.webp")).unwrap();
        assert_eq!(std::fs::read(out.join("over.webp")).unwrap(), b"webp-bytes");

        // 拡張子を足した名前は確認されていないので、既にあれば書かない。
        std::fs::write(out.join("x.png.webp"), b"keep me").unwrap();
        assert!(export(&dir, mine, name, &out.join("x.png")).is_err());
        assert_eq!(std::fs::read(out.join("x.png.webp")).unwrap(), b"keep me");
        // 無ければ足した名前で書く。
        let w = export(&dir, mine, name, &out.join("y")).unwrap();
        assert_eq!(w, out.join("y.webp"));

        // 他 stem・無関係・パス要素・無いファイルは読まない。
        assert!(export(&dir, mine, "fedcba9876543210_200.png", &out.join("o.png")).is_err());
        assert!(!out.join("o.png").exists());
        assert!(export(&dir, mine, "notes.txt", &out.join("n.txt")).is_err());
        assert!(export(&dir, mine, "../0123456789abcdef_100.webp", &out.join("p.webp")).is_err());
        assert!(export(&dir, mine, "0123456789abcdef_999.webp", &out.join("q.webp")).is_err());

        // 元のファイルそのものへ書き出しても元は壊れない (コピーしない)。
        let w = export(&dir, mine, name, &dir.join(name)).unwrap();
        assert_eq!(w, dir.join(name));
        assert_eq!(std::fs::read(dir.join(name)).unwrap(), b"webp-bytes");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&out);
    }
}
