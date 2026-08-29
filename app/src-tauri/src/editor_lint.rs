//! spec 28 Phase B: エディタ診断の二層。
//!
//! **検査規則はここに無い** — 層 1 (単一ファイル) は kind 別に parse + 既存 lint
//! (`manifest_lints` / `unknown_key_lints` / `character_lints`) を呼ぶだけ、
//! 層 2 (パッケージ全体) は `harness::inspect_package` を呼ぶだけ。ここが持つのは
//! **位置と帰属**: parse エラーの行 (serde_yaml `Location` = 正確) / 未知キー lint の
//! **YAML パス** / 層 2 メッセージの接頭辞 → ファイルへの写し。
//!
//! **spec 28 v2: 行の近似をやめた** — v1 はパスの親キー列を前方検索して行を推し、
//! 絞れなければ位置なしにしていた (flow style・引用キーで諦める / 同名キーで誤爆しうる)。
//! いま返すのはパスそのもので、テキスト上の位置へ解くのは**構文木を持っている frontend**
//! の仕事になった (Lezer の木でノード範囲まで確定できる = 行でなく下線)。

use std::collections::BTreeMap;

use serde::Serialize;

/// 層 1 の診断 1 件。位置は二形式のどちらか (両方 None なら位置なし = 行を偽らない):
/// `line` = parse エラーの行 (1 始まり・serde_yaml の Location ゆえ正確) /
/// `path` = 未知キーの YAML パス (`triggers[0].effects[1].entity`)。frontend が木で範囲へ解く。
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct EditorDiagnostic {
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// "error" (parse = 読めない) | "warning" (未知キー = 読めるが効かない)。
    pub severity: String,
    pub message: String,
}

/// 層 2 の 1 件。`file` = 編集ルート相対 (引けなければ None = パッケージ全体の話)。
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct EditorIssue {
    pub file: Option<String>,
    pub severity: String,
    pub message: String,
}

/// kind 別の parse を試み、エラーなら (行, 文言)。
fn parse_error(kind: &str, text: &str) -> Result<Option<(Option<u32>, String)>, String> {
    fn err_of<T: serde::de::DeserializeOwned>(text: &str) -> Option<(Option<u32>, String)> {
        serde_yaml::from_str::<T>(text)
            .err()
            .map(|e| (e.location().map(|l| l.line() as u32), e.to_string()))
    }
    Ok(match kind {
        "manifest" => err_of::<harness::PackageManifest>(text),
        "scenario" => err_of::<gm_core::Scenario>(text),
        "character" => err_of::<gm_core::CharacterDef>(text),
        "campaign" => err_of::<harness::Campaign>(text),
        "memoria" => err_of::<harness::MemoryFragment>(text),
        other => return Err(format!("未知の kind: {other}")),
    })
}

/// 層 1: 単一ファイルで完結する検査だけ (spec 28 C.1)。
/// parse エラーが出たらそれだけを返す (未知キー lint は parse 失敗時に空を返す実装なので、
/// 続けても意味のある結果は出ない — 二重報告もしない)。
pub fn lint_text(kind: &str, text: &str) -> Result<Vec<EditorDiagnostic>, String> {
    if let Some((line, message)) = parse_error(kind, text)? {
        return Ok(vec![EditorDiagnostic {
            line,
            path: None,
            severity: "error".into(),
            message,
        }]);
    }
    // kind 別の未知キー lint。campaign / memoria は parse のみ (spec 28 — 専用 lint は
    // 現存せず、v1 では新設しない。実害が観測されたら character_lints と同型で足す)。
    let lints = match kind {
        "manifest" => harness::manifest_lints(text),
        "scenario" => gm_core::unknown_key_lints(text),
        "character" => harness::character_lints(text),
        _ => Vec::new(),
    };
    Ok(lints
        .into_iter()
        .map(|message| EditorDiagnostic {
            line: None,
            // 文言の接頭 (最初の ": " まで) が YAML パス — 形式は自前の lint が組む
            // `{path}: 不明なフィールド…` (`gm_core::unknown_keys` / `walk`)。
            path: message.split(": ").next().map(str::to_string),
            severity: "warning".into(),
            message,
        })
        .collect())
}

/// 層 2 の帰属: `inspect_package` のメッセージ接頭辞を編集ルート相対ファイルへ写す。
/// 接頭辞の形式は harness 側の実装 (character_warnings / prefixed_manifest_lints /
/// load_package / inspect_package の campaign 分岐) が組むもの:
/// `package.yaml:` / `characters/{name}:` / `{manifest.entry}:` / `modules.{id}:`。
/// どれでもなければ None (パッケージ全体の話として表示)。
pub fn attribute_file(
    message: &str,
    entry: &str,
    modules: &BTreeMap<String, String>,
) -> Option<String> {
    let prefix = message.split(": ").next()?;
    if prefix == "package.yaml" {
        return Some("package.yaml".to_string());
    }
    if prefix == entry {
        return Some(entry.to_string());
    }
    if let Some(name) = prefix.strip_prefix("characters/") {
        if name.ends_with(".yaml") || name.ends_with(".yml") {
            return Some(prefix.to_string());
        }
    }
    if let Some(id) = prefix.strip_prefix("modules.") {
        return modules.get(id).cloned();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// parse エラーは行位置つきの error 1 件だけを返す (未知キー lint と二重報告しない)。
    #[test]
    fn parse_errors_carry_a_precise_line() {
        // 3 行目のインデント破れ。
        let broken = "title: t\nstart: room\n   locations: {}\n";
        let d = lint_text("scenario", broken).unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].severity, "error");
        assert_eq!(d[0].line, Some(3), "serde_yaml の Location が載る: {:?}", d[0]);
        assert_eq!(d[0].path, None, "parse エラーに YAML パスは無い (読めていないので)");
    }

    /// 未知キーは warning + **YAML パス**。テキスト上の位置へ解くのは frontend (構文木) の
    /// 仕事なので、ここが約束するのはパスが正しいことだけ (spec 28 v2 で行の近似を撤去)。
    #[test]
    fn unknown_keys_are_warnings_with_a_yaml_path() {
        let src = concat!(
            "title: t\n",              // 1
            "start: cell\n",           // 2
            "goal: { kind: flag_is, key: f, value: true }\n", // 3
            "allowed_flags: [f]\n",    // 4
            "locations:\n",            // 5
            "  hall:\n",               // 6
            "    description: 広間\n", // 7
            "    exits:\n",            // 8
            "      - to: cell\n",      // 9
            "  cell:\n",               // 10
            "    description: 独房\n", // 11
            "    exits:\n",            // 12
            "      - to: hall\n",      // 13
            "        gaet: { kind: flag_is, key: f, value: true }\n", // 14 ← typo
        );
        let d = lint_text("scenario", src).unwrap();
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].severity, "warning");
        // 同名キー (hall 側にも exits が在る) を跨いで、typo のあるノードを一意に指す。
        assert_eq!(d[0].path.as_deref(), Some("locations.cell.exits[0].gaet"), "{:?}", d[0]);
        assert_eq!(d[0].line, None, "行は frontend が木で解く");
        assert!(d[0].message.contains("gaet"));

        // clean で空。
        let clean = "title: t\nstart: room\ngoal: { kind: flag_is, key: f, value: true }\nallowed_flags: [f]\nlocations:\n  room:\n    description: 部屋\n";
        assert!(lint_text("scenario", clean).unwrap().is_empty());
    }

    /// パスは名前つき map (`stats.HP`) を段ごとに含む — frontend はこれを木で辿るので、
    /// 早い場所の同名キーや、地の文に同じ語が出ることに影響されない。
    #[test]
    fn paths_include_named_map_segments() {
        // characters kind: stats.HP の中の typo。上の profile にも「max」という語が出る。
        let src = concat!(
            "name: アリス\n",              // 1
            "profile: max まで頑張る\n",   // 2
            "stats:\n",                    // 3
            "  好感度:\n",                 // 4
            "    initial: 0\n",            // 5
            "  HP:\n",                     // 6
            "    initial: 10\n",           // 7
            "    mox: 20\n",               // 8 ← typo (max)
        );
        let d = lint_text("character", src).unwrap();
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].path.as_deref(), Some("stats.HP.mox"), "{:?}", d[0]);

        // flow style でもパスは同じ形で出る (v1 の行近似はここで諦めていた)。
        let flow = "name: ア
stats: { HP: { initial: 1, mox: 2 } }
";
        let d = lint_text("character", flow).unwrap();
        assert_eq!(d[0].path.as_deref(), Some("stats.HP.mox"), "{:?}", d[0]);
    }

    /// manifest / campaign / memoria の kind 別: manifest は lint あり、
    /// campaign / memoria は parse のみ (壊れていれば error・読めれば未知キーでも黙る)。
    #[test]
    fn kinds_map_to_their_own_checks() {
        // manifest: 旧キー typo が warning。
        let m = "title: t\nentry: scenarios/x.yaml\nacts_policy: open\n";
        let d = lint_text("manifest", m).unwrap();
        assert!(d.iter().any(|x| x.severity == "warning" && x.message.contains("acts_policy")), "{d:?}");

        // memoria: text 必須 (欠落 = parse error)。読める形は未知キーがあっても黙る (v1)。
        assert!(lint_text("memoria", "tags: [a]\n").unwrap().iter().any(|x| x.severity == "error"));
        assert!(lint_text("memoria", "text: 伏線\ntagz: [a]\n").unwrap().is_empty());

        // campaign: 読めれば空・壊れたら error。
        assert!(lint_text("campaign", "title: c\nstart: a\nmodules: { a: s.yaml }\n").unwrap().is_empty());
        assert!(lint_text("campaign", "title: c\n  start: broken\n").unwrap().iter().any(|x| x.severity == "error"));

        // 未知 kind は Err (呼び出し側のバグを黙らせない)。
        assert!(lint_text("mystery", "a: 1\n").is_err());
    }

    /// 層 2 の帰属: 4 種の接頭辞 + どれでもなければ None。
    #[test]
    fn issue_attribution_maps_prefixes_to_files() {
        let modules: BTreeMap<String, String> =
            [("study".to_string(), "scenarios/study.yaml".to_string())].into();
        let entry = "scenarios/main.yaml";
        assert_eq!(
            attribute_file("package.yaml: 不明なフィールド…", entry, &modules).as_deref(),
            Some("package.yaml")
        );
        assert_eq!(
            attribute_file("characters/alice.yaml: 不明なフィールド…", entry, &modules).as_deref(),
            Some("characters/alice.yaml")
        );
        assert_eq!(
            attribute_file("scenarios/main.yaml: フラグ「x」の…", entry, &modules).as_deref(),
            Some("scenarios/main.yaml")
        );
        assert_eq!(
            attribute_file("modules.study: 未知のキー…", entry, &modules).as_deref(),
            Some("scenarios/study.yaml")
        );
        assert_eq!(attribute_file("modules.ghost: …", entry, &modules), None);
        assert_eq!(attribute_file("package の読み込み失敗 (…)", entry, &modules), None);
    }
}
