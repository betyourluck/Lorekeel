//! 改名 (Kataribe → Lorekeel, 2026-08-28) の一度きりの移行と、その告知。
//!
//! **なぜ改名したか**: 同名の先行者が同じ分野に居た (`kataribe.jp` = 汎用 TRPG ルール
//! 「語り部」とその共創コミュニティ)。名前だけでなく **bundle identifier も替えた**ので、
//! Tauri の `app_data_dir` (`%APPDATA%/{identifier}`) が別の場所を指す — 何もしないと
//! **セーブ・API キー・参照画像・保存した挿絵とログが、在るのに見えない**状態になる。
//!
//! 規律 3 つ:
//! 1. **コピーであって移動ではない** — 旧フォルダを残すので、問題が出れば旧版へ戻れる
//!    (不可逆な操作を移行の初手に置かない)。
//! 2. **新側が空のときだけ**動く。新側に何かあれば、それは新版で作られたものなので触らない
//!    (二度目以降は no-op = 何度起動しても同じ)。
//! 3. **失敗しても起動は止めない**。移行は利便であって正しさではない — 失敗したら告知に
//!    その旨と旧フォルダの場所を載せ、手で運べるようにする。

use std::path::Path;

use serde::Serialize;

/// 旧 bundle identifier。**この文字列は歴史であって設定ではない** — 新規インストールにも
/// 効かせるため据え置く (消すと、古い版から上がってくる人の移行経路が消える)。
const OLD_IDENTIFIER: &str = "jp.kataribe.app";

/// 現行 bundle identifier。`tauri.conf.json` と**二重に持つ** — WebView の保存領域の
/// 引き継ぎは `AppHandle` を貰う前 (`main`) に済ませる必要があり、そこでは Tauri の
/// path API が使えないため。ずれたら移行が黙って空振りするので、PoC で綴りを固定する。
const NEW_IDENTIFIER: &str = "jp.lorekeel.app";

/// 移行の結果。frontend は `old_dir` が在るときだけ一度きりの告知を出す
/// (**新規ユーザーには出さない** — 知らない旧名を知らせても混乱にしかならない)。
#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct RenameNotice {
    /// 旧インストールが在ったか (= 告知を出す条件)。無ければ None。
    pub old_dir: Option<String>,
    /// 実際にコピーしたか (既に新側にデータが在れば false = 告知だけ)。
    pub migrated: bool,
    /// コピーに失敗した理由 (告知に載せて手で運べるようにする)。
    pub error: Option<String>,
}

/// そのフォルダが「空」か (存在しない or 中身ゼロ)。
fn is_empty_dir(p: &Path) -> bool {
    match std::fs::read_dir(p) {
        Ok(mut rd) => rd.next().is_none(),
        Err(_) => true, // 読めない = 無い扱い (作られる)
    }
}

/// 再帰コピー。**中断してもデータは壊れない** — 旧側は読むだけで、書くのは新側だけ。
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let src = entry.path();
        let dst = to.join(&name);
        if entry.file_type()?.is_dir() {
            // 卓の一時パッケージ (`packages/table/{sha256}`) は中継から取り直せるキャッシュ。
            // 大きいうえ、卓が始まれば消える性質のものなので運ばない。
            if name == "packages" && from.join("packages").join("table").is_dir() {
                copy_dir_skipping(&src, &dst, "table")?;
            } else {
                copy_dir(&src, &dst)?;
            }
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn copy_dir_skipping(from: &Path, to: &Path, skip: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_name() == skip {
            continue;
        }
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// 旧 app_data から新 app_data へ一度きりの移行を試み、告知の材料を返す。
/// `new_dir` は現行 identifier の app_data (Tauri から貰う)。
pub fn migrate(new_dir: &Path) -> RenameNotice {
    let Some(parent) = new_dir.parent() else {
        return RenameNotice::default();
    };
    let old_dir = parent.join(OLD_IDENTIFIER);
    if !old_dir.is_dir() || old_dir == new_dir {
        return RenameNotice::default(); // 新規インストール = 告知も移行も無い
    }
    let mut notice = RenameNotice {
        old_dir: Some(old_dir.display().to_string()),
        ..Default::default()
    };
    if !is_empty_dir(new_dir) {
        return notice; // 新側で既に遊んでいる = 触らない (告知だけ)
    }
    match copy_dir(&old_dir, new_dir) {
        Ok(()) => notice.migrated = true,
        Err(e) => notice.error = Some(e.to_string()),
    }
    notice
}

/// WebView の保存領域 (localStorage) を旧 identifier のプロファイルから引き継ぐ。
///
/// **app_data の移行だけでは足りない** — WebView2 のユーザーデータフォルダは
/// `%LOCALAPPDATA%/{identifier}/EBWebView` と identifier 別で、identifier を替えると
/// **空のプロファイルが作られる**。そこに入っているのは登録モデル一覧・`packagePaths`
/// (登録したパッケージ)・画像生成の設定・テーマなど、ユーザーから見て「設定」そのもの。
/// 2026-08-28 の改名で実機がこれを踏んだ (`.env` は運べていたので接続だけ生きていた)。
///
/// **必ず Tauri が WebView を起動する前 (`main`) に呼ぶこと。** 起動後は LevelDB を
/// 掴んでいるので、上書きするとプロファイルを壊す。
///
/// 条件は「新側にまだ Local Storage が無い」— 一度でも起動していれば触らない
/// (そのプロファイルには既にその人の設定が入っている)。
///
/// **Windows のみ**。macOS / Linux は WebView の実装が違い保存場所を実機で確認できて
/// いないので、何もしない (確認できない経路で人のプロファイルを触らない)。
pub fn migrate_webview_storage() {
    if !cfg!(target_os = "windows") {
        return;
    }
    let Ok(local) = std::env::var("LOCALAPPDATA") else { return };
    let rel = Path::new("EBWebView").join("Default").join("Local Storage");
    let src = Path::new(&local).join(OLD_IDENTIFIER).join(&rel);
    let dst = Path::new(&local).join(NEW_IDENTIFIER).join(&rel);
    if !src.is_dir() || dst.exists() {
        return;
    }
    if let Err(e) = copy_dir(&src, &dst) {
        // 失敗しても起動は止めない (設定が初期値に戻るだけで、遊べなくはならない)。
        eprintln!("[rename] WebView の設定を引き継げませんでした: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    /// 旧フォルダが在り新側が空 → コピーされ、旧側は**残る** (移動ではない)。
    /// 卓のキャッシュ (`packages/table`) は運ばない。
    #[test]
    fn migrates_once_and_keeps_the_old_folder() {
        let root = tmp("lorekeel_rename_case1");
        let old = root.join(OLD_IDENTIFIER);
        let new = root.join("jp.lorekeel.app");
        std::fs::create_dir_all(old.join("saves")).unwrap();
        std::fs::create_dir_all(old.join("packages/table/abc")).unwrap();
        std::fs::write(old.join("saves/a.yaml"), "turn: 3").unwrap();
        std::fs::write(old.join(".env"), "LLM_MODEL=x").unwrap();
        std::fs::write(old.join("packages/table/abc/p.zip"), "zip").unwrap();

        let n = migrate(&new);
        assert!(n.migrated && n.error.is_none(), "{n:?}");
        assert_eq!(n.old_dir, Some(old.display().to_string()));
        assert_eq!(std::fs::read_to_string(new.join("saves/a.yaml")).unwrap(), "turn: 3");
        assert!(new.join(".env").is_file(), "API キーの .env も運ぶ");
        assert!(!new.join("packages/table").exists(), "卓のキャッシュは運ばない");
        assert!(old.join("saves/a.yaml").is_file(), "旧側は残る (コピーであって移動ではない)");

        // 二度目は no-op (新側に中身が在るので触らない) だが、告知の材料は返る。
        std::fs::write(new.join("saves/a.yaml"), "turn: 9").unwrap();
        let again = migrate(&new);
        assert!(!again.migrated && again.old_dir.is_some());
        assert_eq!(std::fs::read_to_string(new.join("saves/a.yaml")).unwrap(), "turn: 9");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **identifier の綴りは tauri.conf.json と一致していなければならない** — WebView の
    /// 引き継ぎは AppHandle を貰う前に走るので Tauri から identifier を読めず、ここで
    /// 二重に持っている。ずれると移行が黙って空振りする (設定が初期値に戻るだけなので
    /// 誰も気づかない) ので、設定ファイルの実物と突き合わせて固定する。
    #[test]
    fn identifiers_match_the_tauri_config() {
        let conf = include_str!("../tauri.conf.json");
        let v: serde_json::Value = serde_json::from_str(conf).unwrap();
        assert_eq!(v["identifier"].as_str(), Some(NEW_IDENTIFIER));
        assert_ne!(OLD_IDENTIFIER, NEW_IDENTIFIER);
    }

    /// 旧フォルダが無い = 新規インストール → 移行も告知もしない
    /// (知らない旧名を知らせても混乱にしかならない)。
    #[test]
    fn fresh_install_gets_no_notice() {
        let root = tmp("lorekeel_rename_case2");
        let new = root.join("jp.lorekeel.app");
        std::fs::create_dir_all(&new).unwrap();
        assert_eq!(migrate(&new), RenameNotice::default());
        let _ = std::fs::remove_dir_all(&root);
    }
}
