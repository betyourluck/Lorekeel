//! 作者向け仕様の断片 (spec 29 Phase B、2026-09-06) — `docs/package_spec/` を**正本**として
//! ビルド時に焼き、①AI 編集 (spec 29 Phase C) がファイル種別ごとに渡す断片 ②サイト版
//! `docs/package_spec.md` の連結 (`play spec-assemble`) ③Gate / op の列挙 (`play spec-vocab`)
//! の 3 つを一箇所から出す。
//!
//! **列挙は型から機械生成する** (spec 29 決定 7)。旧 `package_spec.md` は Gate 13 種と op 12 種を
//! 手書きの YAML で列挙しており、型に値が増えても誰かが写さない限り古いままだった。ここでは
//! `gm_core::gate_variant_keys` / `op_variant_keys` (lint と補完が見る表そのもの) と
//! [`crate::docs`] の doc comment から表を組む — **補完に出るのに仕様書に無い**乖離が構造的に
//! 起きない。断片側は `<!-- vocab:gate -->` / `<!-- vocab:op -->` のマーカーだけを持つ。
//!
//! 索引 (`index.yaml`) は連結順・kind ごとの基本断片・話題断片 (要旨 + 辞書の語) を持つ。
//! 断片の実体は `include_str!` (配布物でも効く = 実行時にリポジトリを探さない)。

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// 断片 1 つ (id = ファイル名の stem)。
pub struct Slice {
    pub id: &'static str,
    pub text: &'static str,
}

macro_rules! slice {
    ($id:literal) => {
        Slice { id: $id, text: include_str!(concat!("../../../docs/package_spec/", $id, ".md")) }
    };
}

/// 焼き込んだ断片の全部。**索引と 1:1** であることは PoC が固定する (片方に足して片方に
/// 足し忘れると落ちる)。
pub const SLICES: &[Slice] = &[
    slice!("00_header"),
    slice!("01_principles"),
    slice!("10_manifest"),
    slice!("20_scenario"),
    slice!("21_scenario_image_hold"),
    slice!("22_scenario_max_per_turn"),
    slice!("23_scenario_entity_threshold"),
    slice!("24_scenario_percentile"),
    slice!("25_scenario_certain_action"),
    slice!("26_scenario_expr"),
    slice!("27_scenario_push_buy"),
    slice!("28_scenario_contests"),
    slice!("30_gate"),
    slice!("31_gate_wildcard"),
    slice!("32_gate_presence_is"),
    slice!("33_gate_party"),
    slice!("40_ops"),
    slice!("41_ops_volatile_presence"),
    slice!("42_ops_trigger_challenge"),
    slice!("43_ops_trigger_move"),
    slice!("50_visibility"),
    slice!("51_facts_policy"),
    slice!("52_tts"),
    slice!("53_image_style"),
    slice!("60_characters"),
    slice!("61_memoria"),
    slice!("62_campaign"),
    slice!("70_hidden_roles"),
    slice!("80_conventions"),
    slice!("81_load_errors"),
];

const INDEX_YAML: &str = include_str!("../../../docs/package_spec/index.yaml");

/// `index.yaml` の形。
#[derive(Debug, Clone, Deserialize)]
pub struct SpecIndex {
    /// サイト版の連結順。
    pub order: Vec<String>,
    /// kind を問わず常に渡す断片。
    pub common: Vec<String>,
    /// kind ごとに常に渡す断片 (manifest / scenario / character / memoria / campaign)。
    pub base: BTreeMap<String, Vec<String>>,
    /// 指示か本文に当たったとき渡す断片 + `spec` 道具で引ける話題。
    pub topics: Vec<Topic>,
}

/// 話題断片 1 つ。`keywords` は辞書 (最適化)、`summary` は system に載る 1 行要旨 (`spec` の入口)。
#[derive(Debug, Clone, Deserialize)]
pub struct Topic {
    pub id: String,
    pub kind: String,
    pub slice: String,
    pub summary: String,
    pub keywords: Vec<String>,
}

/// 編集モードのファイル種別 (`lint_editor_text` の kind と同じ語彙)。
pub const KINDS: &[&str] = &["manifest", "scenario", "character", "memoria", "campaign"];

/// 索引 (プロセスで 1 回だけ parse)。
pub fn index() -> &'static SpecIndex {
    static INDEX: OnceLock<SpecIndex> = OnceLock::new();
    INDEX.get_or_init(|| serde_yaml::from_str(INDEX_YAML).expect("docs/package_spec/index.yaml は parse できる"))
}

/// 断片の本文を id で引く。
pub fn slice(id: &str) -> Option<&'static str> {
    SLICES.iter().find(|s| s.id == id).map(|s| s.text)
}

/// Markdown の表セルに載せる (縦棒と改行を潰す)。
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn fields_cell(fields: &std::collections::BTreeSet<String>, tag: &str) -> String {
    let names: Vec<String> = fields.iter().filter(|f| f.as_str() != tag).map(|f| format!("`{f}`")).collect();
    if names.is_empty() { "—".into() } else { names.join(", ") }
}

/// Gate の列挙 (型から機械生成)。
pub fn gate_vocab_markdown() -> String {
    let keys = gm_core::gate_variant_keys();
    let mut out = format!(
        "{} 種類 (エンジンの型から機械生成 — **これ以外の `kind` は存在しない**)。`entity` は省略時に主人公 (`player`)。\n\n| kind | 欄 | 意味 |\n|---|---|---|\n",
        keys.len()
    );
    for (kind, fields) in &keys {
        let doc = crate::docs::variant_doc("Gate", kind).unwrap_or("");
        out.push_str(&format!("| `{kind}` | {} | {} |\n", fields_cell(fields, "kind"), cell(doc)));
    }
    out
}

/// op の列挙 (型から機械生成)。作者専権 (`AUTHORED_ONLY_OPS`) を列に出す。
pub fn op_vocab_markdown() -> String {
    let keys = gm_core::op_variant_keys();
    let mut out = format!(
        "{} 種類 (エンジンの型から機械生成 — **これ以外の `op` は存在しない**)。「作者のみ」は trigger / challenge の effects からだけ使え、GM (AI) が提案すると却下される。\n\n| op | 欄 | 誰が使えるか | 意味 |\n|---|---|---|---|\n",
        keys.len()
    );
    for (op, fields) in &keys {
        let who = if gm_core::AUTHORED_ONLY_OPS.contains(&op.as_str()) {
            "作者のみ (effects)"
        } else {
            "GM の提案も effects も可"
        };
        let doc = crate::docs::variant_doc("StateOp", op).unwrap_or("");
        out.push_str(&format!("| `{op}` | {} | {who} | {} |\n", fields_cell(fields, "op"), cell(doc)));
    }
    out
}

/// 両方の列挙 (`play spec-vocab` の出力)。
pub fn vocab_markdown() -> String {
    format!("### Gate の一覧\n\n{}\n### op の一覧\n\n{}", gate_vocab_markdown(), op_vocab_markdown())
}

/// 断片のマーカーを列挙に置き換える (AI 編集の system に載せるときも同じ形)。
pub fn expand(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        match line.trim_end() {
            "<!-- vocab:gate -->" => out.push_str(&gate_vocab_markdown()),
            "<!-- vocab:op -->" => out.push_str(&op_vocab_markdown()),
            _ => out.push_str(line),
        }
    }
    out
}

/// サイト版 `docs/package_spec.md` (`play spec-assemble` の出力) = 索引の順に断片を連結し、
/// マーカーを列挙へ展開したもの。
pub fn assemble() -> String {
    let mut out = String::new();
    for id in &index().order {
        let text = slice(id).unwrap_or_else(|| panic!("index.yaml の order にある断片 {id} が SLICES に無い"));
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&expand(text));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn ids() -> BTreeSet<&'static str> {
        SLICES.iter().map(|s| s.id).collect()
    }

    /// 索引と焼き込みが 1:1。片方に足して片方に足し忘れると落ちる。話題の kind は編集モードの
    /// 語彙、80/81 は LLM に渡さない (base / common / topics のどこにも出ない)。
    #[test]
    fn index_and_slices_agree() {
        let idx = index();
        let slice_ids = ids();
        let order: BTreeSet<&str> = idx.order.iter().map(String::as_str).collect();
        assert_eq!(order, slice_ids, "order と SLICES がずれている");
        assert_eq!(idx.order.len(), slice_ids.len(), "order に重複がある");
        let mut referenced: BTreeSet<&str> = BTreeSet::new();
        for id in &idx.common {
            assert!(slice_ids.contains(id.as_str()), "common の {id} が無い");
            referenced.insert(id);
        }
        for (kind, list) in &idx.base {
            assert!(KINDS.contains(&kind.as_str()), "base の kind {kind} は編集モードの語彙に無い");
            for id in list {
                assert!(slice_ids.contains(id.as_str()), "base.{kind} の {id} が無い");
                referenced.insert(id);
            }
        }
        for kind in KINDS {
            assert!(idx.base.contains_key(*kind), "base に {kind} が無い");
        }
        let mut topic_ids = BTreeSet::new();
        for t in &idx.topics {
            assert!(topic_ids.insert(t.id.as_str()), "話題 id 重複: {}", t.id);
            assert!(KINDS.contains(&t.kind.as_str()), "話題 {} の kind {}", t.id, t.kind);
            assert!(slice_ids.contains(t.slice.as_str()), "話題 {} の断片 {} が無い", t.id, t.slice);
            assert!(!t.summary.trim().is_empty() && !t.keywords.is_empty(), "話題 {} に要旨か語が無い", t.id);
            referenced.insert(t.slice.as_str());
        }
        for never in ["00_header", "80_conventions", "81_load_errors"] {
            assert!(!referenced.contains(never), "{never} は LLM に渡さない断片");
        }
    }

    /// 列挙は型の全バリアントを含む (手書きの 13 種 / 12 種ではなく、型が増えれば表も増える)。
    #[test]
    fn vocab_lists_every_gate_and_op_from_the_types() {
        let g = gate_vocab_markdown();
        for kind in gm_core::gate_variant_keys().keys() {
            assert!(g.contains(&format!("| `{kind}` |")), "gate {kind} が表に無い");
        }
        assert!(g.contains("| `not` |") && g.contains("| `presence_is` |"));
        let o = op_vocab_markdown();
        for op in gm_core::op_variant_keys().keys() {
            assert!(o.contains(&format!("| `{op}` |")), "op {op} が表に無い");
        }
        assert!(o.contains("| `grant_skill` | `entity`, `skill` | 作者のみ (effects) |"), "{o}");
        assert!(o.contains("| `set_flag` | `key`, `value` | GM の提案も effects も可 |"), "{o}");
        // 断片側にはマーカーだけがあり、展開で消える。
        assert!(slice("30_gate").unwrap().contains("<!-- vocab:gate -->"));
        assert!(!expand(slice("30_gate").unwrap()).contains("<!-- vocab:"));
    }

    /// サイト版 `docs/package_spec.md` は連結の写しであること (鮮度)。断片や型を変えたら
    /// `UPDATE_PACKAGE_SPEC=1 cargo test -p harness package_spec` で作り直す。
    #[test]
    fn assembled_site_spec_is_fresh() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/package_spec.md");
        let want = assemble();
        assert!(!want.contains("<!-- vocab:"), "マーカーが展開されていない");
        assert!(want.starts_with("# Lorekeel パッケージ作成仕様"));
        if std::env::var("UPDATE_PACKAGE_SPEC").is_ok() {
            std::fs::write(path, &want).unwrap();
            return;
        }
        let have = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{path} が読めない ({e}) — UPDATE_PACKAGE_SPEC=1 で生成する"));
        assert!(have == want, "docs/package_spec.md が古い — UPDATE_PACKAGE_SPEC=1 cargo test -p harness package_spec で作り直す");
    }
}
