//! spec 28 Phase A: パッケージエディタ (編集モード) の純関数部。
//!
//! ここに置くのは**ファイル列挙・パス検証・原子書き込み**だけ — 検査規則 (lint) は
//! 一切持たない (spec 28 の原則「新しい検査規則は作らない」。診断は Phase B で
//! `harness::inspect` の既存 5 系統を呼ぶだけにする)。
//!
//! パスの規律: frontend は**相対パスしか運ばない** (編集ルートは `open_editor` が
//! backend に登録する = 保存の瞬間に選択パッケージが替わっていた TOCTOU を構造的に消す)。
//! 相対パスは「直下の package.yaml / campaign.yaml」か「カテゴリフォルダ直下の 1 段」
//! しか受けない — 形の検証で traversal は構造的に不可能だが、パッケージ内のフォルダが
//! シンボリックリンクで外を指す穴は canonicalize 後の containment で塞ぐ
//! (`resolve_asset` と同じ規律)。

use std::path::{Path, PathBuf};

use serde::Serialize;

/// 編集できるカテゴリフォルダ (フォルダ名, カテゴリ名)。直下のみ・YAML のみ。
const EDITABLE_DIRS: &[(&str, &str)] = &[
    ("scenarios", "scenario"),
    ("characters", "character"),
    ("memoria", "memoria"),
];

/// ファイル一覧の 1 行。`category` は frontend のグルーピングと Phase B の lint kind の素。
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct EditorFileEntry {
    /// 編集ルートからの相対パス (区切りは常に `/`)。read/save がこのまま受ける。
    pub rel_path: String,
    /// `package` | `campaign` | `scenario` | `character` | `memoria`。
    pub category: String,
}

/// 相対パスの形の検証。受けるのは:
/// - `package.yaml` / `campaign.yaml` (直下)
/// - `scenarios/<name>.yaml|yml` / `characters/…` / `memoria/…` (カテゴリ直下 1 段のみ)
///
/// 文字集合は縛らない — 一覧は実在ファイルから作るので、ここで縛ると
/// 「一覧に出るのに開けないファイル」を作る。形 (段数・拡張子・`..`・絶対パス・
/// 隠しファイル) だけを縛る。
pub fn validate_rel_path(rel: &str) -> Result<(), String> {
    if rel.is_empty() || rel.contains('\\') {
        return Err(format!("不正なパスです: {rel}"));
    }
    if rel == "package.yaml" || rel == "campaign.yaml" {
        return Ok(());
    }
    let mut parts = rel.split('/');
    let (Some(dir), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(format!("編集できるのはカテゴリフォルダ直下の YAML だけです: {rel}"));
    };
    if !EDITABLE_DIRS.iter().any(|(d, _)| *d == dir) {
        return Err(format!("編集対象外のフォルダです: {dir}"));
    }
    let ok_ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e == "yaml" || e == "yml");
    if name.is_empty() || name == ".." || name.starts_with('.') || !ok_ext {
        return Err(format!("編集できるのは YAML ファイルだけです: {rel}"));
    }
    Ok(())
}

/// 編集ルート直下のファイル一覧 (直下のみ・YAML のみ・決定論的順序)。
/// `package.yaml` → `campaign.yaml` (あれば) → カテゴリフォルダをこの順に、各フォルダ内は
/// ファイル名昇順 (read_dir の順は OS 依存なので sort する — `inspect` と同じ理由)。
pub fn list_files(root: &Path) -> Vec<EditorFileEntry> {
    let mut out = Vec::new();
    let entry = |rel: &str, cat: &str| EditorFileEntry {
        rel_path: rel.to_string(),
        category: cat.to_string(),
    };
    if root.join("package.yaml").is_file() {
        out.push(entry("package.yaml", "package"));
    }
    if root.join("campaign.yaml").is_file() {
        out.push(entry("campaign.yaml", "campaign"));
    }
    for (dir, cat) in EDITABLE_DIRS {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else { continue };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .filter(|n| {
                !n.starts_with('.')
                    && Path::new(n)
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e == "yaml" || e == "yml")
            })
            .collect();
        names.sort();
        for n in names {
            out.push(entry(&format!("{dir}/{n}"), cat));
        }
    }
    out
}

/// 相対パスを編集ルートで解決し、**canonicalize 後の containment** を確認して返す。
/// 対象は実在ファイル前提 (v1 は編集のみ — 新規作成は Phase D)。
pub fn resolve_in_root(root: &Path, rel: &str) -> Result<PathBuf, String> {
    validate_rel_path(rel)?;
    let path = root.join(rel);
    let canon = path
        .canonicalize()
        .map_err(|e| format!("ファイルを開けません: {rel} ({e})"))?;
    let canon_root = root
        .canonicalize()
        .map_err(|e| format!("パッケージフォルダを開けません: {e}"))?;
    if !canon.starts_with(&canon_root) {
        return Err(format!("パッケージフォルダの外を指しています: {rel}"));
    }
    Ok(canon)
}

/// tmp → rename の原子書き込み (`save_session` と同じ流儀 — 書きかけで配布物を壊さない)。
/// tmp はドット始まりの隣接ファイル (`.{name}.tmp`) なので、一覧 (`list_files`) にも
/// `tree_hash` (spec 17 — 除外リストでなく走査対象だが、rename で消えるので観測されない)
/// にも実質載らない。
pub fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "不正な書き込み先です".to_string())?;
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    std::fs::write(&tmp, text).map_err(|e| format!("書き込みに失敗しました: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("書き込みの確定に失敗しました: {e}")
    })
}

/// 保存の本体 (卓ガードの後に呼ばれる)。順序は spec 28 A.6 で凍結:
/// **パス検証 → 原子書き込み → (fork なら) 出所メタ削除**。逆順だと書き込み失敗時に
/// 「フォーク済みなのに中身は書庫版のまま」という半端が残る。
///
/// 返りは `(forked, warning)`。メタ削除だけ失敗したら `(false, Some(警告))` —
/// 保存本体は成功しており、frontend が from_site を保てば次の保存で再試行される。
/// メタが既に無い (NotFound) のは冪等に成功扱い。
pub fn save_with_fork(
    root: &Path,
    rel: &str,
    text: &str,
    fork: bool,
    meta_file: &str,
) -> Result<(bool, Option<String>), String> {
    let path = resolve_in_root(root, rel)?;
    atomic_write(&path, text)?;
    if !fork {
        return Ok((false, None));
    }
    match std::fs::remove_file(root.join(meta_file)) {
        Ok(()) => Ok((true, None)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((true, None)),
        Err(e) => Ok((
            false,
            Some(format!("出所メタの削除に失敗しました (次の保存で再試行します): {e}")),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// パス検証: 受けるのは直下 2 枚とカテゴリ直下 1 段の YAML だけ。
    /// traversal・絶対パス・バックスラッシュ・隠しファイル・段数超過・対象外フォルダを拒否。
    #[test]
    fn rel_path_validation_accepts_categories_and_blocks_escapes() {
        for ok in [
            "package.yaml",
            "campaign.yaml",
            "scenarios/main.yaml",
            "characters/アリス.yml",
            "memoria/promise.yaml",
        ] {
            assert!(validate_rel_path(ok).is_ok(), "{ok} は受けるべき");
        }
        for bad in [
            "",
            "../package.yaml",
            "scenarios/../../etc/passwd",
            "scenarios\\main.yaml",
            "C:/tmp/x.yaml",
            "scenarios/sub/deep.yaml",
            "images/bg.webp",
            "scenarios/.hidden.yaml",
            "scenarios/..",
            "scenarios/notes.txt",
            "README.md",
        ] {
            assert!(validate_rel_path(bad).is_err(), "{bad} は拒否すべき");
        }
    }

    /// 一覧: 直下のみ・YAML のみ・決定論的順序。campaign.yaml は在るときだけ。
    /// resolve は symlink 無しの通常構成で containment を通り、外指しの rel は形で落ちる。
    #[test]
    fn list_and_resolve_are_scoped_to_the_package() {
        let dir = std::env::temp_dir().join(format!("kataribe_editor_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("scenarios/sub")).unwrap();
        std::fs::create_dir_all(dir.join("characters")).unwrap();
        std::fs::write(dir.join("package.yaml"), "title: t\n").unwrap();
        std::fs::write(dir.join("scenarios/b.yaml"), "x: 1\n").unwrap();
        std::fs::write(dir.join("scenarios/a.yaml"), "x: 1\n").unwrap();
        std::fs::write(dir.join("scenarios/sub/deep.yaml"), "x: 1\n").unwrap();
        std::fs::write(dir.join("scenarios/note.txt"), "x").unwrap();
        std::fs::write(dir.join("characters/alice.yaml"), "name: a\n").unwrap();

        let files: Vec<String> = list_files(&dir).into_iter().map(|f| f.rel_path).collect();
        // campaign.yaml 不在 → 出ない / sub/ と .txt は出ない / フォルダ内は名前昇順。
        assert_eq!(
            files,
            vec!["package.yaml", "scenarios/a.yaml", "scenarios/b.yaml", "characters/alice.yaml"]
        );

        assert!(resolve_in_root(&dir, "scenarios/a.yaml").is_ok());
        assert!(resolve_in_root(&dir, "scenarios/sub/deep.yaml").is_err(), "段数超過は形で落ちる");
        assert!(resolve_in_root(&dir, "scenarios/missing.yaml").is_err(), "実在しないファイルは開けない");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// fork の順序 (spec 28 A.6): 書き込み成功でメタが消え、書き込み失敗ならメタは残る。
    /// メタが元々無ければ冪等に forked=true。
    #[test]
    fn fork_removes_meta_only_after_a_successful_write() {
        const META: &str = ".kataribe_source.json";
        let dir = std::env::temp_dir().join(format!("kataribe_editor_fork_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.yaml"), "title: t\n").unwrap();
        std::fs::write(dir.join(META), "{}").unwrap();

        // 書き込み失敗 (実在しないファイル = v1 は編集のみ) → メタは無傷。
        let err = save_with_fork(&dir, "scenarios/missing.yaml", "x", true, META);
        assert!(err.is_err());
        assert!(dir.join(META).is_file(), "保存が失敗したのにメタが消えた");

        // 書き込み成功 → 中身が替わり、メタが消え、forked=true。
        let (forked, warn) = save_with_fork(&dir, "package.yaml", "title: mine\n", true, META).unwrap();
        assert!(forked && warn.is_none());
        assert_eq!(std::fs::read_to_string(dir.join("package.yaml")).unwrap(), "title: mine\n");
        assert!(!dir.join(META).exists(), "フォークしたのにメタが残っている");

        // メタが既に無い → 冪等に成功 (手動配置と同じ状態への収束)。
        let (forked, warn) = save_with_fork(&dir, "package.yaml", "title: mine2\n", true, META).unwrap();
        assert!(forked && warn.is_none());

        // fork を頼まない保存はメタに触れない (手動配置の通常保存)。
        std::fs::write(dir.join(META), "{}").unwrap();
        let (forked, _) = save_with_fork(&dir, "package.yaml", "title: mine3\n", false, META).unwrap();
        assert!(!forked && dir.join(META).is_file());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 原子書き込み: 上書きが効き、tmp が残らない。
    #[test]
    fn atomic_write_replaces_and_leaves_no_tmp() {
        let dir = std::env::temp_dir().join(format!("kataribe_editor_aw_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("package.yaml");
        std::fs::write(&target, "old").unwrap();

        atomic_write(&target, "new contents").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new contents");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "tmp が残っている");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
