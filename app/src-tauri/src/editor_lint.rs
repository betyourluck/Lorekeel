//! spec 28 Phase B: エディタ診断の二層。
//!
//! **検査規則はここに無い** — 層 1 (単一ファイル) は kind 別に parse + 既存 lint
//! (`manifest_lints` / `unknown_key_lints` / `character_lints`) を呼ぶだけ、
//! 層 2 (パッケージ全体) は `harness::inspect_package` を呼ぶだけ。ここが持つのは
//! **位置と帰属**: parse エラーの行 (serde_yaml `Location` = 正確) / 未知キー lint の
//! 行近似 (パスの親キー列で範囲を絞る前方検索。絞れなければ**位置なし** = 行を偽らない) /
//! 層 2 メッセージの接頭辞 → ファイルへの写し。

use std::collections::BTreeMap;

use serde::Serialize;

/// 層 1 の診断 1 件。`line` は 1 始まり (CodeMirror 側で範囲へ変換)、None = 位置なし。
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct EditorDiagnostic {
    pub line: Option<u32>,
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
        return Ok(vec![EditorDiagnostic { line, severity: "error".into(), message }]);
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
            line: lint_line(text, &message),
            severity: "warning".into(),
            message,
        })
        .collect())
}

/// lint 文言の接頭 (最初の ": " まで) を YAML パスとして行を近似する。
/// 形式は自前の lint が組む `{path}: 不明なフィールド…` (`unknown_keys` / `walk`)。
fn lint_line(text: &str, message: &str) -> Option<u32> {
    let path = message.split(": ").next()?;
    resolve_path_line(text, path)
}

/// パスの**親キー列を順に前方検索**して行を絞る (spec 28 C.1 — 終端キーの単純検索は
/// `id`/`name` 級の頻出キーで別の行に赤線を引く誤爆になるので採らない)。
///
/// `triggers[0].effects[1].entity` → セグメント `triggers` → `effects` → `entity` を
/// この順に、**前のヒット行より後**から探す (`[i]` は行に現れないので落とす。
/// mapping 名セグメント `stats.腕力` は「腕力:」として現れるのでそのまま探せる)。
/// どこかで見つからなければ None = 位置なし (flow style・引用キー等は諦めて行を偽らない)。
pub fn resolve_path_line(text: &str, path: &str) -> Option<u32> {
    let segs: Vec<&str> = path
        .split('.')
        .map(|s| s.split('[').next().unwrap_or(s))
        .filter(|s| !s.is_empty())
        .collect();
    if segs.is_empty() {
        return None;
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut from = 0usize;
    let mut found = None;
    for seg in segs {
        let mut hit = None;
        for (i, raw) in lines.iter().enumerate().skip(from) {
            let t = raw.trim_start();
            // 列挙要素の頭 (`- id: t1`) も拾う。
            let t = t.strip_prefix("- ").unwrap_or(t);
            if t.strip_prefix(seg).is_some_and(|rest| rest.starts_with(':')) {
                hit = Some(i);
                break;
            }
        }
        let i = hit?;
        found = Some(i);
        from = i + 1;
    }
    found.map(|i| (i + 1) as u32)
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
    }

    /// 未知キーは warning + 親キー列で絞った近似行。clean なら空。
    #[test]
    fn unknown_keys_are_warnings_with_narrowed_lines() {
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
        // 誤爆しない: hall 側 (9 行目) の exits でなく cell 側の typo 行へ。
        assert_eq!(d[0].line, Some(14), "{:?}", d[0]);
        assert!(d[0].message.contains("gaet"));

        // clean で空。
        let clean = "title: t\nstart: room\ngoal: { kind: flag_is, key: f, value: true }\nallowed_flags: [f]\nlocations:\n  room:\n    description: 部屋\n";
        assert!(lint_text("scenario", clean).unwrap().is_empty());
    }

    /// 頻出キー名の誤爆をしない: 早い場所に同名キーが在っても親キー列で絞る。
    /// 一意に絞れない (セグメントが見つからない) ときは位置なし = 行を偽らない。
    #[test]
    fn narrowing_does_not_bite_earlier_same_named_keys() {
        // characters kind: stats.HP の中の typo。上の profile にも「max」という語が
        // 出てくるが、stats → HP → max の親キー列で下の行に絞られる。
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
        assert_eq!(d[0].line, Some(8), "{:?}", d[0]);

        // flow style はセグメントが行として現れない → 位置なし (None) に落ちる。
        assert_eq!(resolve_path_line("a: {b: {c: 1}}\n", "a.b.c"), None);
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
