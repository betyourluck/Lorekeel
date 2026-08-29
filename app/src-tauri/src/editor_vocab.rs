//! spec 28 Phase C: エディタ補完の語彙。
//!
//! **値は全部 型から導出** (`gm_core::struct_keys` / `gate_variant_keys` / `op_variant_keys`
//! = lint と同じ表。「補完に出るのに lint に叱られる」乖離を構造的に作らない)。
//! **配線 (どの親キーの下がどの型か) だけは手書きの表** — これは lint.rs `walk` が持つ
//! 構造知識の写しで、v1 のドリフトは許容する (ずれても補完が古いだけで lint は正しい。
//! 統合には walk のデータ駆動化が要る = v2)。
//!
//! **説明 (2026-08-28)**: 候補には doc comment 由来の説明が付く ([`crate::editor_docs`])。
//! 表を手で持たないので、フィールドの意味が変わればコードの doc を直すだけで補完も直る。
//!
//! id 語彙は編集ルートのパッケージ (ディスクの保存済み状態) から集める。**parse できれば
//! 集める** — `validate` は通さない (2026-08-28)。書きかけの盤面は幻フラグや幻 goal を普通に
//! 含むので、整合性を条件にすると**補完が最も要る時にだけ語彙が消える**。整合性は診断
//! (層 1/層 2) の仕事で、補完の仕事ではない (役割分離)。読めないものはスキップ。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::editor_docs;

/// 候補 1 つ。**説明 (`doc`) は型のそばの doc comment から機械抽出**したもので、ここが表を
/// 持つことはない。無い説明は `None` のまま出す (捏造しない)。
#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct VocabItem {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
}

impl VocabItem {
    fn bare(name: impl Into<String>) -> Self {
        Self { name: name.into(), doc: None }
    }
    fn with(name: impl Into<String>, doc: Option<&str>) -> Self {
        Self { name: name.into(), doc: doc.map(str::to_string) }
    }
}

/// 型名の最終セグメント (`gm_core::spine::Scenario` → `Scenario`) = doc 表の引き key。
fn short_type_name<T: ?Sized>() -> &'static str {
    std::any::type_name::<T>().rsplit("::").next().unwrap_or_default()
}

/// 補完語彙一式。frontend の completion source がこれだけを見る。
#[derive(Serialize, Clone, Debug, Default)]
pub struct EditorVocabulary {
    /// kind (manifest/scenario/character/campaign/memoria) → **ルート文脈名**。
    pub roots: BTreeMap<String, String>,
    /// 文脈名 → その mapping のキー候補 (doc 付き)。**gm_core の [`gm_core::context_keys`] が正**で、
    /// ここが足すのは harness 側の型 (manifest / campaign / memoria) だけ。
    pub contexts: BTreeMap<String, Vec<VocabItem>>,
    /// 配線 (親文脈 + キー → 子文脈)。frontend は構文木で得たパスをこれで辿り、
    /// カーソル位置の文脈を確定する。**gm_core の表そのもの** (写しを持たない)。
    pub wiring: Vec<gm_core::WiringEntry>,
    /// `kind` の値 → その Gate バリアントのキー (lint と同じ表)。
    pub gate_variant_keys: BTreeMap<String, Vec<VocabItem>>,
    /// `op` の値 → その StateOp バリアントのキー。
    pub op_variant_keys: BTreeMap<String, Vec<VocabItem>>,
    /// タグ欄の値候補: "kind" → Gate バリアント名 / "op" → StateOp バリアント名。
    /// **説明はバリアントの doc** — 「この op が何をするか」が補完で読める。
    pub tag_values: BTreeMap<String, Vec<VocabItem>>,
    /// 宣言済み id: locations / flags / items / entities / challenges / contests /
    /// skills / goals / stats / modules / images / audios。**死んだ参照 lint と同じ誤りの
    /// 上流予防**。説明は作者の付けた表示名 (場所の title・キャラの name・フラグの表示名)。
    pub ids: BTreeMap<String, Vec<VocabItem>>,
}

/// 型 T のキー候補 (doc 付き)。
fn keys_of<T: DeserializeOwned + Serialize>(sample: &str) -> Vec<VocabItem> {
    let ty = short_type_name::<T>();
    gm_core::struct_keys::<T>(sample)
        .into_iter()
        .map(|k| {
            let doc = editor_docs::field_doc(ty, &k);
            VocabItem::with(k, doc)
        })
        .collect()
}

/// harness 側 (gm_core が知らない配布レイアウトの型) の文脈名。
/// gm_core の文脈名 (`Scenario` 等) と衝突しないこと。
const CTX_MANIFEST: &str = "Manifest";
const CTX_PLAYER: &str = "PlayerDef";
const CTX_GLOBALS: &str = "Globals";
const CTX_CAMPAIGN: &str = "Campaign";
const CTX_CAMPAIGN_EDGE: &str = "CampaignEdge";
const CTX_MEMORIA: &str = "MemoryFragment";

/// 型由来のキー語彙を組む (パッケージに依らない静的部分)。
///
/// **配線も語彙も gm_core が正** ([`gm_core::wiring`] / [`gm_core::context_keys`])。
/// ここが手で持つのは harness 側の型の分だけ = 下の [`harness_wiring`] 4 本で、
/// それらは `manifest_lints` / `Campaign` の構造そのままである。
fn build_key_tables() -> EditorVocabulary {
    let mut v = EditorVocabulary::default();

    // ルート文脈 (kind 別)。
    for (kind, ctx) in [
        ("manifest", CTX_MANIFEST),
        ("scenario", gm_core::CTX_SCENARIO),
        ("character", gm_core::CTX_CHARACTER),
        ("campaign", CTX_CAMPAIGN),
        ("memoria", CTX_MEMORIA),
    ] {
        v.roots.insert(kind.into(), ctx.into());
    }

    // gm_core の全文脈 (キーは型から / 説明は型名で doc を引く)。
    for (name, info) in gm_core::context_keys() {
        let ty = info.type_name.clone();
        let items = info
            .keys
            .iter()
            .map(|k| {
                let doc = ty.as_deref().and_then(|t| editor_docs::field_doc(t, k));
                VocabItem::with(k, doc)
            })
            .collect();
        v.contexts.insert(name, items);
    }
    // Location.items 新形式 {when, take} は素の型を持たない (gm_core も手で並べている) ので、
    // ここだけ短い説明を添える。
    v.contexts.insert(
        "LocationItem".into(),
        vec![
            VocabItem::with(
                "take",
                Some("取得様式: once=一度きり (既定) / infinite=何度でも / fixed=備え付けで取得不可"),
            ),
            VocabItem::with("when", Some("拾える条件 (Gate)")),
        ],
    );

    // harness 側の文脈。
    v.contexts.insert(CTX_MANIFEST.into(), keys_of::<harness::PackageManifest>("entry: x"));
    v.contexts.insert(CTX_PLAYER.into(), keys_of::<harness::PlayerDef>("{}"));
    v.contexts.insert(CTX_GLOBALS.into(), keys_of::<harness::Globals>("{}"));
    v.contexts.insert(
        CTX_CAMPAIGN.into(),
        keys_of::<harness::Campaign>("start: a
modules: { a: x.yaml }
edges: []"),
    );
    v.contexts.insert(
        CTX_CAMPAIGN_EDGE.into(),
        keys_of::<harness::CampaignEdge>("from: a
on_goal: g
to: b"),
    );
    v.contexts.insert(CTX_MEMORIA.into(), keys_of::<harness::MemoryFragment>("text: x"));

    // 配線 = gm_core の表 + harness 側の 4 本。
    v.wiring = gm_core::wiring();
    v.wiring.extend(harness_wiring());

    // バリアント表 + タグ値。
    let gates = gm_core::gate_variant_keys();
    let ops = gm_core::op_variant_keys();
    // 値の説明はバリアントの doc (「この op が何をするか」)。
    v.tag_values.insert(
        "kind".into(),
        gates
            .keys()
            .map(|k| VocabItem::with(k, editor_docs::variant_doc("Gate", k)))
            .collect(),
    );
    v.tag_values.insert(
        "op".into(),
        ops.keys()
            .map(|k| VocabItem::with(k, editor_docs::variant_doc("StateOp", k)))
            .collect(),
    );
    // バリアント内のフィールドは、その欄自身に doc があるときだけ説明を付ける
    // (バリアントの doc へ落とすと、欄の説明として別のものを指す文が出てしまう)。
    v.gate_variant_keys = variant_items(gates, "Gate");
    v.op_variant_keys = variant_items(ops, "StateOp");
    v
}

/// harness 側の配線。**gm_core が知らない型の分だけ**で、内容は `manifest_lints` が
/// 見ている入れ子 (`player` / `globals`) と `Campaign` の形そのもの。
/// `player.stats` は `Scenario.initial_stats` と同じ StatInit 意味論 (mapping 値が StatDecl)。
fn harness_wiring() -> Vec<gm_core::WiringEntry> {
    let e = |parent: &str, key: &str, kind: &str, child: &str| gm_core::WiringEntry {
        parent: parent.into(),
        key: key.into(),
        kind: kind.into(),
        child: child.into(),
        child_tagged: None,
    };
    vec![
        e(CTX_MANIFEST, "player", "direct", CTX_PLAYER),
        e(CTX_MANIFEST, "globals", "direct", CTX_GLOBALS),
        e(CTX_PLAYER, "stats", "stat_map", "StatDecl"),
        e(CTX_CAMPAIGN, "edges", "seq", CTX_CAMPAIGN_EDGE),
    ]
}

/// バリアント表 (タグ値 → キー) を doc 付きの候補へ。
fn variant_items(
    table: BTreeMap<String, BTreeSet<String>>,
    ty: &str,
) -> BTreeMap<String, Vec<VocabItem>> {
    table
        .into_iter()
        .map(|(k, fields)| {
            let items = fields
                .into_iter()
                .map(|f| VocabItem::with(&f, editor_docs::variant_field_doc(ty, &k, &f)))
                .collect();
            (k, items)
        })
        .collect()
}

/// id 語彙の集積器: カテゴリ → (id → 表示名)。先に入った説明が勝つ (複数モジュールの和)。
type IdSets = BTreeMap<String, BTreeMap<String, Option<String>>>;

fn add_id(sets: &mut IdSets, cat: &str, id: impl Into<String>, doc: Option<String>) {
    let slot = sets.entry(cat.to_string()).or_default().entry(id.into()).or_default();
    if slot.is_none() {
        if let Some(d) = doc.filter(|d| !d.trim().is_empty()) {
            *slot = Some(head(&d));
        }
    }
}

/// 説明は 1 行に畳む (補完の脇に出るので長い本文は切る)。
fn head(s: &str) -> String {
    let one = s.replace('\n', " ");
    let t = one.trim();
    if t.chars().count() > 60 {
        format!("{}…", t.chars().take(60).collect::<String>())
    } else {
        t.to_string()
    }
}

/// 1 枚の Scenario から id 語彙を吸い上げる (集合に足すだけ)。
fn collect_scenario_ids(sc: &gm_core::Scenario, sets: &mut IdSets) {
    for (id, loc) in &sc.locations {
        // 場所は title (無ければ説明の頭) を見せる — id は機械用キーなので何の場所か分からない。
        let doc = if loc.title.trim().is_empty() {
            loc.description.clone()
        } else {
            loc.title.clone()
        };
        add_id(sets, "locations", id, Some(doc));
        for item in loc.items.keys() {
            add_id(sets, "items", item, None);
        }
    }
    for f in &sc.allowed_flags {
        let doc = sc
            .flag_titles
            .get(f)
            .cloned()
            .or_else(|| sc.flag_hints.get(f).cloned());
        add_id(sets, "flags", f, doc);
    }
    for (id, ch) in &sc.challenges {
        add_id(sets, "challenges", id, Some(ch.description.clone()));
    }
    for id in sc.contests.keys() {
        add_id(sets, "contests", id, None);
    }
    for g in &sc.goals {
        let doc = if g.title.trim().is_empty() { g.hint.clone() } else { g.title.clone() };
        add_id(sets, "goals", &g.id, Some(doc));
    }
    for s in &sc.initial_skills {
        add_id(sets, "skills", s, None);
    }
    for s in sc.initial_stats.keys() {
        add_id(sets, "stats", s, None);
    }
    for i in &sc.initial_inventory {
        add_id(sets, "items", i, None);
    }
    let hero = if sc.protagonist.name.trim().is_empty() {
        "主人公".to_string()
    } else {
        sc.protagonist.name.clone()
    };
    add_id(sets, "entities", "player", Some(hero));
    for (id, c) in &sc.characters {
        add_id(sets, "entities", id, Some(c.name.clone()));
        for s in &c.skills {
            add_id(sets, "skills", s, None);
        }
        for s in c.stats.keys() {
            add_id(sets, "stats", s, None);
        }
        for i in &c.inventory {
            add_id(sets, "items", i, None);
        }
    }
}

/// `characters/*.yaml` を**直に**読む。cast 宣言に依らない — エディタでは「まだ cast に
/// 載せていないキャラ」こそ候補に要る (ファイル名が EntityId、という既存の規則そのまま)。
fn collect_character_files(root: &Path, sets: &mut IdSets) {
    let Ok(rd) = std::fs::read_dir(root.join("characters")) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let def: Option<gm_core::CharacterDef> = serde_yaml::from_str(&text).ok();
        add_id(sets, "entities", id, def.as_ref().map(|c| c.name.clone()));
        if let Some(c) = def {
            for s in &c.skills {
                add_id(sets, "skills", s, None);
            }
            for s in c.stats.keys() {
                add_id(sets, "stats", s, None);
            }
            for i in &c.inventory {
                add_id(sets, "items", i, None);
            }
        }
    }
}

/// パッケージから id 語彙を集める。**parse できたものを全部足す**。
///
/// 見る先は「entry 1 枚」ではなく**フォルダ**: `scenarios/*.yaml` の和、campaign のモジュール、
/// `characters/*.yaml`、package.yaml の注入分 (globals.flags / player.*)。エディタで開いて
/// いるのは entry とは限らず、注入は load 時に起きるので entry を読むだけでは package 由来の
/// フラグが落ちる。読めないもの (壊れた YAML) はスキップ — 補完が減るだけで、壊れていること
/// 自体は診断が報告する。
fn collect_ids(root: &Path) -> BTreeMap<String, Vec<VocabItem>> {
    let mut sets: IdSets = BTreeMap::new();

    collect_character_files(root, &mut sets);

    // package.yaml の注入分 (globals.flags は allowed_flags へ union されるので語彙に要る)。
    // 読むのは**生ファイル** — `load_package` 系は entry 解決や検証まで進むので、書きかけでは
    // 途中で落ちる。ここが欲しいのは宣言だけ。
    if let Ok(text) = std::fs::read_to_string(root.join("package.yaml")) {
        if let Ok(m) = serde_yaml::from_str::<harness::PackageManifest>(&text) {
            if let Some(g) = &m.globals {
                for f in &g.flags {
                    add_id(&mut sets, "flags", f, None);
                }
            }
            if let Some(p) = &m.player {
                for s in p.stats.keys() {
                    add_id(&mut sets, "stats", s, None);
                }
                for s in &p.skills {
                    add_id(&mut sets, "skills", s, None);
                }
                for i in &p.items {
                    add_id(&mut sets, "items", i, None);
                }
                if !p.name.trim().is_empty() {
                    add_id(&mut sets, "entities", "player", Some(p.name.clone()));
                }
            }
        }
    }

    // campaign があればモジュール id (実体は下の scenarios/ 走査が拾う)。
    if let Ok(text) = std::fs::read_to_string(root.join("campaign.yaml")) {
        if let Ok(c) = harness::Campaign::from_yaml(&text) {
            for (id, rel) in &c.modules {
                add_id(&mut sets, "modules", id, Some(rel.clone()));
            }
        }
    }

    // scenarios/*.yaml をすべて (parse できたものだけ)。
    if let Ok(rd) = std::fs::read_dir(root.join("scenarios")) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            if let Ok(sc) = gm_core::Scenario::from_yaml(&text) {
                collect_scenario_ids(&sc, &mut sets);
            }
        }
    }

    sets.into_iter()
        .map(|(cat, items)| {
            let list = items
                .into_iter()
                .map(|(name, doc)| VocabItem { name, doc })
                .collect();
            (cat, list)
        })
        .collect()
}

/// 語彙一式を組む (command の実体)。
pub fn build_vocabulary(root: &Path) -> EditorVocabulary {
    let mut v = build_key_tables();
    v.ids = collect_ids(root);
    // アセット ID (spec 01) は scenario でなく**ディスクの実ファイル**が正
    // — engine は不透明 ID を運ぶだけで宣言を持たないので、宣言集合から導けない。
    // 実在しない名前を書いても engine は黙って None に落とす (resolve_asset は寛容) =
    // **死んだ参照 lint の射程外**。ゆえに補完で実名を出すのが唯一の予防になる。
    let media = crate::editor::list_media(root);
    for (cat, want) in [("images", "image"), ("audios", "audio")] {
        let names: Vec<VocabItem> = media
            .iter()
            .filter(|e| e.category == want)
            .filter_map(|e| e.rel_path.rsplit('/').next().map(VocabItem::bare))
            .collect();
        // 空なら**キーごと入れない** (「そのカテゴリが在るが空」と「無い」を区別しない —
        // 補完は候補ゼロなら開かないので同じ意味になる)。
        if !names.is_empty() {
            v.ids.insert(cat.to_string(), names);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(items: &[VocabItem]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }
    fn doc_of<'a>(items: &'a [VocabItem], name: &str) -> Option<&'a str> {
        items.iter().find(|i| i.name == name).and_then(|i| i.doc.as_deref())
    }

    /// キー語彙は型から導出されている (手書き表が無いことの表明)。
    /// frontend と同じ辿り方 (パス → 配線 → 文脈) をテストからも行う。
    /// 未知キー = 作者の付けた名前 (場所名など) は文脈を変えない。
    fn ctx_at(v: &EditorVocabulary, kind: &str, path: &[&str]) -> String {
        let mut ctx = v.roots[kind].clone();
        for seg in path {
            if *seg == "[]" {
                continue;
            }
            if let Some(e) = v.wiring.iter().find(|e| e.parent == ctx && &e.key == seg) {
                ctx = e.child.clone();
            }
        }
        ctx
    }

    /// 実在フィールドが載り、実在しないキーは載らない。
    #[test]
    fn key_tables_are_derived_from_types() {
        let v = build_key_tables();
        // ルート: scenario に image_style (spec 24 で足したフィールド) が自動で載る。
        let sc = names(&v.contexts[&v.roots["scenario"]]);
        assert!(sc.contains(&"image_style"), "{sc:?}");
        assert!(sc.contains(&"allowed_flags"));
        assert!(!sc.contains(&"entry"), "entry は manifest のキー");
        assert!(names(&v.contexts[&v.roots["manifest"]]).contains(&"facts_policy"));
        // 配線を辿った先の文脈: triggers → Trigger / effects → Op 和集合。
        let trigger = ctx_at(&v, "scenario", &["triggers", "[]"]);
        assert!(names(&v.contexts[&trigger]).contains(&"repeatable"));
        let op = ctx_at(&v, "scenario", &["triggers", "[]", "effects", "[]"]);
        assert!(names(&v.contexts[&op]).contains(&"op"));
        // バリアント表: move は to を持ち、entity を持たない (2026-07-27 の解像度)。
        let mv = names(&v.op_variant_keys["move"]);
        assert!(mv.contains(&"to") && !mv.contains(&"entity"), "{mv:?}");
        // タグ値: op に set_flag / kind に flag_is。
        assert!(names(&v.tag_values["op"]).contains(&"set_flag"));
        assert!(names(&v.tag_values["kind"]).contains(&"flag_is"));
        // 名前つき map 容器: locations の値 = Location。
        let loc = ctx_at(&v, "scenario", &["locations", "room"]);
        assert_eq!(loc, "Location");
        assert!(names(&v.contexts[&loc]).contains(&"description"));
    }

    /// 【spec 28 v2】**実語彙のフィクスチャ**を frontend のテストへ渡す。
    ///
    /// TS 側のテストは手で組んだ語彙で walk を測っており、**実データとの合成は
    /// どちらのテストも見ていなかった** (バリアント名の綴り・文脈名の一致は継ぎ目の話)。
    /// ここで実物を書き出し、古くなったらこのテストが落ちる (`UPDATE_VOCAB_FIXTURE=1`
    /// で更新)。id はパッケージ依存なので静的部分 (`build_key_tables`) だけ。
    #[test]
    fn vocabulary_fixture_is_fresh() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/editorVocabulary.fixture.json");
        let fresh = serde_json::to_string_pretty(&build_key_tables()).unwrap() + "
";
        if std::env::var("UPDATE_VOCAB_FIXTURE").is_ok() {
            std::fs::write(&path, &fresh).unwrap();
            return;
        }
        let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            on_disk.replace("\r\n", "\n").as_str(),
            fresh,
            "語彙のフィクスチャが古い。`UPDATE_VOCAB_FIXTURE=1 cargo test vocabulary_fixture` で更新し、frontend のテスト (npm test) も通ることを確かめること"
        );
    }

    /// 【spec 28 v2】ワイヤの形を留める — frontend の `EditorVocabulary` interface と
    /// 欄名がずれると、補完が黙って何も出さなくなる (型検査は継ぎ目を跨がない)。
    #[test]
    fn wire_shape_matches_the_frontend_interface() {
        let v = serde_json::to_value(build_key_tables()).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "contexts",
                "gate_variant_keys",
                "ids",
                "op_variant_keys",
                "roots",
                "tag_values",
                "wiring",
            ]
        );
        // 配線 1 本の形 (child_tagged は在るときだけ出る)。
        let w = v["wiring"].as_array().unwrap();
        let plain = w.iter().find(|e| e["kind"] == "seq").unwrap();
        let mut wk: Vec<&str> = plain.as_object().unwrap().keys().map(String::as_str).collect();
        wk.sort_unstable();
        assert_eq!(wk, ["child", "key", "kind", "parent"]);
        let tagged = w.iter().find(|e| e["kind"] == "item_map").unwrap();
        assert_eq!(tagged["child_tagged"], "Gate");
        // 候補 1 つの形 (doc は在るときだけ)。
        let item = &v["contexts"]["Scenario"][0];
        assert!(item["name"].is_string(), "{item:?}");
    }

    /// 【spec 28 v2】**配線の写しを持たない** — gm_core の表をそのまま運び、
    /// ここが足すのは harness 側の型の分だけ。文脈が同じキーでも親で割れることを固定する
    /// (`to:` は Exit なら場所、`give_item` ならエンティティ — v1 の平らな表では解けなかった)。
    #[test]
    fn wiring_comes_from_gm_core_and_only_harness_types_are_added_here() {
        let v = build_key_tables();
        let core = gm_core::wiring();
        for e in &core {
            assert!(v.wiring.contains(e), "gm_core の配線が欠けている: {e:?}");
        }
        assert_eq!(
            v.wiring.len(),
            core.len() + harness_wiring().len(),
            "gm_core の表 + harness 4 本ちょうど (手書きの写しを足さない)"
        );
        // 参照の閉包: 全ての文脈名が contexts に在る。
        for e in &v.wiring {
            assert!(v.contexts.contains_key(&e.parent), "未知の親文脈: {e:?}");
            assert!(v.contexts.contains_key(&e.child), "未知の子文脈: {e:?}");
        }
        for ctx in v.roots.values() {
            assert!(v.contexts.contains_key(ctx), "ルート文脈 {ctx} の語彙が無い");
        }
        // 同じキーが親で別の文脈へ割れる (v1 の平らな表が潰していた解像度)。
        assert_eq!(ctx_at(&v, "scenario", &["locations", "r", "exits", "[]", "gate"]), "Gate");
        assert_eq!(ctx_at(&v, "campaign", &["edges", "[]"]), "CampaignEdge");
        assert_eq!(ctx_at(&v, "manifest", &["player"]), "PlayerDef");
        assert_eq!(ctx_at(&v, "manifest", &["player", "stats", "腕力"]), "StatDecl");
        // character は gm_core の Character 文脈をそのまま使う (taboos → Gate まで辿れる)。
        assert_eq!(ctx_at(&v, "character", &["taboos", "[]"]), "Gate");
    }

    /// 説明は doc comment 由来で候補に載る (2026-08-28)。手書き表を持たないので、
    /// **コードの doc を直せば補完も直る**。doc の無い欄は説明なしのまま出る (捏造しない)。
    #[test]
    fn candidates_carry_docs_from_doc_comments() {
        let v = build_key_tables();
        let sc = &v.contexts[&v.roots["scenario"]];
        assert!(doc_of(sc, "world").unwrap().contains("世界観"));
        assert!(doc_of(sc, "global_flags").is_some());
        // op / kind の**値**にはバリアントの説明 (「この op が何をするか」)。
        let ops = &v.tag_values["op"];
        assert!(
            doc_of(ops, "give_item").unwrap().contains("譲渡"),
            "{:?}",
            doc_of(ops, "give_item")
        );
        assert!(doc_of(&v.tag_values["kind"], "has_item").is_some());
        // map 容器の子。
        assert!(doc_of(&v.contexts["Location"], "present").is_some());
        // doc の無い欄は None のまま。
        assert!(doc_of(sc, "title").is_none());
    }

    /// id 語彙: 同梱パッケージから locations / flags / entities / challenges が集まる。
    /// 表示名 (場所の title・キャラの name) が説明として載る。
    /// アセット (images/audios) は**ディスクの実ファイル名**が正 (宣言が無いので
    /// 宣言集合からは導けない = 死んだ参照 lint の射程外を補完で埋める)。
    /// 壊れたルートでは空 (キー補完は静的部分なので生きる)。
    #[test]
    fn ids_are_collected_from_the_package_and_absent_when_broken() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/dice_trial");
        let v = build_vocabulary(&root);
        assert!(!v.ids["images"].is_empty(), "アセット名が載る: {:?}", v.ids.get("images"));
        assert!(!v.ids["audios"].is_empty(), "{:?}", v.ids.get("audios"));
        // ファイル名そのもの (拡張子込み = アセット ID) で、パスではない。
        assert!(v.ids["images"].iter().all(|i| !i.name.contains('/')));

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/lakeside_manor");
        let v = build_vocabulary(&root);
        assert!(!v.ids["locations"].is_empty(), "{:?}", v.ids.keys().collect::<Vec<_>>());
        assert!(names(&v.ids["entities"]).contains(&"player"));
        assert!(!v.ids["challenges"].is_empty());
        assert!(!v.ids["flags"].is_empty());
        // 表示名が説明に載る (id だけでは何の場所か分からない)。
        assert!(v.ids["locations"].iter().any(|i| i.doc.is_some()), "{:?}", v.ids["locations"]);

        let broken = std::env::temp_dir().join("kataribe_vocab_missing");
        let v2 = build_vocabulary(&broken);
        assert!(v2.ids.is_empty());
        assert!(!v2.contexts["Scenario"].is_empty(), "キー補完は常に効く");
    }

    /// campaign パッケージでは全モジュールの和 + modules の id が集まる。
    #[test]
    fn campaign_ids_union_every_module() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/escape");
        let v = build_vocabulary(&root);
        assert!(v.ids["modules"].len() >= 2, "{:?}", v.ids.get("modules"));
        // 複数モジュールの locations が和集合で入る (開始モジュールだけではない)。
        assert!(v.ids["locations"].len() >= 2, "{:?}", v.ids.get("locations"));
    }

    /// **整合性エラーのある書きかけの盤面でも id は集まる** (2026-08-28 に塞いだ穴。旧実装は
    /// `load_package` の Ok だけを見ており、幻フラグ 1 つで語彙が全滅した = 補完が最も要る
    /// 状態でだけ消えていた)。cast に載せていないキャラ・package.yaml の注入分も出る。
    #[test]
    fn ids_survive_a_half_written_package() {
        let dir = std::env::temp_dir().join("kataribe_vocab_halfwritten");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("scenarios")).unwrap();
        std::fs::create_dir_all(dir.join("characters")).unwrap();
        std::fs::write(
            dir.join("package.yaml"),
            "title: t\nentry: scenarios/main.yaml\nglobals:\n  flags: [world_flag]\n",
        )
        .unwrap();
        // 幻フラグ (allowed_flags に無い) を trigger が書く = validate エラー。
        std::fs::write(
            dir.join("scenarios/main.yaml"),
            "title: t\nstart: toyoko\nallowed_flags: [met]\nlocations:\n  toyoko:\n    title: 東横\n\
             triggers:\n  - id: x\n    when: { kind: always }\n    effects: [{ op: set_flag, key: ghost, value: true }]\n",
        )
        .unwrap();
        std::fs::write(dir.join("characters/yua.yaml"), "name: ユア\n").unwrap();

        assert!(harness::load_package(&dir).is_err(), "前提: この盤面は validate で落ちる");
        let v = build_vocabulary(&dir);
        assert!(names(&v.ids["locations"]).contains(&"toyoko"), "{:?}", v.ids.get("locations"));
        assert_eq!(doc_of(&v.ids["locations"], "toyoko"), Some("東横"));
        // cast 宣言が無くても characters/ のファイルが entity 候補になる。
        assert!(names(&v.ids["entities"]).contains(&"yua"), "{:?}", v.ids.get("entities"));
        assert_eq!(doc_of(&v.ids["entities"], "yua"), Some("ユア"));
        // package.yaml の globals.flags も語彙に入る (注入は load 時なので entry には無い)。
        assert!(names(&v.ids["flags"]).contains(&"world_flag"), "{:?}", v.ids.get("flags"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
