//! 未知フィールドの lint — 「静かな罠」(serde が未知キーを黙って無視する) の防衛線。
//!
//! 実測 3 件 (2026-07-11〜12): Location 直下の `gate:` (無効な場所に書いた) / challenge の
//! 入れ子ミス (インデントずれで別 challenge の内部フィールド化) / `entry:` typo (`entity:` の誤り)。
//! いずれも「エラーなく、ただ効かない」— serde の寛容さが失敗を隠す。
//!
//! 生 YAML を [`serde_yaml::Value`] として歩き、各文脈 (Scenario 直下 / Location / Trigger /
//! ChallengeDef / Gate / StateOp …) の**既知キー集合**と突き合わせ、未知キーを警告として名指しする
//! (近い既知キーがあれば「〜の誤り？」を添える)。**非 fatal** — 前方互換 (新しい content を古い
//! Kataribe で読む) を殺さないため、load は拒否せず提示層が ⚠ で出す ([`crate::Scenario::lints`] と同じ線引き)。
//!
//! **既知キー集合は手書きしない** — 構造体は「最小 YAML で parse → serialize → 全フィールド名」で
//! 実際の型から導出する (フィールド追加に自動追従 = 表のドリフトが構造的に起きない)。
//! enum (Gate/StateOp) は全バリアント標本の直列化の和集合 + **網羅 match の番人**
//! (バリアント追加時にコンパイルエラーで標本更新を強制する)。

use std::collections::{BTreeMap, BTreeSet};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_yaml::Value;

use crate::spine::{
    AttrRequirement, ChallengeDef, ChallengeMod, ChallengeOutcome, CharacterDef, Exit, Gate,
    GoalDef, Location, Protagonist, PushCost, RoleAssignment, Scenario, SpendRules, StatDecl,
    ContestDef, RollSpec, TierDef, Trigger, VoteRule,
};
use crate::state::StateOp;

/// scenario YAML の未知フィールドを警告文の列にする (健全なら空)。
/// parse できない YAML は空を返す (エラーは `Scenario::from_yaml` 側が出す — 役割分離)。
pub fn unknown_key_lints(src: &str) -> Vec<String> {
    let Ok(root) = serde_yaml::from_str::<Value>(src) else {
        return Vec::new();
    };
    let tables = Tables::build();
    let mut out = Vec::new();
    walk(&root, Ctx::Scenario, "", &tables, &mut out);
    out
}

/// 文脈 = 「いまどの型の mapping を見ているか」。
///
/// spec 28 v2: エディタ補完も同じ文脈で候補を決めるので、[`Ctx::name`] で**名前に射影**して
/// crate の外へ運べるようにしてある ([`wiring`] / [`context_keys`])。名前は enum の
/// バリアント名そのままで、増減は [`Ctx::ALL`] の番人がコンパイル時に強制する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Ctx {
    Scenario,
    Location,
    /// `Location.items` の値 (新形式 `{when, take}`。`kind` を含む mapping は旧形式 = Gate)。
    LocationItem,
    Exit,
    Trigger,
    Challenge,
    Outcome,
    Tier,
    ChallengeMod,
    Goal,
    Character,
    StatDecl,
    Gate,
    Op,
    Protagonist,
    RoleAssignment,
    VoteRule,
    AttrRequirement,
    SpendRules,
    PushCost,
    Contest,
    RollSpec,
}

impl Ctx {
    /// 全文脈。**バリアントを足したら [`_ctx_exhaustive_guard`] がコンパイルエラーになる**
    /// ので、ここへ追加すること (漏れると export した表からその文脈だけ消える)。
    const ALL: &'static [Ctx] = &[
        Ctx::Scenario,
        Ctx::Location,
        Ctx::LocationItem,
        Ctx::Exit,
        Ctx::Trigger,
        Ctx::Challenge,
        Ctx::Outcome,
        Ctx::Tier,
        Ctx::ChallengeMod,
        Ctx::Goal,
        Ctx::Character,
        Ctx::StatDecl,
        Ctx::Gate,
        Ctx::Op,
        Ctx::Protagonist,
        Ctx::RoleAssignment,
        Ctx::VoteRule,
        Ctx::AttrRequirement,
        Ctx::SpendRules,
        Ctx::PushCost,
        Ctx::Contest,
        Ctx::RollSpec,
    ];

    /// 文脈名 (バリアント名そのまま)。crate 外へ運ぶ唯一の形。
    const fn name(self) -> &'static str {
        match self {
            Ctx::Scenario => "Scenario",
            Ctx::Location => "Location",
            Ctx::LocationItem => "LocationItem",
            Ctx::Exit => "Exit",
            Ctx::Trigger => "Trigger",
            Ctx::Challenge => "Challenge",
            Ctx::Outcome => "Outcome",
            Ctx::Tier => "Tier",
            Ctx::ChallengeMod => "ChallengeMod",
            Ctx::Goal => "Goal",
            Ctx::Character => "Character",
            Ctx::StatDecl => "StatDecl",
            Ctx::Gate => "Gate",
            Ctx::Op => "Op",
            Ctx::Protagonist => "Protagonist",
            Ctx::RoleAssignment => "RoleAssignment",
            Ctx::VoteRule => "VoteRule",
            Ctx::AttrRequirement => "AttrRequirement",
            Ctx::SpendRules => "SpendRules",
            Ctx::PushCost => "PushCost",
            Ctx::Contest => "Contest",
            Ctx::RollSpec => "RollSpec",
        }
    }
}

/// [`Ctx::ALL`] の番人。バリアントを足すとここが非網羅でコンパイルエラーになる。
#[allow(dead_code)]
fn _ctx_exhaustive_guard(c: Ctx) {
    match c {
        Ctx::Scenario
        | Ctx::Location
        | Ctx::LocationItem
        | Ctx::Exit
        | Ctx::Trigger
        | Ctx::Challenge
        | Ctx::Outcome
        | Ctx::Tier
        | Ctx::ChallengeMod
        | Ctx::Goal
        | Ctx::Character
        | Ctx::StatDecl
        | Ctx::Gate
        | Ctx::Op
        | Ctx::Protagonist
        | Ctx::RoleAssignment
        | Ctx::VoteRule
        | Ctx::AttrRequirement
        | Ctx::SpendRules
        | Ctx::PushCost
        | Ctx::Contest
        | Ctx::RollSpec => {}
    }
}

/// シナリオ YAML のルート文脈 (エディタ補完が起点に使う)。
pub const CTX_SCENARIO: &str = Ctx::Scenario.name();
/// `characters/*.yaml` のルート文脈。
pub const CTX_CHARACTER: &str = Ctx::Character.name();
/// Gate の文脈名 (バリアント表 [`gate_variant_keys`] を引く合図)。
pub const CTX_GATE: &str = Ctx::Gate.name();
/// StateOp の文脈名 (バリアント表 [`op_variant_keys`] を引く合図)。
pub const CTX_OP: &str = Ctx::Op.name();

/// 各文脈の既知キー集合。実際の型から導出する ([`Tables::build`])。
///
/// spec 28 v2 で文脈ごとの名前つきフィールド 22 本を 1 本の表へ畳んだ。動機は
/// **型名を doc の引き key として外へ出す**必要が生じたことで、キー集合と型名を
/// 別々のリストで持つと必ずずれる (エディタ補完の説明が黙って消える形でずれる)。
struct Tables {
    /// 文脈 → 既知キー。Gate/StateOp はタグ不明時の**和集合** (`*_variants` が本線)。
    structs: BTreeMap<Ctx, BTreeSet<String>>,
    /// 文脈 → 型名 (doc 抽出の引き key)。型を持たない文脈 (`LocationItem`/`Gate`/`Op`) は無し。
    types: BTreeMap<Ctx, &'static str>,
    gate_variants: BTreeMap<String, BTreeSet<String>>,
    op_variants: BTreeMap<String, BTreeSet<String>>,
}

/// mapping 1 段の未知キーを警告文の列にする (近い既知キーの提案つき)。`path` は表示用の接頭辞
/// (空なら root)。mapping でなければ空。
///
/// scenario 以外の YAML — `package.yaml` 等、gm_core が知らない配布レイアウト側の型 — を
/// **その型を持つ層が自分で** lint するための部品 ([`struct_keys`] と対で使う)。
/// 再帰はしない: 入れ子は呼び出し側が文脈を知っているので、そちらが段ごとに呼ぶ。
pub fn unknown_keys(value: &Value, known: &BTreeSet<String>, path: &str) -> Vec<String> {
    let Value::Mapping(m) = value else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for k in m.keys() {
        let Some(key) = k.as_str() else { continue };
        if known.contains(key) {
            continue;
        }
        let here = if path.is_empty() { key.to_string() } else { format!("{path}.{key}") };
        out.push(format!(
            "{here}: 不明なフィールド「{key}」は無視されます{}",
            suggest(key, known)
        ));
    }
    out
}

/// 最小 YAML から型 `T` を作り、**シリアライズして全フィールド名**を得る
/// (serde は全フィールドを書き出すので、最小 sample でも既知キーは完全になる)。
pub fn struct_keys<T: DeserializeOwned + Serialize>(minimal_yaml: &str) -> BTreeSet<String> {
    let sample: T = serde_yaml::from_str(minimal_yaml)
        .expect("lint の最小サンプルは必ず parse できる (型のフィールド変更時はここを追従)");
    mapping_keys(&serde_yaml::to_value(&sample).expect("シリアライズは失敗しない"))
}

/// mapping からタグ (`kind`/`op`) の文字列値を取り出す (無ければ None = バリアント不明)。
fn tag_of<'a>(v: &'a Value, tag: &str) -> Option<&'a str> {
    match v {
        Value::Mapping(m) => m.get(Value::from(tag)).and_then(|t| t.as_str()),
        _ => None,
    }
}

fn mapping_keys(v: &Value) -> BTreeSet<String> {
    match v {
        Value::Mapping(m) => m
            .keys()
            .filter_map(|k| k.as_str().map(String::from))
            .collect(),
        _ => BTreeSet::new(),
    }
}

/// enum の全バリアント標本を直列化し、キーの**和集合** (タグ `kind`/`op` 込み) を得る。
/// タグが判別できないときの退避先 ([`variant_keys`] が本線)。
fn union_keys<T: Serialize>(samples: &[T]) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for s in samples {
        if let Ok(v) = serde_yaml::to_value(s) {
            set.extend(mapping_keys(&v));
        }
    }
    set
}

/// enum の全バリアント標本を直列化し、**タグの値 → そのバリアントのキー集合**の表を得る。
///
/// 和集合だけで持つと `{op: move, to: x, entity: aaa}` の `entity` が「他の op にとって
/// 正しいキー」ゆえ素通りする = **バリアントごとのフィールド typo が全部見えない**
/// (2026-07-27 発見)。タグ (`op`/`kind`) の値で引けばそのバリアントの語彙だけで検査できる。
fn variant_keys<T: Serialize>(samples: &[T], tag: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut table = BTreeMap::new();
    for s in samples {
        let Ok(v) = serde_yaml::to_value(s) else { continue };
        let keys = mapping_keys(&v);
        if let Some(name) = v.get(tag).and_then(|t| t.as_str()) {
            table.insert(name.to_string(), keys);
        }
    }
    table
}

/// Gate のバリアント表 (`kind` の値 → そのバリアントのキー集合)。spec 28 Phase C:
/// エディタ補完が **lint と同じ表** を見るための pub 面 (補完に出るのに lint に叱られる、
/// という乖離を構造的に作らない)。値は型から導出 (手書きしない規律は不変)。
pub fn gate_variant_keys() -> BTreeMap<String, BTreeSet<String>> {
    variant_keys(&gate_samples(), "kind")
}

/// StateOp のバリアント表 (`op` の値 → キー集合)。[`gate_variant_keys`] の op 版。
pub fn op_variant_keys() -> BTreeMap<String, BTreeSet<String>> {
    variant_keys(&op_samples(), "op")
}

/// Gate の全バリアント標本。**バリアントを追加したら [`_gate_exhaustive_guard`] がコンパイルエラーに
/// なるので、ここへ標本を足すこと** (足し忘れると新バリアントのフィールドが偽陽性警告になる)。
fn gate_samples() -> Vec<Gate> {
    let e = || "e".to_string();
    vec![
        Gate::Always,
        Gate::HasItem { entity: e(), item: "i".into() },
        Gate::FlagIs { key: "k".into(), value: true },
        Gate::LocationIs { at: "l".into() },
        Gate::StatAtLeast { entity: e(), key: "s".into(), value: 0 },
        Gate::StatAtMost { entity: e(), key: "s".into(), value: 0 },
        Gate::HasSkill { entity: e(), skill: "s".into() },
        Gate::AttributeIs { entity: e(), key: "a".into(), value: "v".into() },
        Gate::TurnsSince { entity: e(), key: "s".into(), turns: 1 },
        Gate::HasVoted { entity: e() },
        Gate::PresenceIs { entity: e(), present: true },
        Gate::All { of: Vec::new() },
        Gate::Any { of: Vec::new() },
        Gate::Not { of: Box::new(Gate::Always) },
    ]
}

/// 網羅 match の番人 — Gate にバリアントを足すとここが compile error になり、
/// [`gate_samples`] の更新を強制する (既知キー集合のドリフト防止)。
fn _gate_exhaustive_guard(g: &Gate) {
    match g {
        Gate::Always
        | Gate::HasItem { .. }
        | Gate::FlagIs { .. }
        | Gate::LocationIs { .. }
        | Gate::StatAtLeast { .. }
        | Gate::StatAtMost { .. }
        | Gate::HasSkill { .. }
        | Gate::AttributeIs { .. }
        | Gate::TurnsSince { .. }
        | Gate::HasVoted { .. }
        | Gate::PresenceIs { .. }
        | Gate::All { .. }
        | Gate::Any { .. }
        | Gate::Not { .. } => {}
    }
}

/// StateOp の全バリアント標本 (番人は [`_op_exhaustive_guard`])。
fn op_samples() -> Vec<StateOp> {
    let e = || "e".to_string();
    vec![
        StateOp::AddItem { item: "i".into() },
        StateOp::RemoveItem { item: "i".into() },
        StateOp::GiveItem { from: e(), to: e(), item: "i".into() },
        StateOp::SetFlag { key: "k".into(), value: true },
        StateOp::Move { to: "l".into() },
        StateOp::RequestRoll { sides: 6, dc: 3 },
        StateOp::Check { entity: e(), stat: "s".into(), sides: 20, dc: 10 },
        StateOp::CheckUnder { entity: e(), key: "s".into() },
        StateOp::AttemptChallenge { entity: e(), challenge: "c".into() },
        StateOp::AttemptContest { contest: "c".into() },
        StateOp::AdjustStat { entity: e(), key: "s".into(), delta: 1 },
        StateOp::ScaleStat { entity: e(), key: "s".into(), num: 1, den: 1 },
        StateOp::GrantSkill { entity: e(), skill: "s".into() },
        StateOp::SetAttribute { entity: e(), key: "a".into(), value: "v".into() },
        StateOp::RecordTurn { entity: e(), key: "s".into() },
        StateOp::SetPresence { entity: e(), present: true, volatile: false },
        StateOp::RollStat { entity: e(), key: "s".into(), count: 1, sides: 6, bonus: 0, negate: false },
        StateOp::CastVote { voter: e(), target: e() },
        StateOp::ResolveVote,
    ]
}

/// 網羅 match の番人 — StateOp にバリアントを足すとここが compile error になり、
/// [`op_samples`] の更新を強制する。
fn _op_exhaustive_guard(op: &StateOp) {
    match op {
        StateOp::AddItem { .. }
        | StateOp::RemoveItem { .. }
        | StateOp::GiveItem { .. }
        | StateOp::SetFlag { .. }
        | StateOp::Move { .. }
        | StateOp::RequestRoll { .. }
        | StateOp::Check { .. }
        | StateOp::CheckUnder { .. }
        | StateOp::AttemptChallenge { .. }
        | StateOp::AttemptContest { .. }
        | StateOp::AdjustStat { .. }
        | StateOp::ScaleStat { .. }
        | StateOp::GrantSkill { .. }
        | StateOp::SetAttribute { .. }
        | StateOp::RecordTurn { .. }
        | StateOp::SetPresence { .. }
        | StateOp::RollStat { .. }
        | StateOp::CastVote { .. }
        | StateOp::ResolveVote => {}
    }
}

impl Tables {
    fn build() -> Self {
        let mut structs: BTreeMap<Ctx, BTreeSet<String>> = BTreeMap::new();
        let mut types: BTreeMap<Ctx, &'static str> = BTreeMap::new();
        /// 文脈 1 つ = (既知キー, 型名) を**同じ 1 行から**導く。
        macro_rules! decl {
            ($ctx:expr, $ty:ty, $sample:expr) => {{
                structs.insert($ctx, struct_keys::<$ty>($sample));
                types.insert($ctx, short_type_name::<$ty>());
            }};
        }
        decl!(Ctx::Scenario, Scenario, "start: room
locations: {}");
        decl!(Ctx::Location, Location, "{}");
        decl!(Ctx::Exit, Exit, "to: x");
        decl!(Ctx::Trigger, Trigger, "id: t
when: { kind: always }");
        decl!(Ctx::Challenge, ChallengeDef, "sides: 1
dc: 1");
        decl!(Ctx::Outcome, ChallengeOutcome, "{}");
        decl!(Ctx::Tier, TierDef, "natural: min");
        decl!(Ctx::ChallengeMod, ChallengeMod, "when: { kind: always }
bonus: 0");
        decl!(Ctx::Goal, GoalDef, "id: g
when: { kind: always }");
        decl!(Ctx::Character, CharacterDef, "{}");
        decl!(Ctx::StatDecl, StatDecl, "initial: 0");
        decl!(Ctx::Protagonist, Protagonist, "{}");
        decl!(Ctx::RoleAssignment, RoleAssignment, "key: k
pool: {}
among: []");
        decl!(Ctx::VoteRule, VoteRule, "{}");
        decl!(Ctx::AttrRequirement, AttrRequirement, "key: k
value: v");
        decl!(Ctx::SpendRules, SpendRules, "from: x");
        decl!(Ctx::PushCost, PushCost, "from: x
amount: 1");
        decl!(
            Ctx::Contest,
            ContestDef,
            "opponent: o
player_roll: { sides: 6 }
opponent_roll: { sides: 6 }"
        );
        decl!(Ctx::RollSpec, RollSpec, "{}");

        // 型を持たない文脈。
        // LocationItem 新形式 {when, take} (旧形式 = Gate は kind の有無で判別)。
        structs.insert(Ctx::LocationItem, ["when", "take"].iter().map(|s| s.to_string()).collect());
        structs.insert(Ctx::Gate, union_keys(&gate_samples()));
        structs.insert(Ctx::Op, union_keys(&op_samples()));

        debug_assert_eq!(
            structs.len(),
            Ctx::ALL.len(),
            "全文脈に既知キー集合が要る (Ctx を足したら decl! も足すこと)"
        );
        Self {
            structs,
            types,
            gate_variants: variant_keys(&gate_samples(), "kind"),
            op_variants: variant_keys(&op_samples(), "op"),
        }
    }

    /// この mapping の既知キー集合。Gate/StateOp は**タグ (`kind`/`op`) の値でバリアントを
    /// 特定**し、そのバリアントの語彙だけで検査する (和集合だとバリアント別の typo が見えない)。
    /// タグが無い・未知の値なら和集合へ退避 = 判別できないものは警告しない。
    fn known(&self, ctx: Ctx, v: &Value) -> &BTreeSet<String> {
        match ctx {
            Ctx::Gate => tag_of(v, "kind")
                .and_then(|name| self.gate_variants.get(name))
                .unwrap_or_else(|| self.known_struct(Ctx::Gate)),
            Ctx::Op => tag_of(v, "op")
                .and_then(|name| self.op_variants.get(name))
                .unwrap_or_else(|| self.known_struct(Ctx::Op)),
            _ => self.known_struct(ctx),
        }
    }

    fn known_struct(&self, ctx: Ctx) -> &BTreeSet<String> {
        self.structs.get(&ctx).expect("Tables::build が全文脈を埋める (build の debug_assert と対)")
    }
}

/// 型名の最終セグメント (`gm_core::spine::Scenario` → `Scenario`) = doc 表の引き key。
fn short_type_name<T: ?Sized>() -> &'static str {
    std::any::type_name::<T>().rsplit("::").next().unwrap_or_default()
}

/// キー配下をどう歩くか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Child {
    /// この文脈の mapping として直接歩く。
    Direct(Ctx),
    /// 列の各要素を歩く。
    Seq(Ctx),
    /// mapping の**値**をそれぞれ歩く (キーは作者の自由語彙 = 検査しない)。
    MapValues(Ctx),
    /// `Location.items` の値: `kind` を含む mapping = 旧形式 Gate、それ以外 = 新形式 {when, take}。
    ItemMap,
    /// `CharacterDef.stats` の値: mapping = StatDecl、scalar (数値糖衣) = 検査なし。
    StatMap,
    /// 葉 (これ以上構造を知らない)。
    None,
}

/// **配線の唯一の表** — 「どの文脈のどのキーの下が、どの型か」。
///
/// spec 28 v2 でここを match からデータへ移した。理由は二重管理の解消で、エディタ補完は
/// 以前この表を**平らに潰した写し**を app 側に手書きで持っていた (平らなので `to:` が
/// 場所かエンティティかを文脈で解けず、提示層が場当たりに補っていた)。いまは
/// [`wiring`] が名前へ射影したこの表そのものを渡すので、写しは存在しない。
#[rustfmt::skip]
const WIRING: &[(Ctx, &[&str], Child)] = &[
    (Ctx::Scenario, &["locations"], Child::MapValues(Ctx::Location)),
    (Ctx::Scenario, &["triggers"], Child::Seq(Ctx::Trigger)),
    (Ctx::Scenario, &["challenges"], Child::MapValues(Ctx::Challenge)),
    (Ctx::Scenario, &["goals"], Child::Seq(Ctx::Goal)),
    (Ctx::Scenario, &["goal"], Child::Direct(Ctx::Gate)),
    (Ctx::Scenario, &["characters"], Child::MapValues(Ctx::Character)),
    (Ctx::Scenario, &["flag_rules"], Child::MapValues(Ctx::Gate)),
    (Ctx::Scenario, &["protagonist"], Child::Direct(Ctx::Protagonist)),
    (Ctx::Scenario, &["role_assignment"], Child::Direct(Ctx::RoleAssignment)),
    (Ctx::Scenario, &["spend_rules"], Child::Direct(Ctx::SpendRules)),
    (Ctx::Scenario, &["push_cost"], Child::Direct(Ctx::PushCost)),
    (Ctx::Scenario, &["contests"], Child::MapValues(Ctx::Contest)),
    (Ctx::Scenario, &["vote_rules"], Child::Seq(Ctx::VoteRule)),
    // initial_stats は素の数値と境界つき宣言 (StatInit) の両受け — mapping 値だけ
    // StatDecl として typo 検査する (Character.stats と同じ StatMap 意味論)。
    (Ctx::Scenario, &["initial_stats"], Child::StatMap),
    (Ctx::Contest, &["requires", "until"], Child::Direct(Ctx::Gate)),
    // player_roll/opponent_roll は文字列 (テンプレート名) or mapping (RollSpec) の両受け。
    // 文字列は walker が mapping でないため素通りし、mapping だけ RollSpec 検査になる。
    (Ctx::Contest, &["player_roll", "opponent_roll"], Child::Direct(Ctx::RollSpec)),
    (Ctx::Contest, &["on_win", "on_lose", "on_tie"], Child::Direct(Ctx::Outcome)),
    // キャラの振り方テンプレート: キーはテンプレート名 (データ)、値が RollSpec。
    (Ctx::Character, &["rolls"], Child::MapValues(Ctx::RollSpec)),
    (Ctx::Character, &["stats"], Child::StatMap),
    (Ctx::Character, &["taboos"], Child::Seq(Ctx::Gate)),
    (Ctx::Location, &["items"], Child::ItemMap),
    (Ctx::Location, &["exits"], Child::Seq(Ctx::Exit)),
    (Ctx::Exit, &["gate"], Child::Direct(Ctx::Gate)),
    (Ctx::Trigger, &["when"], Child::Direct(Ctx::Gate)),
    (Ctx::Trigger, &["effects"], Child::Seq(Ctx::Op)),
    (Ctx::Challenge, &["requires"], Child::Direct(Ctx::Gate)),
    (Ctx::Challenge, &["modifiers"], Child::Seq(Ctx::ChallengeMod)),
    // 全帰結スロット (spec 16 の degree 別 + spec 18 の on_push_failure)。
    // 従来 on_success/on_failure のみ = degree スロット内の typo が盲点だった。
    (Ctx::Challenge, &["on_success", "on_failure", "on_critical", "on_extreme", "on_hard",
                       "on_fumble", "on_push_failure"], Child::Direct(Ctx::Outcome)),
    (Ctx::Challenge, &["tiers"], Child::MapValues(Ctx::Tier)),
    (Ctx::Outcome, &["effects"], Child::Seq(Ctx::Op)),
    (Ctx::Tier, &["effects"], Child::Seq(Ctx::Op)),
    (Ctx::ChallengeMod, &["when"], Child::Direct(Ctx::Gate)),
    (Ctx::Goal, &["when"], Child::Direct(Ctx::Gate)),
    (Ctx::Gate, &["of"], Child::Seq(Ctx::Gate)),
    (Ctx::VoteRule, &["when"], Child::Direct(Ctx::Gate)),
    (Ctx::VoteRule, &["voter_attribute"], Child::Direct(Ctx::AttrRequirement)),
];

fn child_of(ctx: Ctx, key: &str) -> Child {
    for (c, keys, child) in WIRING {
        if *c == ctx && keys.contains(&key) {
            return *child;
        }
    }
    Child::None
}

/// 配線 1 本を crate の外へ運ぶ形 (文脈は名前)。
///
/// `kind` は `"direct"` (この文脈の mapping) / `"seq"` (列の各要素) /
/// `"map_values"` (mapping の値。キーは作者の自由語彙) / `"item_map"` / `"stat_map"`。
/// `child_tagged` は「その mapping が `kind:` を持つときは別の文脈」という一例外
/// (`Location.items` の旧形式 = Gate 直書き) を**データで**渡すためのもので、
/// 受け手が形式の分岐を手で知らずに済む。
#[derive(serde::Serialize, Clone, Debug, PartialEq, Eq)]
pub struct WiringEntry {
    pub parent: String,
    pub key: String,
    pub kind: String,
    pub child: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_tagged: Option<String>,
}

/// 配線表を名前へ射影して返す (spec 28 v2 — エディタ補完の文脈解決がこれを辿る)。
pub fn wiring() -> Vec<WiringEntry> {
    let mut out = Vec::new();
    for (parent, keys, child) in WIRING {
        let (kind, ctx, tagged) = match child {
            Child::Direct(c) => ("direct", *c, None),
            Child::Seq(c) => ("seq", *c, None),
            Child::MapValues(c) => ("map_values", *c, None),
            Child::ItemMap => ("item_map", Ctx::LocationItem, Some(Ctx::Gate.name().to_string())),
            Child::StatMap => ("stat_map", Ctx::StatDecl, None),
            Child::None => continue,
        };
        for key in *keys {
            out.push(WiringEntry {
                parent: parent.name().to_string(),
                key: (*key).to_string(),
                kind: kind.to_string(),
                child: ctx.name().to_string(),
                child_tagged: tagged.clone(),
            });
        }
    }
    out
}

/// 文脈 1 つの語彙。
#[derive(serde::Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ContextInfo {
    /// 既知キー。Gate/Op はタグ不明時の**和集合**で、バリアント別は
    /// [`gate_variant_keys`] / [`op_variant_keys`] が本線。
    pub keys: BTreeSet<String>,
    /// doc を引くための型名 (型を持たない文脈は `None`)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
}

/// 全文脈の語彙 (文脈名 → [`ContextInfo`])。[`wiring`] と対で使う。
pub fn context_keys() -> BTreeMap<String, ContextInfo> {
    let t = Tables::build();
    Ctx::ALL
        .iter()
        .map(|c| {
            let info = ContextInfo {
                keys: t.known_struct(*c).clone(),
                type_name: t.types.get(c).map(|s| (*s).to_string()),
            };
            (c.name().to_string(), info)
        })
        .collect()
}

fn walk(v: &Value, ctx: Ctx, path: &str, t: &Tables, out: &mut Vec<String>) {
    let Value::Mapping(m) = v else { return };
    let known = t.known(ctx, v);
    for (k, child) in m {
        let Some(key) = k.as_str() else { continue };
        if !known.contains(key) {
            let here = if path.is_empty() { key.to_string() } else { format!("{path}.{key}") };
            out.push(format!(
                "{here}: 不明なフィールド「{key}」は無視されます{}",
                suggest(key, known)
            ));
            continue; // 未知キーの下は文脈が分からないので潜らない
        }
        let sub_path = if path.is_empty() { key.to_string() } else { format!("{path}.{key}") };
        match child_of(ctx, key) {
            Child::Direct(c) => walk(child, c, &sub_path, t, out),
            Child::Seq(c) => {
                if let Value::Sequence(seq) = child {
                    for (i, item) in seq.iter().enumerate() {
                        walk(item, c, &format!("{sub_path}[{i}]"), t, out);
                    }
                }
            }
            Child::MapValues(c) => {
                if let Value::Mapping(map) = child {
                    for (mk, mv) in map {
                        let name = mk.as_str().unwrap_or("?");
                        walk(mv, c, &format!("{sub_path}.{name}"), t, out);
                    }
                }
            }
            Child::ItemMap => {
                if let Value::Mapping(map) = child {
                    for (mk, mv) in map {
                        let name = mk.as_str().unwrap_or("?");
                        let p = format!("{sub_path}.{name}");
                        // 旧形式 (Gate 直書き) は `kind` を含む。新形式は {when, take}。
                        let is_gate =
                            matches!(mv, Value::Mapping(im) if im.contains_key(Value::from("kind")));
                        if is_gate {
                            walk(mv, Ctx::Gate, &p, t, out);
                        } else {
                            walk(mv, Ctx::LocationItem, &p, t, out);
                            if let Value::Mapping(im) = mv {
                                if let Some(w) = im.get(Value::from("when")) {
                                    walk(w, Ctx::Gate, &format!("{p}.when"), t, out);
                                }
                            }
                        }
                    }
                }
            }
            Child::StatMap => {
                if let Value::Mapping(map) = child {
                    for (mk, mv) in map {
                        if mv.is_mapping() {
                            let name = mk.as_str().unwrap_or("?");
                            walk(mv, Ctx::StatDecl, &format!("{sub_path}.{name}"), t, out);
                        }
                    }
                }
            }
            Child::None => {}
        }
    }
}

/// 近い既知キーの提案 (「entry」→「entity」等)。編集距離 2 以下・3 文字以上のキーのみ。
fn suggest(key: &str, known: &BTreeSet<String>) -> String {
    if key.chars().count() < 3 {
        return String::new();
    }
    known
        .iter()
        .map(|k| (levenshtein(key, k), k))
        .filter(|(d, _)| *d <= 2)
        .min()
        .map(|(_, k)| format!("（「{k}」の誤り？）"))
        .unwrap_or_default()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

// =============================================================================
// PoC: 実測 3 事故 (entry typo / Location 直下 gate / challenge 入れ子) を名指しで捕まえ、
// 健全な総合盤面では偽陽性ゼロであること。
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 【spec 28 v2】配線を**データ表**にして名前へ射影した export が、
    /// (a) 自己完結している (全ての文脈名が [`context_keys`] に在る)
    /// (b) 全 [`Ctx`] を覆う ([`Ctx::ALL`] の番人と対)
    /// (c) 名前だけを辿って内部の walk と同じ文脈へ着く
    /// — (c) が本命で、エディタ補完はこの辿り方しか持たない。
    #[test]
    fn exported_wiring_is_self_contained_and_walkable_by_name() {
        let ctxs = context_keys();
        let w = wiring();

        // (a) 参照の閉包。
        for e in &w {
            assert!(ctxs.contains_key(&e.parent), "未知の親文脈: {e:?}");
            assert!(ctxs.contains_key(&e.child), "未知の子文脈: {e:?}");
            if let Some(t) = &e.child_tagged {
                assert!(ctxs.contains_key(t), "未知の tagged 文脈: {e:?}");
            }
        }
        // (b) 文脈の網羅。
        assert_eq!(ctxs.len(), Ctx::ALL.len());
        assert!(ctxs[CTX_SCENARIO].keys.contains("locations"));
        assert!(ctxs[CTX_CHARACTER].keys.contains("profile"));
        assert_eq!(ctxs[CTX_CHARACTER].type_name.as_deref(), Some("CharacterDef"));
        assert_eq!(ctxs["Challenge"].type_name.as_deref(), Some("ChallengeDef"));
        assert!(ctxs[CTX_GATE].type_name.is_none(), "Gate は型でなくバリアント表が本線");
        assert!(ctxs[CTX_GATE].keys.contains("kind") && ctxs[CTX_OP].keys.contains("op"));

        // (c) 名前だけで辿る (frontend と同じ形。`[]` = 列の要素)。
        let step = |ctx: &str, key: &str| -> Option<String> {
            w.iter().find(|e| e.parent == ctx && e.key == key).map(|e| e.child.clone())
        };
        let walk_path = |segs: &[&str]| -> Option<String> {
            let mut ctx = CTX_SCENARIO.to_string();
            for seg in segs {
                if *seg == "[]" {
                    continue; // 列/名前つき map の 1 段は文脈を変えない (子文脈は親キーが決める)
                }
                ctx = match step(&ctx, seg) {
                    Some(c) => c,
                    // 未知キー = 作者の付けた名前 (locations の場所名など)。文脈は据え置き。
                    None => ctx,
                };
            }
            Some(ctx)
        };
        assert_eq!(walk_path(&["triggers", "[]", "effects", "[]"]).as_deref(), Some(CTX_OP));
        assert_eq!(walk_path(&["triggers", "[]", "when"]).as_deref(), Some(CTX_GATE));
        assert_eq!(walk_path(&["triggers", "[]", "when", "of", "[]"]).as_deref(), Some(CTX_GATE));
        assert_eq!(walk_path(&["locations", "room", "exits", "[]"]).as_deref(), Some("Exit"));
        assert_eq!(walk_path(&["locations", "room", "exits", "[]", "gate"]).as_deref(), Some(CTX_GATE));
        assert_eq!(walk_path(&["challenges", "force", "on_success", "effects", "[]"]).as_deref(), Some(CTX_OP));
        assert_eq!(walk_path(&["challenges", "force", "tiers", "big"]).as_deref(), Some("Tier"));
        assert_eq!(walk_path(&["contests", "brawl", "on_win"]).as_deref(), Some("Outcome"));
        assert_eq!(walk_path(&["characters", "alice", "taboos", "[]"]).as_deref(), Some(CTX_GATE));

        // `Location.items` は形式で子文脈が割れる = その分岐をデータで渡している。
        let items = w.iter().find(|e| e.parent == "Location" && e.key == "items").unwrap();
        assert_eq!(items.kind, "item_map");
        assert_eq!(items.child, "LocationItem");
        assert_eq!(items.child_tagged.as_deref(), Some(CTX_GATE));
    }

    /// 【entry typo (実測 2026-07-12, 1ldk)】challenge の `entity:` を `entry:` と書くと serde が
    /// 黙って無視し主体固定が効かない。lint が名指しし「entity の誤り？」を提案する。
    #[test]
    fn lints_entry_typo_with_suggestion() {
        let yaml = "
start: room
challenges:
  hina_work:
    entry: hina
    stat: 主人公❤
    sides: 100
    dc: 100
locations:
  room: { description: d }
";
        let warns = unknown_key_lints(yaml);
        assert_eq!(warns.len(), 1, "{warns:?}");
        assert!(
            warns[0].contains("entry") && warns[0].contains("entity"),
            "typo の名指しと正しいキーの提案: {warns:?}"
        );
        assert!(warns[0].contains("hina_work"), "どの challenge かをパスで示す: {warns:?}");
    }

    /// 【Location 直下 gate (実測 2026-07-11, friday_lemmon)】Location に存在しない `gate:` を
    /// 書いても黙って無視される (出口の gate と混同しやすい)。lint が捕まえる。
    /// 【challenge 入れ子 (実測 2026-07-12, 1ldk)】インデントずれで challenge が別 challenge の
    /// 内部フィールドになると、その id が未知フィールドとして黙殺される。lint が捕まえる。
    #[test]
    fn lints_location_stray_gate_and_nested_challenge() {
        let yaml = "
start: room
challenges:
  sleep:
    sides: 6
    dc: 1
    hina_cafe_work:
      sides: 100
      dc: 10
locations:
  room:
    description: d
    gate: { kind: always }
";
        let warns = unknown_key_lints(yaml);
        assert!(
            warns.iter().any(|w| w.contains("locations.room") && w.contains("gate")),
            "Location 直下の gate を名指し: {warns:?}"
        );
        assert!(
            warns.iter().any(|w| w.contains("challenges.sleep") && w.contains("hina_cafe_work")),
            "入れ子になった challenge を名指し: {warns:?}"
        );
    }

    /// 【バリアント別の既知キー (2026-07-27, ユーザー質問「move に entity を書くと？」)】
    /// enum (Gate/StateOp) の既知キーを**全バリアントの和集合**で持つと、`{op: move, to: x,
    /// entity: aaa}` の `entity` は「他の op にとって正しいキー」ゆえ素通りする — つまり
    /// **op/gate ごとのフィールド typo が全部見えない**。タグ (`op`/`kind`) の値でそのバリアントの
    /// キー集合だけを引くことで塞ぐ。タグが無い/未知の値のときは和集合へ退避する (判別できない
    /// ものを誤って警告しない = lint は疑わしきは黙る)。
    #[test]
    fn lints_per_variant_field_typos_in_ops_and_gates() {
        let yaml = "
start: room
allowed_flags: [f]
triggers:
  - id: t
    when: { kind: location_is, at: room, entity: aaa }
    effects:
      - { op: move, to: room, entity: aaa }
      - { op: set_flag, key: f, value: true }
locations:
  room: { description: d }
";
        let warns = unknown_key_lints(yaml);
        assert!(
            warns.iter().any(|w| w.contains("effects[0]") && w.contains("entity")),
            "move に書いた entity を名指しする (他の op では正しいキーでも): {warns:?}"
        );
        assert!(
            warns.iter().any(|w| w.contains("when") && w.contains("entity")),
            "location_is に書いた entity も同様: {warns:?}"
        );
        assert_eq!(warns.len(), 2, "正しく書かれた set_flag には出ない = 偽陽性なし: {warns:?}");
    }

    /// 【判別できないときは黙る】タグ (`op`/`kind`) 自体が無い・未知の値のときは、どのバリアントか
    /// 決められないので和集合へ退避して警告しない。前方互換 (新しい content を古い Kataribe で
    /// 読む = 未知のバリアント名) を壊さないための退避でもある。
    #[test]
    fn unknown_or_missing_variant_tag_falls_back_to_the_union() {
        let yaml = "
start: room
triggers:
  - id: t
    when: { kind: 未知の条件, entity: e, key: k, value: true }
    effects:
      - { entity: e, key: s, delta: 1 }
locations:
  room: { description: d }
";
        assert!(unknown_key_lints(yaml).is_empty(), "{:?}", unknown_key_lints(yaml));
    }

    /// 【偽陽性ゼロ】主要な構造 (goal/goals/trigger/challenge/tier/modifiers/character/
    /// role_assignment/vote_rules/flag_rules/items 新旧形式/protagonist) を使った健全な盤面で
    /// 警告が出ない。型にフィールドを追加してもここは通り続ける (既知キーは型から導出)。
    #[test]
    fn no_false_positives_on_kitchen_sink() {
        let yaml = "
title: t
start: room
world: w
protagonist: { name: n, profile: p }
allowed_flags: [f, g]
global_flags: [f]
persistent_flags: [f]
flag_rules:
  f: { kind: flag_is, key: g, value: true }
flag_hints: { f: hint }
flag_titles: { f: 表示名 }
hidden_flags: [g]
initial_stats: { hp: 10 }
initial_skills: [剣術]
initial_inventory: [鍵]
initial_attributes: { クラス: 見習い }
hidden_stats: [タイマー]
cast: []
characters:
  alice:
    name: アリス
    profile: p
    stats: { 好感度: { initial: 0, min: 0, max: 100 }, 体力: 10 }
    skills: [料理]
    taboos: [{ kind: flag_is, key: f, value: true }]
    inventory: [花]
    attributes: { 職業: 店員 }
role_assignment: { key: 役職, pool: { 人狼: 1 }, among: [alice] }
vote_rules:
  - when: { kind: flag_is, key: f, value: true }
    voter_attribute: { key: 役職, value: 人狼 }
locations:
  room:
    title: 部屋
    description: d
    present: [alice]
    items:
      鍵: { kind: always }
      ジュース: { when: { kind: always }, take: infinite }
    exits:
      - { to: hall, gate: { kind: all, of: [{ kind: has_item, entity: player, item: 鍵 }] } }
  hall: { description: d }
triggers:
  - id: t1
    when: { kind: any, of: [{ kind: stat_at_least, entity: alice, key: 好感度, value: 30 }] }
    effects:
      - { op: set_flag, key: f, value: true }
      - { op: set_presence, entity: alice, present: false }
      - { op: record_turn, entity: player, key: タイマー }
    narration: n
    recall: cue
    repeatable: true
challenges:
  c1:
    description: d
    entity: alice
    stat: 好感度
    requires: { kind: turns_since, entity: player, key: タイマー, turns: 2 }
    modifiers:
      - { when: { kind: attribute_is, entity: alice, key: 職業, value: 店員 }, bonus: -5 }
    sides: 100
    dc: 50
    on_success: { flag: f, effects: [{ op: adjust_stat, entity: alice, key: 好感度, delta: 5 }], narration: n, sound: s.wav }
    on_failure: { effects: [{ op: scale_stat, entity: alice, key: 好感度, num: 1, den: 2 }] }
    tiers:
      crit_fail: { natural: at_most, threshold: 10, flag: g, narration: n }
goals:
  - { id: g1, when: { kind: stat_at_most, entity: alice, key: 好感度, value: 0 }, title: 表示, hint: h, narration: n, visible: false, epilogue_prompt: 余韻を }
goal: { kind: always }
";
        let warns = unknown_key_lints(yaml);
        assert!(warns.is_empty(), "健全な盤面に偽陽性を出さない: {warns:?}");
    }

    /// 【spec 11: 旧形式への epilogue_prompt】旧 `goal:` は素の Gate なので epilogue_prompt の
    /// 置き場が無い — serde は黙って無視する (deny_unknown_fields ではない) が、生 YAML 走査の
    /// この lint が未知キーとして名指しする、という防衛線の前提を回帰固定する。
    /// named goals (GoalDef) 側は既知キーが型から自動導出されるので警告しない
    /// (上の kitchen_sink に epilogue_prompt 入りの goal がある = 同時に担保)。
    #[test]
    fn lints_epilogue_prompt_on_old_form_goal_gate() {
        let yaml = "
start: room
goal: { kind: always, epilogue_prompt: 生存者のその後を語れ }
locations:
  room: { description: d }
";
        let warns = unknown_key_lints(yaml);
        assert_eq!(warns.len(), 1, "{warns:?}");
        assert!(
            warns[0].contains("epilogue_prompt"),
            "旧形式 goal (Gate) への epilogue_prompt を未知キーとして名指し: {warns:?}"
        );
    }

    /// 【壊れた YAML は沈黙】parse 不能なら空 (エラーは from_yaml 側の責務 — 二重報告しない)。
    #[test]
    fn broken_yaml_returns_empty() {
        assert!(unknown_key_lints(": : :").is_empty());
    }
}
