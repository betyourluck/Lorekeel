//! 参照ストック (spec 27 Phase A): 挿絵の参照画像を**セッションフォルダ**で持つ。
//!
//! **`images/settings_sheets/` は種であって参照先ではない。** ゲーム開始後に効くのは
//! `app_data/refs/<パッケージパスの FNV>/` の現物だけで、package 側は原則読まない
//! (例外は「同梱の参照画像を入れ直す」= [`seed_from_package`] の明示呼び出しのみ)。
//! `initial_state` が package 宣言を `GameState` へ seed して以降 state が正本、と同型。
//!
//! **前詰め不変条件 (決定 10)**: `ref1..k` (k = 0..max) が常に前詰めで存在し `ref{k+1}` 以降は
//! 無い。削除は後続を前へリネームして詰める。これで **UI 番号 = ファイル名番号 = ワイヤ番号
//! `%ref_n%`** が恒等的に一致し、「2 番に入れたのに 1 番として送られた」が構造的に起きない。
//!
//! **第二の真実源を持たない** (決定 4/11): 状態はファイルの存在と前詰め順序そのもの。出所メタも
//! `.initialized` マーカーも置かない (初期化済みの判定は**フォルダの存在のみ**)。

use std::path::{Path, PathBuf};

/// プレイヤーが枠へ入れる 1 枚の上限 (決定 8)。**セッションフォルダは配布されないので、
/// 配布サイズの作法 (`harness::SHEET_MAX_BYTES` 4MB) は適用しない。** 実測の生成画像は
/// 1〜3MB で、当たるのは `highest` の上端だけ。
pub const PLAYER_SHEET_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// 1 回の生成で送る参照の合計上限 (raw bytes、決定 8)。**Gemini の inlineData は base64 で
/// 4/3 に膨らむ**ので、1 枚の上限を上げただけだと静かな 400 に化ける。超過は後ろから落とす。
pub const SEND_TOTAL_MAX_BYTES: u64 = 12 * 1024 * 1024;

/// 参照ファイルの basename 接頭辞。
const REF_PREFIX: &str = "ref";

/// mime → 拡張子 (`harness::sheet_mime` の逆写像)。対応外は None。
pub fn ext_for_mime(mime: &str) -> Option<&'static str> {
    match mime {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// `ref{n}.{ext}` の n (1 始まり)。参照ファイルでなければ None (純関数)。
/// 拡張子が対応外のものは**参照として数えない** — 数えると前詰めの穴になる。
pub fn ref_index(name: &str) -> Option<usize> {
    harness::sheet_mime(name)?;
    let stem = name.split('.').next()?;
    let digits = stem.strip_prefix(REF_PREFIX)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match digits.parse::<usize>() {
        Ok(0) | Err(_) => None,
        Ok(n) => Some(n),
    }
}

/// 前詰め不変条件の検査 (純関数・PoC 用)。`ref1..k` が過不足なく在り、参照以外が混ざらないこと。
/// 本番は構成によって不変条件を保つ ([`put_slot`] / [`delete_slot`]) ので、検査はテストだけが使う。
#[cfg(test)]
pub fn is_front_packed(names: &[String]) -> bool {
    let mut idx: Vec<usize> = names.iter().filter_map(|n| ref_index(n)).collect();
    if idx.len() != names.len() {
        return false; // 参照でないファイルが混ざっている
    }
    idx.sort_unstable();
    idx.iter().enumerate().all(|(i, n)| *n == i + 1)
}

/// 送信合計の上限で**後ろから**落とす (純関数)。返り値 = (残す枚数, 落とした名前列)。
/// 落とす順は ref3 → ref2 = 前詰めの前側 (作者の種・最初に置いた 1 枚) を最後まで残す。
pub fn trim_to_total(sizes: &[(String, u64)], max_total: u64) -> (usize, Vec<String>) {
    let mut total: u64 = sizes.iter().map(|(_, s)| *s).sum();
    let mut keep = sizes.len();
    let mut dropped = Vec::new();
    while keep > 0 && total > max_total {
        keep -= 1;
        total -= sizes[keep].1;
        dropped.push(sizes[keep].0.clone());
    }
    (keep, dropped)
}

/// セッションフォルダの現物 (index 昇順)。参照以外のファイルは無視する。
pub fn stock_files(dir: &Path) -> Vec<(usize, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(usize, PathBuf)> = rd
        .flatten()
        .filter(|e| e.metadata().map(|m| m.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            ref_index(&name).map(|i| (i, e.path()))
        })
        .collect();
    out.sort_by_key(|(i, _)| *i);
    out
}

/// 送る 1 枚 (`image_gen::RefImage` へ写す前の形)。
#[derive(Debug, Clone)]
pub struct RefImage {
    pub name: String,
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

/// 送られなかった理由 (提示層が文言にする)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefSkip {
    /// [`PLAYER_SHEET_MAX_BYTES`] 超。
    Oversize,
    /// 枠数 (`MAX_REFS`) の外。
    OverLimit,
    /// `ref{n}.{png|jpg|webp}` でない = **手でフォルダに置かれたファイル**。
    /// 黙って無視すると「入れたのに効かない」の沈黙になるので、理由つきで見せる。
    Unrecognized,
}

/// セッションフォルダから**送る列**を読む。順序は index 昇順 = `%ref_1%..` と恒等的に一致する
/// (ファイル名順ではない — 手で置いた `aaa.png` が `ref1.png` より前に並んで枠番号と食い違う
/// のを構造的に防ぐ)。**ここが唯一の送信経路**で、UI の枠列と同じ関数から導かれる。
pub fn load_refs(dir: &Path, max: usize) -> (Vec<RefImage>, Vec<(String, RefSkip)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return (Vec::new(), Vec::new());
    };
    let mut refs: Vec<(usize, String, PathBuf, u64)> = Vec::new();
    let mut skipped: Vec<(String, RefSkip)> = Vec::new();
    for e in rd.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        match ref_index(&name) {
            Some(i) => refs.push((i, name, e.path(), meta.len())),
            None => skipped.push((name, RefSkip::Unrecognized)),
        }
    }
    refs.sort_by_key(|r| r.0);
    let mut out = Vec::new();
    for (i, name, path, size) in refs {
        if i > max {
            skipped.push((name, RefSkip::OverLimit));
        } else if size > PLAYER_SHEET_MAX_BYTES {
            skipped.push((name, RefSkip::Oversize));
        } else if let (Some(mime), Ok(bytes)) = (harness::sheet_mime(&name), std::fs::read(&path)) {
            out.push(RefImage { name, mime, bytes });
        }
    }
    skipped.sort();
    (out, skipped)
}

/// パッケージの種を写す (**常に書く**)。既存の参照は消してから前詰めで書き直す。
/// 初回判定は [`ensure_seeded`] の責務で、こちらは「入れ直す」の実体でもある。
/// 種の選別は `harness::load_settings_sheets` = **配布作法 4MB** が適用される。
pub fn seed_from_package(dir: &Path, package_root: &Path, max: usize) -> std::io::Result<usize> {
    let (sheets, _skipped) = harness::load_settings_sheets(package_root, max);
    std::fs::create_dir_all(dir)?;
    for (_, p) in stock_files(dir) {
        let _ = std::fs::remove_file(p);
    }
    for (i, s) in sheets.iter().enumerate() {
        let ext = ext_for_mime(s.mime).unwrap_or("png");
        std::fs::write(dir.join(format!("{REF_PREFIX}{}.{ext}", i + 1)), &s.bytes)?;
    }
    Ok(sheets.len())
}

/// フォルダ不在時だけ写す (決定 3)。**判定はフォルダの存在のみ** — ファイル数は見ない
/// (全削除で空になっても初期化済み)。種が 0 枚でもフォルダは作る = 二度目に写さない。
/// 返り値は「今回写したか」。
pub fn ensure_seeded(dir: &Path, package_root: &Path, max: usize) -> std::io::Result<bool> {
    if dir.exists() {
        return Ok(false);
    }
    seed_from_package(dir, package_root, max)?;
    Ok(true)
}

/// 枠へ書く (入れ替え)。`slot` は 1 始まり。前詰めを保つため**末尾+1 より後ろは丸める**。
/// 同じ枠の旧ファイルは拡張子違いも含めて消してから書く (`ref1.png` と `ref1.webp` の同居を作らない)。
/// 返り値は実際に書いた枠番号。
pub fn put_slot(
    dir: &Path,
    slot: usize,
    mime: &str,
    bytes: &[u8],
    max: usize,
) -> Result<usize, String> {
    if bytes.len() as u64 > PLAYER_SHEET_MAX_BYTES {
        return Err(format!(
            "画像が大きすぎます ({:.1}MB > {}MB)",
            bytes.len() as f64 / (1024.0 * 1024.0),
            PLAYER_SHEET_MAX_BYTES / (1024 * 1024)
        ));
    }
    let Some(ext) = ext_for_mime(mime) else {
        return Err(format!("参照にできない形式です ({mime})"));
    };
    if max == 0 {
        return Err("このプロバイダは参照画像を受け付けません".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("フォルダを作成できません: {e}"))?;
    let files = stock_files(dir);
    let slot = slot.clamp(1, (files.len() + 1).min(max));
    for (i, p) in &files {
        if *i == slot {
            let _ = std::fs::remove_file(p);
        }
    }
    std::fs::write(dir.join(format!("{REF_PREFIX}{slot}.{ext}")), bytes)
        .map_err(|e| format!("書き込みに失敗しました: {e}"))?;
    Ok(slot)
}

/// 枠を消して**後続を前へ詰める** (前詰め不変条件)。拡張子は保持したままリネームする。
pub fn delete_slot(dir: &Path, slot: usize) -> Result<(), String> {
    let files = stock_files(dir);
    if !files.iter().any(|(i, _)| *i == slot) {
        return Err("その枠は空です".into());
    }
    for (i, p) in &files {
        if *i == slot {
            std::fs::remove_file(p).map_err(|e| format!("削除に失敗しました: {e}"))?;
        }
    }
    let rest: Vec<&PathBuf> = files.iter().filter(|(i, _)| *i > slot).map(|(_, p)| p).collect();
    for (n, from) in rest.iter().enumerate() {
        let ext = from.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        let to = dir.join(format!("{REF_PREFIX}{}.{ext}", slot + n));
        let _ = std::fs::remove_file(&to); // Windows の rename は既存先で失敗する
        std::fs::rename(from, &to).map_err(|e| format!("詰め直しに失敗しました: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kataribe_refstock_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }
    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
    /// 種を持つパッケージを作る。
    fn package(root: &Path, sheets: &[(&str, &[u8])]) {
        let d = root.join("images").join(harness::SETTINGS_SHEETS_DIR);
        std::fs::create_dir_all(&d).unwrap();
        for (n, b) in sheets {
            std::fs::write(d.join(n), b).unwrap();
        }
    }

    /// 【純関数】`ref{n}.{ext}` の解釈と前詰め検査。
    #[test]
    fn ref_index_and_front_packed_are_pure() {
        assert_eq!(ref_index("ref1.png"), Some(1));
        assert_eq!(ref_index("ref3.WEBP"), Some(3));
        assert_eq!(ref_index("ref0.png"), None, "0 番は無い (1 始まり)");
        assert_eq!(ref_index("ref1.txt"), None, "対応外の拡張子は参照として数えない");
        assert_eq!(ref_index("refa.png"), None);
        assert_eq!(ref_index("cast.png"), None);
        assert_eq!(ext_for_mime("image/jpeg"), Some("jpg"));
        assert_eq!(ext_for_mime("image/gif"), None);

        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(is_front_packed(&s(&[])));
        assert!(is_front_packed(&s(&["ref2.jpg", "ref1.png"])));
        assert!(!is_front_packed(&s(&["ref1.png", "ref3.png"])), "中抜けは違反");
        assert!(!is_front_packed(&s(&["ref1.png", "ref1.webp"])), "同じ枠の二重は違反");
        assert!(!is_front_packed(&s(&["ref1.png", "notes.txt"])), "参照以外の同居は違反");
    }

    /// 【合計上限】後ろ (ref3 → ref2) から落とし、落とした名前を返す。
    #[test]
    fn trim_to_total_drops_from_the_back() {
        let mb = |n: u64| n * 1024 * 1024;
        let sizes = vec![
            ("ref1.png".to_string(), mb(5)),
            ("ref2.png".to_string(), mb(5)),
            ("ref3.png".to_string(), mb(5)),
        ];
        let (keep, dropped) = trim_to_total(&sizes, SEND_TOTAL_MAX_BYTES);
        assert_eq!(keep, 2, "15MB → 10MB へ");
        assert_eq!(dropped, vec!["ref3.png"]);
        let (keep, dropped) = trim_to_total(&sizes, mb(4));
        assert_eq!((keep, dropped.len()), (0, 3), "1 枚も入らないなら全部落ちる");
        assert_eq!(trim_to_total(&sizes, mb(100)), (3, vec![]), "収まれば無改変");
        assert_eq!(trim_to_total(&[], mb(1)), (0, vec![]));
    }

    /// 【写し取り】フォルダ不在なら前詰めで写す・**存在すれば写さない** (消した絵が復活しない)・
    /// 種 0 枚でもフォルダは作る・「入れ直す」は空フォルダにも書く。
    #[test]
    fn seed_copies_once_and_reseed_restores() {
        let dir = tmp("seed");
        let pkg = tmp("seed_pkg");
        package(&pkg, &[("02_bg.webp", b"RIFFxxxxWEBP"), ("01_cast.png", b"\x89PNG1")]);

        assert!(ensure_seeded(&dir, &pkg, 3).unwrap(), "初回は写す");
        assert_eq!(names(&dir), vec!["ref1.png", "ref2.webp"], "ファイル名順で前詰め・拡張子保持");
        assert!(is_front_packed(&names(&dir)));

        // プレイヤーが 1 枚消す → 二度目の開始で復活しない (決定 3)。
        delete_slot(&dir, 1).unwrap();
        assert_eq!(names(&dir), vec!["ref1.webp"], "詰まる");
        assert!(!ensure_seeded(&dir, &pkg, 3).unwrap(), "フォルダが在るので写さない");
        assert_eq!(names(&dir), vec!["ref1.webp"], "消した絵は復活しない");

        // 全削除しても「初期化済み」のまま (判定はフォルダの存在のみ・マーカーを持たない)。
        delete_slot(&dir, 1).unwrap();
        assert!(names(&dir).is_empty());
        assert!(!ensure_seeded(&dir, &pkg, 3).unwrap());

        // 「入れ直す」だけが種を書き戻す唯一の経路 (空フォルダにも書く)。
        assert_eq!(seed_from_package(&dir, &pkg, 3).unwrap(), 2);
        assert_eq!(names(&dir), vec!["ref1.png", "ref2.webp"]);

        // 種 0 枚のパッケージでもフォルダは作る = 二度目に写さない。
        let dir0 = tmp("seed0");
        let pkg0 = tmp("seed0_pkg");
        std::fs::create_dir_all(&pkg0).unwrap();
        assert!(ensure_seeded(&dir0, &pkg0, 3).unwrap());
        assert!(dir0.is_dir() && names(&dir0).is_empty());
        assert!(!ensure_seeded(&dir0, &pkg0, 3).unwrap());

        for d in [dir, pkg, dir0, pkg0] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// 【前詰め不変条件】削除で後続がリネームされ、中抜けも `ref3` の残骸も生まれない。
    #[test]
    fn delete_compacts_and_preserves_extensions() {
        let dir = tmp("del");
        std::fs::create_dir_all(&dir).unwrap();
        for (n, b) in [("ref1.png", &b"a"[..]), ("ref2.jpg", b"bb"), ("ref3.webp", b"ccc")] {
            std::fs::write(dir.join(n), b).unwrap();
        }
        delete_slot(&dir, 2).unwrap();
        assert_eq!(names(&dir), vec!["ref1.png", "ref2.webp"], "ref3.webp が ref2.webp へ (拡張子保持)");
        assert_eq!(std::fs::read(dir.join("ref2.webp")).unwrap(), b"ccc", "中身が入れ替わらない");
        assert!(is_front_packed(&names(&dir)));

        delete_slot(&dir, 1).unwrap();
        assert_eq!(names(&dir), vec!["ref1.webp"]);
        assert!(delete_slot(&dir, 2).is_err(), "空の枠は消せない");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 【送る列】index 昇順で読む (ファイル名順ではない) = ワイヤの `%ref_n%` と枠番号が一致。
    /// 手で置いたファイル・枠外・8MB 超は**理由つきで**返す (黙って無視しない)。
    #[test]
    fn load_refs_orders_by_index_and_explains_every_omission() {
        let dir = tmp("load");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ref2.webp"), b"two").unwrap();
        std::fs::write(dir.join("ref1.png"), b"one").unwrap();
        // 手で置いたファイル: ファイル名順なら ref1 より前に来るが、送る列には入らない。
        std::fs::write(dir.join("aaa.png"), b"manual").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        std::fs::write(dir.join("ref4.png"), b"far").unwrap();
        std::fs::write(dir.join("ref3.png"), vec![0u8; (PLAYER_SHEET_MAX_BYTES + 1) as usize]).unwrap();

        let (refs, skipped) = load_refs(&dir, 3);
        assert_eq!(
            refs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["ref1.png", "ref2.webp"],
            "index 昇順・手置きは混ざらない"
        );
        assert_eq!(refs[0].bytes, b"one");
        assert_eq!(refs[1].mime, "image/webp");
        assert!(skipped.contains(&("aaa.png".to_string(), RefSkip::Unrecognized)), "{skipped:?}");
        assert!(skipped.contains(&("notes.txt".to_string(), RefSkip::Unrecognized)));
        assert!(skipped.contains(&("ref4.png".to_string(), RefSkip::OverLimit)));
        assert!(skipped.contains(&("ref3.png".to_string(), RefSkip::Oversize)));
        assert!(load_refs(&tmp("load_absent"), 3).0.is_empty(), "フォルダ無しは空 (エラーにしない)");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 【入れ替え】既存枠は拡張子違いでも置き換わる・末尾+1 に丸める・8MB 超と非対応 mime は弾く。
    #[test]
    fn put_slot_replaces_appends_and_rejects() {
        let dir = tmp("put");
        assert_eq!(put_slot(&dir, 1, "image/png", b"one", 3).unwrap(), 1);
        assert_eq!(put_slot(&dir, 9, "image/webp", b"two", 3).unwrap(), 2, "末尾+1 へ丸める");
        assert_eq!(names(&dir), vec!["ref1.png", "ref2.webp"]);

        // 同じ枠へ別形式 → 旧ファイルが残らない (ref1.png と ref1.webp の同居を作らない)。
        assert_eq!(put_slot(&dir, 1, "image/webp", b"one-b", 3).unwrap(), 1);
        assert_eq!(names(&dir), vec!["ref1.webp", "ref2.webp"]);
        assert!(is_front_packed(&names(&dir)));

        assert!(put_slot(&dir, 1, "image/gif", b"x", 3).is_err(), "非対応 mime");
        let big = vec![0u8; (PLAYER_SHEET_MAX_BYTES + 1) as usize];
        let e = put_slot(&dir, 1, "image/png", &big, 3).unwrap_err();
        assert!(e.contains("大きすぎます"), "{e}");
        assert_eq!(names(&dir), vec!["ref1.webp", "ref2.webp"], "弾いたら現物は無傷");
        let _ = std::fs::remove_dir_all(dir);
    }
}
