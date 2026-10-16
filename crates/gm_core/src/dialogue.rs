//! spec 35: 台詞と地の文の分離 — 話者の解決と照合。
//!
//! GM の語り (`StateDelta.narration`) を「地の文と話者つき台詞を順に並べた段」にする。
//! **本文は検証しない** (LLM の領分) が、**話者は照合する** — 台詞が登場人物の id つきで出れば、
//! その場にいない人物の台詞 (#47・#49) と、多人数で人が操作している人物の台詞の代筆をエンジンの事実で
//! 却下できる。単騎の主人公の台詞は通す (主人公の吹き出しで出し読み上げない = 2026-10-16 ユーザー決定)。
//!
//! 処理の順序 (data_contract `DialogueSegments.processing_order`):
//! parse (llm_client) → sanitize (llm_client) → [`resolve_narration`] → `adjudicate` →
//! [`apply_checked`]。解決には scenario が要るので、受け側の deserializer ではなくここで行う。

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::engine::{apply, ApplyOutcome, Verdict};
use crate::reason::RejectReason;
use crate::spine::{Protagonist, Scenario};
use crate::state::{EntityId, GameState, StateDelta, PLAYER};

/// 語りの 1 段。`speaker` が無ければ地の文、あれば台詞。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Segment {
    /// 台詞を話す人物。**この場にいる人物の id** を書く (名もない人物は `店員` のような短い
    /// 呼び名)。地の文の段では省く。主人公の台詞・心の声は台詞の段にしない。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    /// 本文。台詞の段なら台詞そのもの (「」で囲まなくてよい)。
    pub text: String,
}

/// `StateDelta.narration` の受け側の型。schema は段の配列だけを示し、**生の文字列は受け側の
/// 救済** (弱いモデル・旧来の出力) として受ける。
///
/// 手書きの Visitor で受ける — **untagged enum は使わない** (#93)。untagged は全形に失敗すると
/// 内側の理由 (段の `text` が無い等) を捨てて総称文言だけを返し、自己修復の燃料が死ぬ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Narration {
    Segments(Vec<Segment>),
    Raw(String),
}

impl Default for Narration {
    fn default() -> Self {
        Narration::Segments(Vec::new())
    }
}

impl From<&str> for Narration {
    fn from(s: &str) -> Self {
        Narration::Raw(s.to_string())
    }
}

impl From<String> for Narration {
    fn from(s: String) -> Self {
        Narration::Raw(s)
    }
}

impl From<Vec<Segment>> for Narration {
    fn from(v: Vec<Segment>) -> Self {
        Narration::Segments(v)
    }
}

impl Narration {
    /// 中身が空 (空白だけを含む) か。
    pub fn is_empty(&self) -> bool {
        match self {
            Narration::Raw(s) => s.trim().is_empty(),
            Narration::Segments(v) => v.iter().all(|s| s.text.trim().is_empty()),
        }
    }

    /// 本文に後処理を掛ける (漏れたマークアップの掃除 = llm_client の sanitize)。段ごとに掛け、
    /// 空になった段は落とす。
    pub fn map_text(&mut self, f: impl Fn(&str) -> String) {
        match self {
            Narration::Raw(s) => *s = f(s),
            Narration::Segments(v) => {
                for seg in v.iter_mut() {
                    seg.text = f(&seg.text);
                }
                v.retain(|s| !s.text.trim().is_empty());
            }
        }
    }

    /// scenario を持たない場所 (テスト・ログの断片) 向けの平らな文字列。話者は書かれたラベルの
    /// まま `ラベル「text」`。提示や還流には解決後の [`plain`] / [`plain_for_gm`] を使う。
    pub fn raw_text(&self) -> String {
        match self {
            Narration::Raw(s) => s.clone(),
            Narration::Segments(v) => v
                .iter()
                .map(|s| match s.speaker.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
                    Some(l) => format!("{l}「{}」", strip_quotes(&s.text)),
                    None => s.text.clone(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

impl Serialize for Narration {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self {
            Narration::Segments(v) => v.serialize(ser),
            Narration::Raw(s) => ser.serialize_str(s),
        }
    }
}

impl<'de> Deserialize<'de> for Narration {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Narration;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("地の文と台詞の段の配列 ([{speaker?, text}])")
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<Narration, E> {
                Ok(Narration::Raw(s.to_string()))
            }
            fn visit_string<E: de::Error>(self, s: String) -> Result<Narration, E> {
                Ok(Narration::Raw(s))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Narration, E> {
                Ok(Narration::default())
            }
            fn visit_none<E: de::Error>(self) -> Result<Narration, E> {
                Ok(Narration::default())
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Narration, A::Error> {
                // 要素の失敗はそのまま伝播させる (理由を捨てない = #93)。
                let mut v = Vec::new();
                while let Some(seg) = seq.next_element::<Segment>()? {
                    v.push(seg);
                }
                Ok(Narration::Segments(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Narration, A::Error> {
                // 段 1 つを配列で包まずに書く出力も受ける (#64 の one_or_many と同じ救済)。
                let seg = Segment::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(Narration::Segments(vec![seg]))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Narration {
    fn schema_name() -> String {
        "Narration".to_string()
    }
    fn is_referenceable() -> bool {
        false
    }
    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        // schema は段の配列だけを示す (文字列は受け側の救済であって案内しない)。
        <Vec<Segment>>::json_schema(gen)
    }
}

/// 解決後の話者。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Speaker {
    /// 登場人物か主人公 ([`PLAYER`])。照合の対象。
    Entity(EntityId),
    /// 名もない人物 (店員・衛兵など)。ラベルを表示し、照合しない。
    Extra(String),
}

/// 解決後の段。`speaker` が `None` なら地の文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSegment {
    pub speaker: Option<Speaker>,
    pub text: String,
}

/// 主人公を指す語 (手順 0)。ASCII は大文字小文字を区別しない。
const PROTAGONIST_WORDS: &[&str] = &[
    "player", "私", "俺", "僕", "わたし", "あたし", "主人公", "あなた", "プレイヤー", "i", "me",
    "you", "protagonist",
];

fn is_protagonist_word(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    PROTAGONIST_WORDS.contains(&lower.as_str())
}

/// 話者ラベルの解決 (data_contract `DialogueSegments.speaker_resolution` の手順 0〜4)。
/// `Err` は箱に入れる (`RejectReason` は Gate を抱えて大きい = clippy::result_large_err)。
fn resolve_label(scenario: &Scenario, label: &str) -> Result<Speaker, Box<RejectReason>> {
    // 手順 0: `player` と主人公を指す語。
    if is_protagonist_word(label) {
        return Ok(Speaker::Entity(PLAYER.to_string()));
    }
    // 手順 1: 登場人物の id に完全一致 (ASCII の大文字小文字は区別しない: `Alice` = `alice`)。
    if let Some(id) = scenario.characters.keys().find(|id| id.eq_ignore_ascii_case(label)) {
        return Ok(Speaker::Entity(id.clone()));
    }
    // 照合の相手 = (id, 表示名)。表示名が空なら名前では一致させない (空文字の誤爆を防ぐ)。
    let mut named: Vec<(&str, &str)> = scenario
        .characters
        .iter()
        .map(|(id, def)| (id.as_str(), def.name.trim()))
        .collect();
    named.push((PLAYER, scenario.protagonist.name.trim()));
    // 手順 2: 表示名に完全一致。主人公の名前もここで引く (同名の NPC を曖昧さとして検出する)。
    let exact: Vec<&str> = named
        .iter()
        .filter(|(_, name)| !name.is_empty() && *name == label)
        .map(|(id, _)| *id)
        .collect();
    match exact.len() {
        1 => return Ok(Speaker::Entity(exact[0].to_string())),
        n if n >= 2 => {
            return Err(Box::new(RejectReason::AmbiguousSpeaker {
                label: label.to_string(),
                candidates: exact.iter().map(|s| s.to_string()).collect(),
            }))
        }
        _ => {}
    }
    // 手順 2b: 主人公の名前の揺れ (プロフィール冒頭の呼び名と読み)。2026-10-16 実プレイ: 主人公の名前が
    // 「ゆうじ」でプロフィールが「日暮 悠二（ひぐらし ゆうじ）」の盤面で、GM が「悠二」と書くと
    // 手順 4 のモブになり地の文に畳まれていた。手順 3 より前に置く (「ひぐらしゆうじ」が「ゆうじ」を
    // 含むせいで書き直しに回らないように)。完全一致だけなので「ゆうじの母」は手順 3 のまま。
    let squeezed = squeeze(label);
    if protagonist_aliases(&scenario.protagonist).contains(&squeezed) {
        return Ok(Speaker::Entity(PLAYER.to_string()));
    }
    // 手順 3: 近い / 含む → 書き直させる (取り違えより再生成 1 回のほうが安い)。
    let mut candidates: Vec<String> = Vec::new();
    for (id, name) in &named {
        if (resembles(label, id) || resembles(label, name)) && !candidates.iter().any(|c| c == id) {
            candidates.push(id.to_string());
        }
    }
    if !candidates.is_empty() {
        return Err(Box::new(RejectReason::UnknownSpeaker { label: label.to_string(), candidates }));
    }
    // 手順 4: 名もない人物。
    Ok(Speaker::Extra(label.to_string()))
}

/// 空白を除いた形 (「日暮 悠二」と「日暮悠二」を同じに扱う)。
fn squeeze(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 主人公の呼び名の揺れ (手順 2b)。表示名と、プロフィールの最初の一文にある「括弧の前の名前」と
/// 「括弧の中の読み」の、それぞれ全体と空白で区切った部分 (空白は除いて比べる)。2〜12 字に限り、
/// 12 字を超える部分は区切りもしない — 名前の無い長い一文 (英語のプロフィール等) から語を拾わない。
/// 括弧の後ろ (「30才の男性」) は見ない。
fn protagonist_aliases(p: &Protagonist) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let t = squeeze(s);
        if (2..=12).contains(&t.chars().count()) && !out.contains(&t) {
            out.push(t);
        }
    };
    push(&p.name);
    let first = p.profile.trim().split(['。', '\n', '.', '、', ',']).next().unwrap_or("");
    let (name_part, reading) = match first.find(['（', '(']) {
        Some(i) => {
            let open = first[i..].chars().next().map_or(1, char::len_utf8);
            (&first[..i], first[i + open..].split(['）', ')']).next().unwrap_or(""))
        }
        None => (first, ""),
    };
    for part in [name_part, reading] {
        if squeeze(part).chars().count() > 12 {
            continue;
        }
        push(part);
        for tok in part.split_whitespace() {
            push(tok);
        }
    }
    out
}

/// `label` が既知の名前 `target` に近いか、含むか (手順 3)。`target` が空なら常に false。
fn resembles(label: &str, target: &str) -> bool {
    let tlen = target.chars().count();
    if tlen == 0 {
        return false;
    }
    let limit = if tlen <= 3 { 1 } else { 2 };
    if crate::lint::levenshtein(&label.to_lowercase(), &target.to_lowercase()) <= limit {
        return true;
    }
    tlen >= 2 && label.to_lowercase().contains(&target.to_lowercase())
}

/// 本文の外側を「」が囲んでいれば外す (台詞の段に「」を付けて書く出力の正規化)。
fn strip_quotes(text: &str) -> String {
    let t = text.trim();
    t.strip_prefix('「')
        .and_then(|r| r.strip_suffix('」'))
        .filter(|inner| !inner.contains('「') && !inner.contains('」'))
        .map(str::to_string)
        .unwrap_or_else(|| text.to_string())
}

/// 語りの全段の話者を解決する。生の文字列は行頭の `名前「…」` で、手順 0〜2 で主人公か登場人物に
/// 解決できた行だけを台詞の段にし、残りは地の文の段にする (名もない人物は台詞にしない —
/// 文字列では行頭の語が話者か語りの一部か判別できない)。
///
/// 却下理由は**全段ぶんまとめて**返す (1 回の自己修復で全部直させる)。
pub fn resolve_narration(
    scenario: &Scenario,
    narration: &Narration,
) -> Result<Vec<ResolvedSegment>, Vec<RejectReason>> {
    match narration {
        Narration::Raw(s) => Ok(split_raw(scenario, s)),
        Narration::Segments(segs) => {
            let mut out = Vec::new();
            let mut reasons = Vec::new();
            for seg in segs {
                let label = seg.speaker.as_deref().map(str::trim).unwrap_or("");
                if label.is_empty() {
                    out.push(ResolvedSegment { speaker: None, text: seg.text.clone() });
                    continue;
                }
                match resolve_label(scenario, label) {
                    Ok(sp) => out.push(ResolvedSegment { speaker: Some(sp), text: strip_quotes(&seg.text) }),
                    Err(r) => {
                        if !reasons.contains(&*r) {
                            reasons.push(*r);
                        }
                    }
                }
            }
            if reasons.is_empty() {
                Ok(out)
            } else {
                Err(reasons)
            }
        }
    }
}

fn split_raw(scenario: &Scenario, s: &str) -> Vec<ResolvedSegment> {
    let mut out: Vec<ResolvedSegment> = Vec::new();
    let mut prose: Vec<&str> = Vec::new();
    let flush = |prose: &mut Vec<&str>, out: &mut Vec<ResolvedSegment>| {
        if !prose.is_empty() {
            let text = prose.join("\n");
            if !text.trim().is_empty() {
                out.push(ResolvedSegment { speaker: None, text });
            }
            prose.clear();
        }
    };
    for line in s.lines() {
        if let Some(line_seg) = raw_line_as_dialogue(scenario, line) {
            flush(&mut prose, &mut out);
            out.push(line_seg);
        } else {
            prose.push(line);
        }
    }
    flush(&mut prose, &mut out);
    out
}

/// 行全体が `名前「…」` で、名前が手順 0〜2 で一意に解決できるときだけ台詞の段にする。
fn raw_line_as_dialogue(scenario: &Scenario, line: &str) -> Option<ResolvedSegment> {
    let t = line.trim();
    let open = t.find('「')?;
    let label = t[..open].trim();
    if label.is_empty() || !t.ends_with('」') {
        return None;
    }
    match resolve_label(scenario, label) {
        Ok(Speaker::Entity(id)) => Some(ResolvedSegment {
            speaker: Some(Speaker::Entity(id)),
            text: strip_quotes(&t[open..]),
        }),
        _ => None,
    }
}

/// 解決済みの段の話者を照合する (data_contract `DialogueSegments.apply_checked` の ②)。
/// `before` = ターン開始時の顔ぶれ、`after` = 適用後の顔ぶれ。`controlled` = 多人数で人が操作している
/// 人物 (多人数では主人公も入る)。単騎の主人公は常にその場にいて、台詞も通す (2026-10-16)。
pub fn check_speakers(
    before: &BTreeSet<EntityId>,
    after: &BTreeSet<EntityId>,
    segments: &[ResolvedSegment],
    controlled: &[EntityId],
) -> Vec<RejectReason> {
    let mut reasons: Vec<RejectReason> = Vec::new();
    let present: Vec<String> = before.union(after).cloned().collect();
    for seg in segments {
        let Some(Speaker::Entity(id)) = &seg.speaker else { continue };
        let r = if controlled.iter().any(|c| c == id) {
            RejectReason::SpeakerControlled { speaker: id.clone() }
        } else if id == PLAYER {
            continue;
        } else if !before.contains(id) && !after.contains(id) {
            RejectReason::SpeakerNotPresent { speaker: id.clone(), present: present.clone() }
        } else {
            continue;
        };
        if !reasons.contains(&r) {
            reasons.push(r);
        }
    }
    reasons
}

/// 写しへ適用 → 話者を照合 → 通れば確定する (data_contract `DialogueSegments.apply_checked`)。
///
/// 適用後の顔ぶれはトリガーの発火で変わるので、`adjudicate` (射影はトリガーを含まない) の中では
/// 判定できない。**`apply` 本体は変えない** — 決断の確定・対決のラウンドからも呼ばれ、どれも
/// LLM の語りを持たない。受理時は**写しの `ApplyOutcome` をそのまま返す** (再適用しない =
/// 出目・判定・発火ビートが試し適用と同一)。却下なら写しを捨てる (正本も RNG も無傷)。
pub fn apply_checked(
    state: &mut GameState,
    scenario: &Scenario,
    delta: &StateDelta,
    segments: &[ResolvedSegment],
    controlled: &[EntityId],
) -> Result<ApplyOutcome, Vec<RejectReason>> {
    let before = scenario.present_at(state);
    let mut trial = state.clone();
    let out = apply(&mut trial, scenario, delta).map_err(|v| match v {
        Verdict::Reject { reasons } => reasons,
        Verdict::Accept => Vec::new(),
    })?;
    let after = scenario.present_at(&trial);
    let reasons = check_speakers(&before, &after, segments, controlled);
    if !reasons.is_empty() {
        return Err(reasons);
    }
    *state = trial;
    Ok(out)
}

fn display_name(scenario: &Scenario, id: &str) -> String {
    let name = if id == PLAYER {
        scenario.protagonist.name.trim()
    } else {
        scenario.characters.get(id).map(|d| d.name.trim()).unwrap_or("")
    };
    if name.is_empty() {
        id.to_string()
    } else {
        name.to_string()
    }
}

/// 表示用の平らな文字列。台詞は `名前「text」` (id を含めない)。
pub fn plain(scenario: &Scenario, segments: &[ResolvedSegment]) -> String {
    flatten(scenario, segments, false)
}

/// GM へ還流する平らな文字列。台詞は `名前 (id):「text」` — GM が自分の過去の語りを**段の形で
/// 思い出す**ための書式 (旧来の「地の文に「」」を真似させない)。
pub fn plain_for_gm(scenario: &Scenario, segments: &[ResolvedSegment]) -> String {
    flatten(scenario, segments, true)
}

fn flatten(scenario: &Scenario, segments: &[ResolvedSegment], for_gm: bool) -> String {
    segments
        .iter()
        .map(|s| match &s.speaker {
            None => s.text.clone(),
            Some(Speaker::Entity(id)) if for_gm => {
                format!("{} ({id}):「{}」", display_name(scenario, id), s.text)
            }
            Some(Speaker::Entity(id)) => format!("{}「{}」", display_name(scenario, id), s.text),
            Some(Speaker::Extra(label)) if for_gm => format!("{label}:「{}」", s.text),
            Some(Speaker::Extra(label)) => format!("{label}「{}」", s.text),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateOp;

    fn sc() -> Scenario {
        let yaml = r#"
title: t
start: classroom
allowed_flags: []
goal: { kind: always }
protagonist: { name: 坂上 }
characters:
  rinka: { name: 凛花 }
  alice: { name: アリス }
  bob: { name: ボブ }
  yukina: { name: 雪菜 }
locations:
  classroom: { description: 教室, present: [rinka, yukina], exits: [{ to: hall }] }
  hall: { description: 廊下, exits: [] }
triggers:
  - id: bob_arrives
    when: { kind: location_is, at: hall }
    effects: [ { op: set_presence, entity: bob, present: true } ]
    narration: ボブが駆け寄ってきた。
"#;
        let s = Scenario::from_yaml(yaml).unwrap();
        assert_eq!(s.validate(), Vec::new(), "{:?}", s.validate());
        s
    }

    fn line(speaker: &str, text: &str) -> Segment {
        Segment { speaker: Some(speaker.into()), text: text.into() }
    }

    fn prose(text: &str) -> Segment {
        Segment { speaker: None, text: text.into() }
    }

    fn resolve_one(label: &str) -> Result<Speaker, Box<RejectReason>> {
        let n = Narration::Segments(vec![line(label, "…")]);
        match resolve_narration(&sc(), &n) {
            Ok(v) => Ok(v[0].speaker.clone().unwrap()),
            Err(mut r) => Err(Box::new(r.remove(0))),
        }
    }

    /// 【受け側】配列でも文字列でも単一オブジェクトでも null でも受ける。段の欄の誤りは
    /// 総称文言に潰れず、どの欄が足りないかが残る (#93)。
    #[test]
    fn narration_accepts_array_string_single_object_and_keeps_field_errors() {
        let arr: Narration =
            serde_yaml::from_str(r#"[{"text":"霧"},{"speaker":"rinka","text":"遅い"}]"#).unwrap();
        assert_eq!(arr, Narration::Segments(vec![prose("霧"), line("rinka", "遅い")]));
        let raw: Narration = serde_yaml::from_str(r#""扉が軋む""#).unwrap();
        assert_eq!(raw, Narration::Raw("扉が軋む".into()));
        let one: Narration = serde_yaml::from_str(r#"{"speaker":"rinka","text":"遅い"}"#).unwrap();
        assert_eq!(one, Narration::Segments(vec![line("rinka", "遅い")]));
        let null: Narration = serde_yaml::from_str("null").unwrap();
        assert!(null.is_empty());
        let err = serde_yaml::from_str::<Narration>(r#"[{"speaker":"rinka"}]"#).unwrap_err();
        assert!(err.to_string().contains("text"), "欄の誤りが残る: {err}");
        // 欄を省いた delta (従来どおり narration なし) も読める。
        let d: StateDelta = serde_yaml::from_str(r#"{"ops":[]}"#).unwrap();
        assert!(d.narration.is_empty());
    }

    /// 【文字列の救済】行頭の `名前「…」` は、名前が登場人物に解決できるときだけ台詞にする。
    /// `そして「…」` や名もない人物は地の文のまま (吹き出しを捏造しない)。
    #[test]
    fn raw_string_splits_only_lines_with_known_speakers() {
        let raw = Narration::Raw("教室が静まる。\n凛花「遅いわね」\nそして「行こう」と誰かが言った。\n店員「いらっしゃい」".into());
        let v = resolve_narration(&sc(), &raw).unwrap();
        assert_eq!(v.len(), 3, "{v:?}");
        assert_eq!(v[0], ResolvedSegment { speaker: None, text: "教室が静まる。".into() });
        assert_eq!(v[1], ResolvedSegment { speaker: Some(Speaker::Entity("rinka".into())), text: "遅いわね".into() });
        assert_eq!(v[2].speaker, None, "そして / 店員 は地の文のまま");
        assert!(v[2].text.contains("店員「いらっしゃい」"));
    }

    /// 【手順 0 / 2】`player`・主人公の表示名・主人公を指す語はすべて主人公に解決される。
    /// **単騎では主人公の台詞を通す** (主人公の吹き出しで出し、読み上げない = 2026-10-16 ユーザー決定。
    /// 主人公は常にその場にいる)。多人数で人が操作しているとき (`controlled` に入る) は従来どおり却下。
    #[test]
    fn protagonist_lines_pass_in_solo_and_are_rejected_when_controlled() {
        for label in ["player", "Player", "坂上", "私", "主人公"] {
            assert_eq!(resolve_one(label), Ok(Speaker::Entity(PLAYER.into())), "{label}");
        }
        let segs = vec![ResolvedSegment { speaker: Some(Speaker::Entity(PLAYER.into())), text: "…".into() }];
        let none = BTreeSet::new();
        assert_eq!(check_speakers(&none, &none, &segs, &[]), Vec::new(), "単騎: 主人公の台詞は通る (不在扱いもしない)");
        assert_eq!(
            check_speakers(&none, &none, &segs, &[PLAYER.into()]),
            vec![RejectReason::SpeakerControlled { speaker: PLAYER.into() }],
            "多人数: 人が操作する主人公の台詞は却下"
        );
    }

    /// 【手順 2b】名前の揺れ: 主人公のプロフィール冒頭の呼び名 (括弧の前の名前と括弧の中の読み、
    /// それぞれ空白で区切った部分も) は主人公に解決する。2026-10-16 実プレイ: 主人公の名前が「ゆうじ」で
    /// プロフィールが「日暮 悠二」の盤面で、GM が「悠二」と書くとモブとして地の文に畳まれていた。
    /// 冒頭の名前の外の語 (「男性」「田中」) は拾わず、主人公の名前を含む別人 (「ゆうじの母」) は従来どおり書き直させる。
    #[test]
    fn protagonist_profile_names_resolve_to_player() {
        let mut s = sc();
        s.protagonist.name = "ゆうじ".into();
        s.protagonist.profile = "日暮 悠二（ひぐらし ゆうじ）。28歳、無職の男性。元上司の田中を避けている。".into();
        for label in ["悠二", "日暮悠二", "日暮 悠二", "日暮", "ひぐらし", "ひぐらしゆうじ", "ゆうじ"] {
            assert_eq!(resolve_label(&s, label), Ok(Speaker::Entity(PLAYER.into())), "{label}");
        }
        for label in ["男性", "田中", "店員"] {
            assert_eq!(resolve_label(&s, label), Ok(Speaker::Extra(label.into())), "{label}");
        }
        assert!(matches!(resolve_label(&s, "ゆうじの母").map_err(|b| *b), Err(RejectReason::UnknownSpeaker { .. })));
        // 文字列の救済も同じ解決を通る。
        let v = resolve_narration(&s, &Narration::Raw("悠二「行くぞ」".into())).unwrap();
        assert_eq!(v[0].speaker, Some(Speaker::Entity(PLAYER.into())));

        // 名前が空白無しで、括弧の中だけ読みがある形。
        s.protagonist.name = "春樹".into();
        s.protagonist.profile = "戸渡春樹（とたり はるき）30才の男性。休日は推し活をしている。".into();
        for label in ["戸渡春樹", "はるき", "とたり", "とたりはるき"] {
            assert_eq!(resolve_label(&s, label), Ok(Speaker::Entity(PLAYER.into())), "{label}");
        }
        assert_eq!(resolve_label(&s, "男性"), Ok(Speaker::Extra("男性".into())));

        // 名前の無い長い一文 (英語) からは語を拾わない。
        s.protagonist.name = "Me".into();
        s.protagonist.profile = "A young man who embarked on a journey to escape his tedious daily life.".into();
        assert_eq!(resolve_label(&s, "man"), Ok(Speaker::Extra("man".into())));
    }

    /// 【手順 0 / 2】主人公の表示名が空でも、空ラベルや部分一致で主人公に誤爆しない。
    #[test]
    fn empty_protagonist_name_never_matches() {
        let mut s = sc();
        s.protagonist.name = String::new();
        let n = Narration::Segments(vec![line("店員", "いらっしゃい"), line("  ", "地の文")]);
        let v = resolve_narration(&s, &n).unwrap();
        assert_eq!(v[0].speaker, Some(Speaker::Extra("店員".into())));
        assert_eq!(v[1].speaker, None, "空白だけのラベルは地の文");
    }

    /// 【手順 2】表示名が 2 人に一致すれば曖昧 (NPC と主人公が同名のときも)。
    #[test]
    fn duplicate_display_names_are_ambiguous() {
        let mut s = sc();
        s.characters.get_mut("bob").unwrap().name = "アリス".into();
        let n = Narration::Segments(vec![line("アリス", "…")]);
        let r = resolve_narration(&s, &n).unwrap_err();
        assert!(matches!(&r[0], RejectReason::AmbiguousSpeaker { candidates, .. }
            if candidates.contains(&"alice".to_string()) && candidates.contains(&"bob".to_string())));
        let mut s = sc();
        s.characters.get_mut("bob").unwrap().name = "坂上".into();
        let r = resolve_narration(&s, &Narration::Segments(vec![line("坂上", "…")])).unwrap_err();
        assert!(matches!(&r[0], RejectReason::AmbiguousSpeaker { candidates, .. }
            if candidates.contains(&PLAYER.to_string())), "同名の NPC を黙って主人公扱いしない");
    }

    /// 【手順 3 / 4】誤字・敬称つきは候補を添えて書き直させ、名もない人物は通す。
    #[test]
    fn near_misses_are_rejected_with_candidates_and_extras_pass() {
        assert!(matches!(resolve_one("aliec").map_err(|b| *b), Err(RejectReason::UnknownSpeaker { candidates, .. })
            if candidates == vec!["alice".to_string()]));
        assert!(matches!(resolve_one("凛花さん").map_err(|b| *b), Err(RejectReason::UnknownSpeaker { candidates, .. })
            if candidates == vec!["rinka".to_string()]));
        assert_eq!(resolve_one("店員"), Ok(Speaker::Extra("店員".into())));
        assert_eq!(resolve_one("凛花"), Ok(Speaker::Entity("rinka".into())), "表示名 → id");
        assert_eq!(resolve_one("rinka"), Ok(Speaker::Entity("rinka".into())));
        assert_eq!(resolve_one("Rinka"), Ok(Speaker::Entity("rinka".into())), "id の大文字小文字");
        assert!(matches!(resolve_one("playr").map_err(|b| *b), Err(RejectReason::UnknownSpeaker { candidates, .. })
            if candidates.contains(&PLAYER.to_string())), "player の誤字も主人公の候補で書き直させる");
    }

    /// 【照合】不在の話者は今いる人物を添えて却下、操作中の仲間も却下、居る人物は通る。
    #[test]
    fn absent_and_controlled_speakers_are_rejected() {
        let s = sc();
        let mut st = s.initial_state(1);
        let segs = resolve_narration(
            &s,
            &Narration::Segments(vec![line("rinka", "遅い"), line("alice", "やあ"), line("yukina", "…")]),
        )
        .unwrap();
        let before = st.clone();
        let r = apply_checked(&mut st, &s, &StateDelta::new("", vec![]), &segs, &["yukina".into()]).unwrap_err();
        assert!(r.contains(&RejectReason::SpeakerNotPresent {
            speaker: "alice".into(),
            present: vec!["rinka".into(), "yukina".into()],
        }), "{r:?}");
        assert!(r.contains(&RejectReason::SpeakerControlled { speaker: "yukina".into() }), "{r:?}");
        assert_eq!(r.len(), 2, "居る rinka は通る: {r:?}");
        assert_eq!(st, before, "却下で正本も RNG も無傷");
    }

    /// 【照合】到着時のトリガーで現れた人物の台詞は通る (開始時 ∪ 適用後 = spec 32 の教訓)。
    /// 受理時の出目・発火ビートは試し適用と同一で、再適用しない。
    #[test]
    fn arrival_trigger_speaker_passes_and_outcome_is_the_trial_one() {
        let s = sc();
        let st0 = s.initial_state(7);
        let delta = StateDelta::new("", vec![
            StateOp::RequestRoll { sides: 20, dc: 10 },
            StateOp::Move { to: "hall".into() },
        ]);
        let segs = resolve_narration(&s, &Narration::Segments(vec![line("bob", "おかえり")])).unwrap();
        let mut st = st0.clone();
        let out = apply_checked(&mut st, &s, &delta, &segs, &[]).expect("到着時に現れた bob は通る");
        // 同じ seed の写しへ素の apply をした結果と一致 = 試し適用の結果をそのまま返している。
        let mut reference = st0.clone();
        let expect = apply(&mut reference, &s, &delta).unwrap();
        assert_eq!(out, expect);
        assert_eq!(st, reference, "確定した正本も素の apply と同一 (RNG を二重に進めない)");
        assert_eq!(out.fired.len(), 1);
    }

    /// 【平らにする】`plain` は id を含まず、`plain_for_gm` は `名前 (id):「…」`。
    /// 台詞の本文に付いた外側の「」は外す。
    #[test]
    fn plain_hides_ids_and_plain_for_gm_shows_them() {
        let s = sc();
        let segs = resolve_narration(
            &s,
            &Narration::Segments(vec![prose("霧が出た。"), line("rinka", "「遅いわね」"), line("店員", "毎度")]),
        )
        .unwrap();
        assert_eq!(plain(&s, &segs), "霧が出た。\n凛花「遅いわね」\n店員「毎度」");
        assert_eq!(plain_for_gm(&s, &segs), "霧が出た。\n凛花 (rinka):「遅いわね」\n店員:「毎度」");
    }
}
