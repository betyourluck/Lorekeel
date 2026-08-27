//! spec 28 Phase C: エディタ補完の語彙。
//!
//! **値は全部 型から導出** (`gm_core::struct_keys` / `gate_variant_keys` / `op_variant_keys`
//! = lint と同じ表。「補完に出るのに lint に叱られる」乖離を構造的に作らない)。
//! **配線 (どの親キーの下がどの型か) だけは手書きの表** — これは lint.rs `walk` が持つ
//! 構造知識の写しで、v1 のドリフトは許容する (ずれても補完が古いだけで lint は正しい。
//! 統合には walk のデータ駆動化が要る = v2)。
//!
//! id 語彙は編集ルートのパッケージ (ディスクの保存済み状態) から集める。読めなければ
//! **その分の補完が出ないだけ** (キー補完は型由来なので常に効く)。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

/// 補完語彙一式。frontend の completion source がこれだけを見る。
#[derive(Serialize, Clone, Debug, Default)]
pub struct EditorVocabulary {
    /// kind (manifest/scenario/character/campaign/memoria) → ルート直下のキー。
    pub root_keys: BTreeMap<String, Vec<String>>,
    /// 親キー → そのブロックのキー候補 (Vec 容器・単一 struct 欄・Gate/Op 欄)。
    pub key_contexts: BTreeMap<String, Vec<String>>,
    /// **名前つき map 容器** (locations 等) → 名前 1 段はさんだ子のキー候補。
    /// 親キーが未知 (= 作者が付けた名前) のとき、frontend は祖父キーでここを引く。
    pub map_child_keys: BTreeMap<String, Vec<String>>,
    /// `kind` の値 → その Gate バリアントのキー (lint と同じ表)。
    pub gate_variant_keys: BTreeMap<String, Vec<String>>,
    /// `op` の値 → その StateOp バリアントのキー。
    pub op_variant_keys: BTreeMap<String, Vec<String>>,
    /// タグ欄の値候補: "kind" → Gate バリアント名 / "op" → StateOp バリアント名。
    pub tag_values: BTreeMap<String, Vec<String>>,
    /// 宣言済み id: locations / flags / items / entities / challenges / contests /
    /// skills / goals / stats / modules。**死んだ参照 lint と同じ誤りの上流予防**。
    pub ids: BTreeMap<String, Vec<String>>,
}

fn vec_of(set: BTreeSet<String>) -> Vec<String> {
    set.into_iter().collect()
}

/// 型由来のキー語彙を組む (パッケージに依らない静的部分)。
fn build_key_tables() -> EditorVocabulary {
    use gm_core::{
        struct_keys, CharacterDef, ChallengeDef, ChallengeMod, ChallengeOutcome, GoalDef,
        Location, Protagonist, Scenario, StatDecl, TierDef, Trigger,
    };
    let mut v = EditorVocabulary::default();

    // ルート (kind 別)。最小 YAML は lint.rs Tables::build と同じものを使う。
    v.root_keys.insert(
        "manifest".into(),
        vec_of(struct_keys::<harness::PackageManifest>("entry: x")),
    );
    v.root_keys
        .insert("scenario".into(), vec_of(struct_keys::<Scenario>("start: room\nlocations: {}")));
    v.root_keys.insert("character".into(), vec_of(struct_keys::<CharacterDef>("{}")));
    v.root_keys.insert(
        "campaign".into(),
        vec_of(struct_keys::<harness::Campaign>("start: a\nmodules: { a: x.yaml }\nedges: []")),
    );
    v.root_keys
        .insert("memoria".into(), vec_of(struct_keys::<harness::MemoryFragment>("text: x")));

    // バリアント表 + タグ値 + 退避先の和集合。
    let gates = gm_core::gate_variant_keys();
    let ops = gm_core::op_variant_keys();
    let gate_union: BTreeSet<String> = gates.values().flatten().cloned().collect();
    let op_union: BTreeSet<String> = ops.values().flatten().cloned().collect();
    v.tag_values.insert("kind".into(), gates.keys().cloned().collect());
    v.tag_values.insert("op".into(), ops.keys().cloned().collect());
    v.gate_variant_keys = gates.into_iter().map(|(k, s)| (k, vec_of(s))).collect();
    v.op_variant_keys = ops.into_iter().map(|(k, s)| (k, vec_of(s))).collect();

    // 配線 (手書き — walk の Child 写像の写し)。値は型から。
    let gate = vec_of(gate_union);
    let op = vec_of(op_union);
    let outcome = vec_of(struct_keys::<ChallengeOutcome>("{}"));
    for k in ["when", "gate", "goal", "requires", "not", "until"] {
        v.key_contexts.insert(k.into(), gate.clone());
    }
    v.key_contexts.insert("of".into(), gate.clone()); // all/any の子列
    v.key_contexts.insert("effects".into(), op.clone());
    for k in [
        "on_success", "on_failure", "on_critical", "on_extreme", "on_hard", "on_fumble",
        "on_push_failure", "on_win", "on_lose", "on_tie",
    ] {
        v.key_contexts.insert(k.into(), outcome.clone());
    }
    v.key_contexts
        .insert("exits".into(), vec_of(struct_keys::<gm_core::Exit>("to: x")));
    v.key_contexts.insert(
        "triggers".into(),
        vec_of(struct_keys::<Trigger>("id: t\nwhen: { kind: always }")),
    );
    v.key_contexts.insert(
        "goals".into(),
        vec_of(struct_keys::<GoalDef>("id: g\nwhen: { kind: always }")),
    );
    v.key_contexts.insert(
        "modifiers".into(),
        vec_of(struct_keys::<ChallengeMod>("when: { kind: always }\nbonus: 0")),
    );
    v.key_contexts
        .insert("protagonist".into(), vec_of(struct_keys::<Protagonist>("{}")));
    v.key_contexts.insert(
        "role_assignment".into(),
        vec_of(struct_keys::<gm_core::RoleAssignment>("key: k\npool: {}\namong: []")),
    );
    v.key_contexts
        .insert("vote_rules".into(), vec_of(struct_keys::<gm_core::VoteRule>("{}")));
    v.key_contexts
        .insert("spend_rules".into(), vec_of(struct_keys::<gm_core::SpendRules>("from: x")));
    v.key_contexts.insert(
        "push_cost".into(),
        vec_of(struct_keys::<gm_core::PushCost>("from: x\namount: 1")),
    );
    let roll = vec_of(struct_keys::<gm_core::RollSpec>("{}"));
    v.key_contexts.insert("player_roll".into(), roll.clone());
    v.key_contexts.insert("opponent_roll".into(), roll);
    // manifest 側。
    v.key_contexts
        .insert("player".into(), vec_of(struct_keys::<harness::PlayerDef>("{}")));
    v.key_contexts
        .insert("globals".into(), vec_of(struct_keys::<harness::Globals>("{}")));
    // campaign 側 (edges の要素)。
    v.key_contexts
        .insert("edges".into(), vec!["from".into(), "on_goal".into(), "to".into()]);

    // 名前つき map 容器 (親キー = 作者の名前 → 祖父で引く)。
    v.map_child_keys
        .insert("locations".into(), vec_of(struct_keys::<Location>("{}")));
    v.map_child_keys
        .insert("characters".into(), vec_of(struct_keys::<CharacterDef>("{}")));
    v.map_child_keys.insert(
        "challenges".into(),
        vec_of(struct_keys::<ChallengeDef>("sides: 1\ndc: 1")),
    );
    v.map_child_keys
        .insert("tiers".into(), vec_of(struct_keys::<TierDef>("natural: min")));
    v.map_child_keys
        .insert("stats".into(), vec_of(struct_keys::<StatDecl>("initial: 0")));
    v.map_child_keys
        .insert("initial_stats".into(), vec_of(struct_keys::<StatDecl>("initial: 0")));
    // Location.items 新形式 {when, take} (旧形式 Gate は kind: で行内タグから引ける)。
    v.map_child_keys.insert("items".into(), vec!["take".into(), "when".into()]);
    v.map_child_keys.insert("flag_rules".into(), gate.clone());
    v.map_child_keys.insert("taboos".into(), gate);
    // contests は map (id → ContestDef)。
    v.map_child_keys.insert(
        "contests".into(),
        vec_of(struct_keys::<gm_core::ContestDef>(
            "opponent: x\nplayer_roll: {}\nopponent_roll: {}",
        )),
    );
    v
}

/// 1 枚の Scenario から id 語彙を吸い上げる (集合に足すだけ)。
fn collect_scenario_ids(sc: &gm_core::Scenario, ids: &mut BTreeMap<String, BTreeSet<String>>) {
    let mut add = |cat: &str, it: Box<dyn Iterator<Item = String> + '_>| {
        ids.entry(cat.to_string()).or_default().extend(it);
    };
    add("locations", Box::new(sc.locations.keys().cloned()));
    add("flags", Box::new(sc.allowed_flags.iter().cloned()));
    add("challenges", Box::new(sc.challenges.keys().cloned()));
    add("contests", Box::new(sc.contests.keys().cloned()));
    add("goals", Box::new(sc.goals.iter().map(|g| g.id.clone())));
    add("skills", Box::new(sc.initial_skills.iter().cloned()));
    add("stats", Box::new(sc.initial_stats.keys().cloned()));
    add("items", Box::new(sc.initial_inventory.iter().cloned()));
    add(
        "items",
        Box::new(sc.locations.values().flat_map(|l| l.items.keys().cloned())),
    );
    add("entities", Box::new(sc.characters.keys().cloned()));
    add("entities", Box::new(std::iter::once("player".to_string())));
    for c in sc.characters.values() {
        add("skills", Box::new(c.skills.iter().cloned()));
        add("stats", Box::new(c.stats.keys().cloned()));
        add("items", Box::new(c.inventory.iter().cloned()));
    }
}

/// パッケージから id 語彙を集める (単発 = entry 1 枚 / campaign = 全モジュールの和)。
/// 読めないもの (壊れた盤面・欠けたモジュール) はスキップ — 補完は減るだけで、
/// 壊れていること自体は診断 (層 1/層 2) が報告する (役割分離)。
fn collect_ids(root: &Path) -> BTreeMap<String, Vec<String>> {
    let mut sets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let Ok(manifest) = harness::read_manifest(root) else {
        return BTreeMap::new();
    };
    if harness::is_campaign_entry(&manifest.entry) {
        if let Ok(text) = std::fs::read_to_string(root.join(&manifest.entry)) {
            if let Ok(c) = harness::Campaign::from_yaml(&text) {
                sets.entry("modules".into())
                    .or_default()
                    .extend(c.modules.keys().cloned());
                for id in c.modules.keys() {
                    if let Ok(sc) =
                        harness::load_module_injected(&c, root, &manifest, id)
                    {
                        collect_scenario_ids(&sc, &mut sets);
                    }
                }
            }
        }
    } else if let Ok(loaded) = harness::load_package(root) {
        collect_scenario_ids(&loaded.scenario, &mut sets);
    }
    sets.into_iter().map(|(k, s)| (k, vec_of(s))).collect()
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
        let names: Vec<String> = media
            .iter()
            .filter(|e| e.category == want)
            .filter_map(|e| e.rel_path.rsplit('/').next().map(String::from))
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

    /// キー語彙は型から導出されている (手書き表が無いことの表明)。
    /// 実在フィールドが載り、実在しないキーは載らない。
    #[test]
    fn key_tables_are_derived_from_types() {
        let v = build_key_tables();
        // ルート: scenario に image_style (spec 24 で足したフィールド) が自動で載る。
        let sc = &v.root_keys["scenario"];
        assert!(sc.contains(&"image_style".to_string()), "{sc:?}");
        assert!(sc.contains(&"allowed_flags".to_string()));
        assert!(!sc.contains(&"entry".to_string()), "entry は manifest のキー");
        assert!(v.root_keys["manifest"].contains(&"facts_policy".to_string()));
        // 配線: triggers → Trigger のキー / effects → op 和集合。
        assert!(v.key_contexts["triggers"].contains(&"repeatable".to_string()));
        assert!(v.key_contexts["effects"].contains(&"op".to_string()));
        // バリアント表: move は to を持ち、entity を持たない (2026-07-27 の解像度)。
        let mv = &v.op_variant_keys["move"];
        assert!(mv.contains(&"to".to_string()) && !mv.contains(&"entity".to_string()), "{mv:?}");
        // タグ値: op に set_flag / kind に flag_is。
        assert!(v.tag_values["op"].contains(&"set_flag".to_string()));
        assert!(v.tag_values["kind"].contains(&"flag_is".to_string()));
        // map 容器: locations の孫 = Location のキー。
        assert!(v.map_child_keys["locations"].contains(&"description".to_string()));
    }

    /// id 語彙: 同梱パッケージから locations / flags / entities / challenges が集まる。
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
        assert!(v.ids["images"].iter().all(|s| !s.contains('/')), "{:?}", v.ids["images"]);

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/lakeside_manor");
        let v = build_vocabulary(&root);
        assert!(!v.ids["locations"].is_empty(), "{:?}", v.ids);
        assert!(v.ids["entities"].contains(&"player".to_string()));
        assert!(!v.ids["challenges"].is_empty());
        assert!(!v.ids["flags"].is_empty());

        let broken = std::env::temp_dir().join("kataribe_vocab_missing");
        let v2 = build_vocabulary(&broken);
        assert!(v2.ids.is_empty());
        assert!(!v2.root_keys["scenario"].is_empty(), "キー補完は常に効く");
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
}
