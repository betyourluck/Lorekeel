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

/// 新規ファイルの stem の検証 (spec 28 Phase D)。保守的に縛る (`resolve_asset` と同系):
/// 英数と `_` `-`、1〜64 字。既存ファイルの編集は縛らない (一覧は実在ファイルから作る) —
/// これは新規だけの門。character では**ファイル名 = EntityId** になるので、加えて
/// 予約 id (`player` = 主人公スロット / `party` = sentinel / `*` = ワイルドカード) を
/// 名指しで拒否する。
fn validate_new_stem(stem: &str, id_note: &str) -> Result<(), String> {
    if stem.is_empty() || stem.len() > 64 {
        return Err("ファイル名は 1〜64 文字で入力してください".to_string());
    }
    if !stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(format!("ファイル名に使えるのは英数字と _ - だけです ({id_note}): {stem}"));
    }
    Ok(())
}

/// fork のメタ削除 (書き込み/削除の**成功後**に呼ぶ。順序と冪等則は spec 28 A.6)。
fn fork_meta(root: &Path, fork: bool, meta_file: &str) -> (bool, Option<String>) {
    if !fork {
        return (false, None);
    }
    match std::fs::remove_file(root.join(meta_file)) {
        Ok(()) => (true, None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (true, None),
        Err(e) => (
            false,
            Some(format!("出所メタの削除に失敗しました (次の保存で再試行します): {e}")),
        ),
    }
}

/// 新規ファイルの作成 (spec 28 Phase D、2026-08-27 に 4 カテゴリへ一般化 = ユーザーFB)。
/// 雛形は**そのまま parse できる**形にする (作った瞬間に赤線が出る雛形は摩擦):
/// - character: `name` に stem (null は String で parse エラー・後で直せばよい)
/// - scenario: start + goal + 場所 1 つ (validate も通る最小盤面)
/// - memoria: tags + text (text は必須欄)
/// - campaign: **固定名 campaign.yaml** (stem 不要・在れば拒否)。作っただけでは
///   campaign-entry にならない — package.yaml の entry を差し替えるのは作者の判断
///
/// 重複は明示エラー (黙って開き直さない — 上書きの芽を作らない)。
/// fork の意味論は保存と同じ (新ファイルの追加も tree_hash を変える)。
pub fn create_file(
    root: &Path,
    category: &str,
    stem: &str,
    fork: bool,
    meta_file: &str,
) -> Result<(String, bool, Option<String>), String> {
    let (rel, template) = match category {
        "character" => {
            validate_new_stem(stem, "ファイル名がそのままキャラクターの id になります")?;
            if stem == gm_core::PLAYER || stem == gm_core::PARTY || stem == gm_core::WILDCARD {
                return Err(format!("「{stem}」は予約された id です (主人公/パーティ用)"));
            }
            (format!("characters/{stem}.yaml"), format!("name: {stem}\nprofile: \"\"\n"))
        }
        "memoria" => {
            validate_new_stem(stem, "ファイル名がそのまま伏線の id になります")?;
            (format!("memoria/{stem}.yaml"), "tags: []\ntext: \"\"\n".to_string())
        }
        "scenario" => {
            validate_new_stem(stem, "package.yaml の entry や campaign の modules から参照する名前です")?;
            (
                format!("scenarios/{stem}.yaml"),
                concat!(
                    "title: \"\"\n",
                    "start: start\n",
                    "goal: { kind: flag_is, key: goal_flag, value: true }\n",
                    "allowed_flags: [goal_flag]\n",
                    "locations:\n",
                    "  start:\n",
                    "    description: \"\"\n",
                )
                .to_string(),
            )
        }
        "campaign" => (
            "campaign.yaml".to_string(),
            concat!(
                "title: \"\"\n",
                "start: main\n",
                "modules:\n",
                "  main: scenarios/main.yaml\n",
                "edges: []\n",
            )
            .to_string(),
        ),
        other => return Err(format!("このカテゴリには作成できません: {other}")),
    };
    let path = root.join(&rel);
    if path.exists() {
        return Err(format!("{rel} は既にあります"));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("フォルダを作れません: {e}"))?;
    }
    atomic_write(&path, &template)?;
    let (forked, warning) = fork_meta(root, fork, meta_file);
    Ok((rel, forked, warning))
}

/// ファイルの削除 (2026-08-27 に v1 へ昇格 = ユーザーFB。リネームは v2 のまま)。
/// **package.yaml だけは拒否** (消すとパッケージごと読めなくなる = エディタの土台が消える)。
/// entry シナリオや campaign.yaml の削除は許す — 壊れることは層 2 の inspect が
/// エラーとして報告する (作者の再構成の途中経過を機械が先回りで禁止しない)。
/// 確認 (不可逆) は frontend の責務。fork の意味論は保存と同じ。
pub fn delete_file(
    root: &Path,
    rel: &str,
    fork: bool,
    meta_file: &str,
) -> Result<(bool, Option<String>), String> {
    if rel == "package.yaml" {
        return Err("package.yaml は削除できません (パッケージの土台です)".to_string());
    }
    let path = resolve_in_root(root, rel)?; // 実在 + containment
    std::fs::remove_file(&path).map_err(|e| format!("削除に失敗しました: {e}"))?;
    Ok(fork_meta(root, fork, meta_file))
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

    /// Phase D: 新規作成 4 カテゴリ — stem 検証 (character は予約 id 込み)・雛形が
    /// それぞれの型として parse できる・フォルダ不在でも作れる・重複は明示エラー・一覧に載る。
    #[test]
    fn create_file_writes_parseable_templates_for_every_category() {
        // stem 検証: 受ける / 拒否 (文字集合・長さ・character の予約 id)。
        let dir = std::env::temp_dir().join(format!("kataribe_editor_new_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.yaml"), "title: t\n").unwrap();

        for bad in ["", "アリス", "a.b", "a/b", "a b", &"x".repeat(65)] {
            assert!(create_file(&dir, "character", bad, false, ".meta").is_err(), "{bad}");
            assert!(create_file(&dir, "scenario", bad, false, ".meta").is_err(), "{bad}");
        }
        for reserved in ["player", "party", "*"] {
            assert!(create_file(&dir, "character", reserved, false, ".meta").is_err(), "{reserved}");
        }
        // 予約 id は character だけの縛り (memoria の promise_of_player 等は自由)。
        assert!(create_file(&dir, "memoria", "player", false, ".meta").is_ok());

        // character: 雛形が CharacterDef として parse でき、name に stem が入る。
        let (rel, forked, warn) = create_file(&dir, "character", "beryl", false, ".meta").unwrap();
        assert_eq!(rel, "characters/beryl.yaml");
        assert!(!forked && warn.is_none());
        let def: gm_core::CharacterDef =
            serde_yaml::from_str(&std::fs::read_to_string(dir.join(&rel)).unwrap()).unwrap();
        assert_eq!(def.name, "beryl");

        // scenario: 雛形が parse でき **validate も通る** (作った瞬間に壊れていない)。
        let (rel, ..) = create_file(&dir, "scenario", "chapter2", false, ".meta").unwrap();
        let sc: gm_core::Scenario =
            serde_yaml::from_str(&std::fs::read_to_string(dir.join(&rel)).unwrap()).unwrap();
        assert!(sc.validate().is_empty(), "{:?}", sc.validate());

        // memoria: text 必須欄が入って parse できる。
        let (rel, ..) = create_file(&dir, "memoria", "old_promise", false, ".meta").unwrap();
        let _: harness::MemoryFragment =
            serde_yaml::from_str(&std::fs::read_to_string(dir.join(&rel)).unwrap()).unwrap();

        // campaign: 固定名・stem 不要・二度目は拒否。parse できる。
        let (rel, ..) = create_file(&dir, "campaign", "", false, ".meta").unwrap();
        assert_eq!(rel, "campaign.yaml");
        assert!(harness::Campaign::from_yaml(&std::fs::read_to_string(dir.join(&rel)).unwrap()).is_ok());
        assert!(create_file(&dir, "campaign", "", false, ".meta").is_err());

        // 一覧に載る + 重複は明示エラー。
        let files = list_files(&dir);
        assert!(files.iter().any(|f| f.rel_path == "characters/beryl.yaml"));
        assert!(files.iter().any(|f| f.rel_path == "campaign.yaml"));
        assert!(create_file(&dir, "character", "beryl", false, ".meta").is_err());

        // fork の順序は保存と同じ: 作成成功でメタが消える。
        std::fs::write(dir.join(".meta"), "{}").unwrap();
        let (_, forked, _) = create_file(&dir, "character", "coral", true, ".meta").unwrap();
        assert!(forked && !dir.join(".meta").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 削除 (v1 昇格): package.yaml は拒否・実在ファイルは消える・fork は成功後・
    /// 実在しないファイルは明示エラー。
    #[test]
    fn delete_file_removes_but_protects_the_manifest() {
        let dir = std::env::temp_dir().join(format!("kataribe_editor_del_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("scenarios")).unwrap();
        std::fs::write(dir.join("package.yaml"), "title: t\n").unwrap();
        std::fs::write(dir.join("scenarios/old.yaml"), "title: t\n").unwrap();
        std::fs::write(dir.join(".meta"), "{}").unwrap();

        assert!(delete_file(&dir, "package.yaml", false, ".meta").is_err(), "土台は守る");
        assert!(delete_file(&dir, "scenarios/missing.yaml", false, ".meta").is_err());

        let (forked, warn) = delete_file(&dir, "scenarios/old.yaml", true, ".meta").unwrap();
        assert!(forked && warn.is_none());
        assert!(!dir.join("scenarios/old.yaml").exists());
        assert!(!dir.join(".meta").exists(), "fork は削除成功の後");

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
