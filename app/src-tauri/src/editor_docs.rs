//! spec 28 Phase C-2 (2026-08-28): 補完候補の**説明**を、型のそばの doc comment から機械抽出する。
//!
//! **説明を手で書き写さない**のが核。エディタ用の説明表を新設すると、フィールドの意味が変わった
//! ときに「コードの doc」「data_contract」「補完の表」の三箇所を揃える羽目になり、必ずどれかが
//! 古くなる (掟の「撤去したら grep」が言う追従漏れの、説明版)。ゆえに一次情報は
//! **`crates/*/src/*.rs` の doc comment そのもの**とし、ここは抽出するだけにする。
//!
//! ソースは `include_str!` で**ビルド時に焼く** (実行時にリポジトリを探さない = 配布物でも効く)。
//! 抽出は行単位の走査で、対象は自分たちのコードだけなので完全な Rust パーサは要らない
//! (`syn` を build-dependency に足さない = 収縮側の判断)。**書式が変わったら PoC が落ちる**ので、
//! 静かに空になることはない。
//!
//! キーの形:
//! - `Scenario.world`          … struct のフィールド
//! - `StateOp::give_item`      … enum のバリアント (serde の snake_case 名 = YAML に書く値)
//! - `StateOp::give_item.from` … バリアントの中のフィールド
//!
//! doc が無いフィールドは**説明なし**で出す (無い説明を捏造しない)。

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// 抽出対象。**型の定義があるファイルだけ**を焼く (engine.rs のような実装本体は要らない)。
const SOURCES: &[&str] = &[
    include_str!("../../../crates/gm_core/src/spine.rs"),
    include_str!("../../../crates/gm_core/src/state.rs"),
    include_str!("../../../crates/harness/src/package.rs"),
    include_str!("../../../crates/harness/src/campaign.rs"),
    include_str!("../../../crates/harness/src/memoria.rs"),
];

/// `AddItem` -> `add_item` (serde の `rename_all = "snake_case"` と同じ写像)。
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// rustdoc のマークアップを剥がす — 補完のポップアップは素のテキストしか描かない。
fn tidy(doc: &str) -> String {
    let mut s = doc.replace("**", "");
    // `[`Scenario::transition`]` -> `Scenario::transition`
    while let Some(a) = s.find("[`") {
        let Some(rel) = s[a..].find("`]") else { break };
        let inner = s[a + 2..a + rel].to_string();
        s.replace_range(a..a + rel + 2, &inner);
    }
    s.replace('`', "").trim().to_string()
}

fn decl_name(line: &str, kw: &str) -> Option<String> {
    let rest = line.strip_prefix(kw)?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// enum のバリアント行 (`AddItem {` / `Always,` / `RemoveItem { item: ItemId },`)。
fn variant_name(line: &str) -> Option<String> {
    let head: String = line
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if !head.chars().next()?.is_uppercase() {
        return None;
    }
    let after = line[head.len()..].trim_start();
    (after.starts_with('{') || after.starts_with(',') || after.is_empty()).then_some(head)
}

/// フィールド行 (`pub name: T,` / `name: T,`)。型名 (大文字始まり) は弾く。
fn field_name(line: &str) -> Option<String> {
    let body = line.strip_prefix("pub ").unwrap_or(line);
    let name: String = body
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() || name.chars().next()?.is_uppercase() {
        return None;
    }
    let after = body[name.len()..].trim_start();
    after.starts_with(':').then_some(name)
}

/// 1 ファイルを走査して doc を集める。
fn scan(src: &str, out: &mut BTreeMap<String, String>) {
    let mut pending: Vec<String> = Vec::new();
    let mut ty: Option<String> = None;
    let mut is_enum = false;
    let mut variant: Option<String> = None;
    let mut depth = 0i32; // 型の本体に入ってからの波括弧の深さ

    for raw in src.lines() {
        let line = raw.trim();

        if let Some(rest) = line.strip_prefix("///") {
            pending.push(rest.trim().to_string());
            continue;
        }
        // 空行・属性・通常コメントは doc を捨てない (`#[serde(default)]` が間に挟まるのが常態)。
        if line.is_empty() || line.starts_with("#[") || line.starts_with("//") {
            continue;
        }
        let doc = (!pending.is_empty()).then(|| tidy(&pending.join(" ")));
        pending.clear();

        if depth == 0 {
            for (kw, en) in [("pub struct ", false), ("pub enum ", true)] {
                if let Some(name) = decl_name(line, kw) {
                    if line.ends_with('{') {
                        ty = Some(name);
                        is_enum = en;
                        variant = None;
                        depth = 1;
                    }
                    break;
                }
            }
            continue;
        }

        let Some(tname) = ty.clone() else {
            depth = 0;
            continue;
        };

        if is_enum && depth == 1 {
            if let Some(v) = variant_name(line) {
                let snake_v = snake(&v);
                if let Some(d) = &doc {
                    out.entry(format!("{tname}::{snake_v}"))
                        .or_insert_with(|| d.clone());
                }
                variant = Some(snake_v);
                depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
                continue;
            }
        }

        if let Some(field) = field_name(line) {
            let key = match (&variant, is_enum) {
                (Some(v), true) => format!("{tname}::{v}.{field}"),
                _ => format!("{tname}.{field}"),
            };
            if let Some(d) = &doc {
                out.entry(key).or_insert_with(|| d.clone());
            }
        }

        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
        if depth <= 0 {
            ty = None;
            variant = None;
            depth = 0;
        } else if is_enum && depth == 1 {
            variant = None;
        }
    }
}

/// 抽出済みの doc 表 (プロセスで 1 回だけ走査)。
pub fn docs() -> &'static BTreeMap<String, String> {
    static DOCS: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    DOCS.get_or_init(|| {
        let mut out = BTreeMap::new();
        for src in SOURCES {
            scan(src, &mut out);
        }
        out
    })
}

/// `Type.field` を引く。無ければ None (説明なしで候補は出す = 無い説明を捏造しない)。
pub fn field_doc(ty: &str, field: &str) -> Option<&'static str> {
    docs().get(&format!("{ty}.{field}")).map(String::as_str)
}

/// `Type::variant` を引く (`op: give_item` のような**値**の説明)。
pub fn variant_doc(ty: &str, variant: &str) -> Option<&'static str> {
    docs().get(&format!("{ty}::{variant}")).map(String::as_str)
}

/// `Type::variant.field` を引く。**バリアント自身の doc へは落とさない** — 欄の説明として
/// 読まれるので、別のものを指す文を出すと誤誘導になる。
pub fn variant_field_doc(ty: &str, variant: &str, field: &str) -> Option<&'static str> {
    docs()
        .get(&format!("{ty}::{variant}.{field}"))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実ソースから doc が取れている。**書式が変われば落ちる** = 静かに空にならないことの表明。
    #[test]
    fn docs_are_extracted_from_the_real_sources() {
        let d = docs();
        let world = field_doc("Scenario", "world").expect("Scenario.world");
        assert!(world.contains("世界観"), "{world}");
        assert!(field_doc("Location", "present").is_some(), "Location.present");
        assert!(field_doc("CharacterDef", "profile").is_some());
        assert!(field_doc("PackageManifest", "entry").is_some());
        // enum バリアントは YAML に書く snake_case 名で引ける。
        let give = variant_doc("StateOp", "give_item").expect("StateOp::give_item");
        assert!(give.contains("譲渡"), "{give}");
        assert!(variant_doc("Gate", "has_item").is_some(), "Gate::has_item");
        // マークアップは剥がれている。
        assert!(!d.values().any(|v| v.contains("**") || v.contains('`')));
        // 量の下限 (走査が途中で死んでいない)。
        assert!(d.len() > 120, "抽出数 {}", d.len());
    }

    #[test]
    fn snake_matches_serde_rename_all() {
        assert_eq!(snake("AddItem"), "add_item");
        assert_eq!(snake("Always"), "always");
        assert_eq!(snake("AttemptChallenge"), "attempt_challenge");
    }
}
