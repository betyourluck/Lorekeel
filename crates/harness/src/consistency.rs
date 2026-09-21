//! 語りの事後検査 (spec 32)。
//!
//! `adjudicate` は ops を全件裁くが `narration` は engine が原理的に検証できない (#23)。
//! ここはその**第三の層** — 規律 (prompt) でも還流 (事実の差し戻し) でもなく、
//! **書かれた後に照合する**。
//!
//! # 設計の核
//! - **Jev に「良い語りか」を聞かない。** 聞くのは state との照合だけ。美的判断には
//!   ground truth が無く、それは TypeSafe 自身がベンチマークで踏んだ穴の再演になる。
//! - **質問文は GM_SYSTEM と同じ接地の文言**。一文の増減で確度が 0.36 動くことを実測した
//!   (live で「話題に出るだけ…」を落としたら 0.87 → 0.51) ので、**ここの文言はテストで固定し、
//!   変更したら実データで再測定する**。
//! - **presence と location はターン開始時 (出発地) のもの**を使う。移動後で判定すると
//!   移動ターンの語り (出発地で始まる) が 0.94 で誤検出される。
//! - **移動検査は一方向** — op なし × 語りが移動、だけ。逆向き (op は移動済みで語りが
//!   「扉に手をかけた」で止まる) は語りの作法であって違反ではない。
//!
//! Phase A は**観測だけ**。却下も還流もせず、engine・`run_turn`・prompt を一切触らない。

use std::collections::BTreeMap;

use gm_core::{GameState, Scenario, PLAYER};
use llm_client::jev::{JevClient, NoulQuestion};

use crate::HarnessError;

/// `past_narrations` に載せる最大文字数。超えたら**古い方から**落とす。
///
/// Jev の予算は state + 最長質問で約 32k トークン。profile と語りを合わせても余裕を持たせて
/// この値にしてある (実測: 9 ターンで約 2,000 字 = 50 ターンでも約 11,000 字)。
pub const MAX_PAST_CHARS: usize = 16_000;

/// 陽性とみなす既定の閾値。Phase A は観測なので「印」であって却下条件ではない。
pub const DEFAULT_THRESHOLD: f64 = 0.5;

/// 検査の軸。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Axis {
    /// この場にいない人物が台詞を話した (#47 / #49)。
    AbsentSpeaker,
    /// 主人公が所持していない物を手渡した (#23)。
    HandedUnowned,
    /// move op が無いのに語りが移動を完了した (#42)。
    MovedWithoutOp,
    /// 過去に語られた事実と食い違う。
    PastContradiction,
    /// profile に書かれた人物像から外れた振る舞いをしている。
    ///
    /// **2026-09-21 に「別人の特徴の混入」から差し替えた。** 旧問い (「別の人物の特徴として
    /// 書かれているものを見せているか") は実プレイ 43 ターンで中央値 0.30・真相開示のターンで
    /// 0.5 超えを 5 回出し、**分離しなかった** (鳴らすべきでない最大 0.71 / 鳴らすべき最小 0.32
    /// = 重なる)。ミステリ盤面では**特徴の共有が真相そのもの**で、「同じ指輪」「同じ癖」は
    /// 作者が置いた手がかりだから — 問いの立て方が盤面と噛み合っていなかった。
    /// 論点を 1 つに削った現在の問いは 鳴らすべきでない 0.38 / 鳴らすべき 0.56 で分離する。
    ProfileDeviation,
    /// profile に書かれた設定と食い違う。
    ProfileContradiction,
    /// 秘匿とされた関係を地の文が明かした (#33)。
    SecretLeak,
}

impl Axis {
    /// **還流に乗せてよいか。** `SecretLeak` は false — 漏洩は明示だけでなく示唆でも起きるので
    /// 二値で切れず (実測ギャップ 0.41)、誤ラベルを積むと GM が萎縮して匂わせすらしなくなる。
    /// ミステリ盤面では致命的なので、表示だけに留める。
    pub fn is_advisory(self) -> bool {
        matches!(self, Axis::SecretLeak)
    }

    /// 人が読む名前 (dev 表示 / jsonl 用)。
    pub fn label(self) -> &'static str {
        match self {
            Axis::AbsentSpeaker => "不在者の発話",
            Axis::HandedUnowned => "未所持物の譲渡",
            Axis::MovedWithoutOp => "語りだけの移動",
            Axis::PastContradiction => "過去の語りとの矛盾",
            Axis::ProfileDeviation => "人物像からの逸脱",
            Axis::ProfileContradiction => "設定との食い違い",
            Axis::SecretLeak => "秘匿の漏洩",
        }
    }
}

/// **ターン開始時**の盤面スナップショット。
///
/// `narration` と `moved_by_op` は適用後にしか決まらないので [`ConsistencyQuery`] 側で受ける。
#[derive(Debug, Clone, Default)]
pub struct ConsistencySnapshot {
    pub location: String,
    /// 実効 presence (主人公を含む)。**ターン開始時**。
    pub present: Vec<String>,
    pub player_inventory: Vec<String>,
    pub npc_inventory: BTreeMap<String, Vec<String>>,
    /// entity id → 表示名。語りは名前で書かれ ops は id なので、対応表を渡す。
    pub character_names: BTreeMap<String, String>,
    pub character_profiles: BTreeMap<String, String>,
    /// 秘匿の関係: entity id → (属性キー, 値)。
    pub secret_relations: BTreeMap<String, (String, String)>,
    /// 過去の語り (古い順)。
    pub past_narrations: Vec<String>,
}

/// 盤面から検査用スナップショットを作る (純関数)。
///
/// **`state` はターン開始時のもの**を渡すこと — 適用後の state を渡すと、移動ターンの語り
/// (出発地で始まる) が不在発話として誤検出される。
pub fn snapshot(
    state: &GameState,
    scenario: &Scenario,
    past_narrations: &[String],
) -> ConsistencySnapshot {
    let present: Vec<String> = std::iter::once(PLAYER.to_string())
        .chain(scenario.present_at(state))
        .collect();

    let player_inventory = state
        .inventory
        .get(PLAYER)
        .map(|s| s.iter().cloned().collect())
        .unwrap_or_default();
    let npc_inventory = state
        .inventory
        .iter()
        .filter(|(e, items)| e.as_str() != PLAYER && !items.is_empty())
        .map(|(e, items)| (e.clone(), items.iter().cloned().collect()))
        .collect();

    let mut character_names = BTreeMap::new();
    let mut character_profiles = BTreeMap::new();
    if !scenario.protagonist.name.trim().is_empty() {
        character_names.insert(PLAYER.to_string(), scenario.protagonist.name.clone());
    }
    if !scenario.protagonist.profile.trim().is_empty() {
        character_profiles.insert(PLAYER.to_string(), scenario.protagonist.profile.clone());
    }
    for (id, def) in &scenario.characters {
        if !def.name.trim().is_empty() {
            character_names.insert(id.clone(), def.name.clone());
        }
        if !def.profile.trim().is_empty() {
            character_profiles.insert(id.clone(), def.profile.clone());
        }
    }

    // 秘匿の関係 (spec 06 の secret_attributes / spec 32 で「本人未知」も同じ扱い)。
    // 値は実 state から取る — トリガーで書き換わるので宣言側では現在値が分からない。
    let mut secret_relations = BTreeMap::new();
    for (entity, attrs) in &state.attributes {
        for (key, value) in attrs {
            let secret = scenario.secret_attributes.contains(key)
                || scenario.hidden_attributes.contains(key);
            if secret && !value.trim().is_empty() {
                secret_relations.insert(entity.clone(), (key.clone(), value.clone()));
            }
        }
    }

    ConsistencySnapshot {
        location: state.location.clone(),
        present,
        player_inventory,
        npc_inventory,
        character_names,
        character_profiles,
        secret_relations,
        past_narrations: past_narrations.to_vec(),
    }
}

/// 1 回の検査。
#[derive(Debug, Clone)]
pub struct ConsistencyQuery {
    pub snapshot: ConsistencySnapshot,
    /// 検査対象の語り (適用後に確定)。
    pub narration: String,
    /// このターンに move op が受理されたか。**true なら移動の質問を出さない** (一方向検査)。
    pub moved_by_op: bool,
}

/// 秘匿の質問 id は `secret:{entity}` (entity ごとに 1 問 = 論点を 1 つに保つ)。
fn secret_question_id(entity: &str) -> String {
    format!("secret:{entity}")
}

/// 質問 id → 軸。秘匿だけ動的 id なので接頭辞で判る。
fn axis_of(id: &str) -> Option<Axis> {
    match id {
        "absent_speaker" => Some(Axis::AbsentSpeaker),
        "handed_unowned" => Some(Axis::HandedUnowned),
        "moved_without_op" => Some(Axis::MovedWithoutOp),
        "past_contradiction" => Some(Axis::PastContradiction),
        "profile_deviation" => Some(Axis::ProfileDeviation),
        "profile_contradiction" => Some(Axis::ProfileContradiction),
        _ if id.starts_with("secret:") => Some(Axis::SecretLeak),
        _ => None,
    }
}

/// 質問を組む (純関数)。
///
/// **出し分けが要点** — 材料が無い軸は問わない。秘匿が 0 件の盤面に秘匿を問えば、
/// 答えは常に低く出るのでノイズにしかならず、トークンも無駄になる。
///
/// 文言は 2026-09-21 に friday_lemmon の実ログで測って通ったもの。**変えたら再測定する。**
pub fn build_questions(query: &ConsistencyQuery) -> BTreeMap<String, NoulQuestion> {
    let s = &query.snapshot;
    let mut q = BTreeMap::new();

    q.insert(
        "absent_speaker".to_string(),
        NoulQuestion::new(
            "`narration` で台詞を話した人物のうち、`present` に含まれない者がいるか。\
`character_names` は id と表示名の対応表であり、主人公 player は「あなた」「私」「僕」として\
登場する。話題に出るだけ・名前が出るだけで台詞を話していない人物は数えない。",
        )
        .with_criteria(
            "present に無い人物が台詞を話している",
            "台詞を話したのは present に含まれる人物だけである",
        ),
    );

    q.insert(
        "handed_unowned".to_string(),
        NoulQuestion::new(
            "`narration` で主人公が誰かに手渡した物は、`player_inventory` に含まれない物か。",
        )
        .with_criteria(
            "player_inventory に無い物を手渡した",
            "手渡した物は player_inventory にある、または何も手渡していない",
        ),
    );

    // 一方向検査: move op が受理されたターンは問わない。op ありで語りが移動を完了して
    // いないのは「語りが 1 ターン遅れて場所を確定する」作法であって違反ではない (実測)。
    if !query.moved_by_op {
        q.insert(
            "moved_without_op".to_string(),
            NoulQuestion::new(
                "`narration` のなかで、主人公自身が `location` から別の場所へ実際に移動し終えたか。\
他の人物が移動しても、主人公が動いていなければ数えない。移動の意思を述べただけ、\
扉に手をかけただけなど移動の途中で終わっている場合は数えない。",
            )
            .with_criteria(
                "主人公が別の場所へ移動し終えた",
                "主人公はまだ同じ場所にいる",
            ),
        );
    }

    if !s.past_narrations.is_empty() {
        q.insert(
            "past_contradiction".to_string(),
            NoulQuestion::new(
                "`narration` の記述は、`past_narrations` で既に語られた事実 (人物の名前・\
関係・来訪の曜日や時刻・過去の出来事) と食い違っているか。",
            )
            .with_criteria(
                "過去に語られた事実と食い違う",
                "過去に語られた事実と一致している、または新しい話題である",
            ),
        );
    }

    if !s.character_profiles.is_empty() {
        q.insert(
            "profile_contradiction".to_string(),
            NoulQuestion::new(
                "`narration` の中に、`character_profiles` に書かれた人物の設定と食い違う記述があるか。",
            )
            .with_criteria("profile と食い違う記述がある", "profile と矛盾しない"),
        );
    }
    // 人物像からの逸脱は**1 人でも成立する** — 旧「混入」と違い「別の人物」を必要としない。
    //
    // **論点は 1 つに保つ** (2026-09-21 に実データで確かめた): 旧問いを厳密化して
    // 「その人物自身の profile に無く・別の人物の profile にあり・共有なら除外」と
    // 三つ載せた版は、偽陽性も真陽性も一緒に中央へ寄せて判別力を半分にした
    // (ギャップ 0.50 → 0.25)。削って 1 論点にしたこの問いが分離する側。
    if !s.character_profiles.is_empty() {
        q.insert(
            "profile_deviation".to_string(),
            NoulQuestion::new(
                "`narration` の中で、`character_profiles` に書かれた人物像から外れた振る舞いを\
している人物がいるか。",
            )
            .with_criteria(
                "profile の人物像から外れた振る舞いがある",
                "各人物は profile どおりに振る舞っている",
            ),
        );
    }

    // 秘匿は **関係ごとに 1 問**。一括で問うと公開情報 (「孫」) に引っ張られて
    // 実語りが 0.41 まで上がった — 割ると 0.19/0.16 に落ちた (実測)。
    for (entity, (key, value)) in &s.secret_relations {
        let name = s
            .character_names
            .get(entity)
            .cloned()
            .unwrap_or_else(|| entity.clone());
        q.insert(
            secret_question_id(entity),
            NoulQuestion::new(format!(
                "`narration` は、{name} の「{key}」が「{value}」であることを、\
読者に分かる形で述べているか。既に公然と知られている事柄は数えない。"
            ))
            .with_criteria(
                format!("{name} が「{value}」だと読者に分かる形で述べられている"),
                "伏せられている、または述べられていない",
            ),
        );
    }

    q
}

/// Jev に渡す `state` を組む (純関数)。
///
/// **空の欄は入れない** — 材料が無いのに欄だけあると、モデルが「無い」ことを判断材料にする。
/// `past_narrations` は [`MAX_PAST_CHARS`] を超えたら**古い方から**落とす。
pub fn build_state(query: &ConsistencyQuery) -> serde_json::Value {
    let s = &query.snapshot;
    let mut obj = serde_json::Map::new();

    obj.insert("location".into(), s.location.clone().into());
    obj.insert("present".into(), s.present.clone().into());
    obj.insert("player_inventory".into(), s.player_inventory.clone().into());
    if !s.npc_inventory.is_empty() {
        obj.insert(
            "npc_inventory".into(),
            serde_json::to_value(&s.npc_inventory).unwrap_or(serde_json::Value::Null),
        );
    }
    if !s.character_names.is_empty() {
        obj.insert(
            "character_names".into(),
            serde_json::to_value(&s.character_names).unwrap_or(serde_json::Value::Null),
        );
    }
    if !s.character_profiles.is_empty() {
        obj.insert(
            "character_profiles".into(),
            serde_json::to_value(&s.character_profiles).unwrap_or(serde_json::Value::Null),
        );
    }
    if !s.secret_relations.is_empty() {
        let rendered: BTreeMap<&String, String> = s
            .secret_relations
            .iter()
            .map(|(e, (k, v))| (e, format!("{k}={v}")))
            .collect();
        obj.insert(
            "secret_relations".into(),
            serde_json::to_value(&rendered).unwrap_or(serde_json::Value::Null),
        );
    }
    if !s.past_narrations.is_empty() {
        obj.insert("past_narrations".into(), trim_past(&s.past_narrations).into());
    }
    obj.insert("narration".into(), query.narration.clone().into());

    serde_json::Value::Object(obj)
}

/// 予算内に収まるよう**古い方から**落として連結する。
fn trim_past(past: &[String]) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut total = 0usize;
    for text in past.iter().rev() {
        let len = text.chars().count();
        if total + len > MAX_PAST_CHARS && !kept.is_empty() {
            break;
        }
        total += len;
        kept.push(text.as_str());
    }
    kept.reverse();
    kept.join("\n\n")
}

/// 1 軸の判定。
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub axis: Axis,
    /// 質問 id (秘匿は `secret:{entity}`)。
    pub question_id: String,
    pub score: f64,
    /// 閾値を超えたか。**Phase A では印であって却下条件ではない。**
    pub flagged: bool,
}

/// 検査 1 回の結果。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConsistencyReport {
    pub findings: Vec<Finding>,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

impl ConsistencyReport {
    /// 閾値を超えた軸だけ (表示・記録用)。**スコアの高い順**。
    pub fn flagged(&self) -> Vec<&Finding> {
        let mut v: Vec<&Finding> = self.findings.iter().filter(|f| f.flagged).collect();
        v.sort_by(|a, b| b.score.total_cmp(&a.score));
        v
    }
}

/// 答えを軸へ解く (純関数)。
///
/// 未回答の質問は**落とす** (0.0 に潰さない) — 答えが返らなかったことと「違反なし」は別。
pub fn interpret(
    response: &llm_client::jev::JevResponse,
    threshold: f64,
) -> ConsistencyReport {
    let mut findings: Vec<Finding> = response
        .answers
        .iter()
        .filter_map(|(id, score)| {
            axis_of(id).map(|axis| Finding {
                axis,
                question_id: id.clone(),
                score: *score,
                flagged: *score >= threshold,
            })
        })
        .collect();
    findings.sort_by(|a, b| a.question_id.cmp(&b.question_id));
    ConsistencyReport {
        findings,
        model: response.model.clone(),
        prompt_tokens: response.usage.prompt,
        completion_tokens: response.usage.completion,
    }
}

/// 語りの検査者。[`crate::DeltaProposer`] / [`crate::synopsis::Summarizer`] と同型の依存性逆転 —
/// 実装は `llm_client::JevClient`、テストは fake。
#[allow(async_fn_in_trait)] // 本 crate 内でしか実装/消費しないため dyn 化の懸念なし
pub trait ConsistencyChecker {
    async fn check(&self, query: &ConsistencyQuery) -> Result<ConsistencyReport, HarnessError>;
}

impl ConsistencyChecker for JevClient {
    async fn check(&self, query: &ConsistencyQuery) -> Result<ConsistencyReport, HarnessError> {
        let questions = build_questions(query);
        if questions.is_empty() {
            return Ok(ConsistencyReport::default());
        }
        let state = build_state(query);
        let response = self
            .ask(&state, &questions)
            .await
            .map_err(|e| HarnessError::Consistency(e.to_string()))?;
        Ok(interpret(&response, DEFAULT_THRESHOLD))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> ConsistencySnapshot {
        ConsistencySnapshot {
            location: "cafe_counter".into(),
            present: vec!["player".into(), "akari".into()],
            player_inventory: vec!["メモ帳".into()],
            npc_inventory: BTreeMap::from([("fudemura".to_string(), vec!["万年筆".to_string()])]),
            character_names: BTreeMap::from([
                ("player".to_string(), "あなた".to_string()),
                ("akari".to_string(), "岬あかり".to_string()),
                ("genzo".to_string(), "岬源蔵".to_string()),
            ]),
            character_profiles: BTreeMap::from([
                ("akari".to_string(), "17歳。酸っぱいものが苦手。".to_string()),
                ("genzo".to_string(), "78歳。「豆は生きてるからな」が口癖。".to_string()),
            ]),
            secret_relations: BTreeMap::from([(
                "genzo".to_string(),
                ("関係".to_string(), "祖父であり父".to_string()),
            )]),
            past_narrations: vec!["T1: 商店街であかりに会った。".into()],
        }
    }

    fn query(moved_by_op: bool) -> ConsistencyQuery {
        ConsistencyQuery {
            snapshot: snap(),
            narration: "あかりは俯いた。".into(),
            moved_by_op,
        }
    }

    /// 材料が無い軸は問わない。**出し分けが効かないと、常に低く出る質問で
    /// jsonl が埋まりトークンも無駄になる。**
    #[test]
    fn questions_are_gated_by_available_material() {
        let q = build_questions(&query(false));
        assert!(q.contains_key("absent_speaker"));
        assert!(q.contains_key("handed_unowned"));
        assert!(q.contains_key("moved_without_op"), "move op が無いなら問う");
        assert!(q.contains_key("past_contradiction"));
        assert!(q.contains_key("profile_contradiction"));
        assert!(q.contains_key("profile_deviation"), "profile があれば逸脱を問う");
        assert!(q.contains_key("secret:genzo"), "秘匿は entity ごとに 1 問");

        // move op ありのターンは移動を問わない (一方向検査)。
        let moved = build_questions(&query(true));
        assert!(
            !moved.contains_key("moved_without_op"),
            "op が移動済みのターンで語りの移動を咎めてはいけない (語りの作法)"
        );

        // 材料を削ると、その軸は消える。
        let mut bare = query(false);
        bare.snapshot.past_narrations.clear();
        bare.snapshot.secret_relations.clear();
        bare.snapshot.character_profiles.remove("genzo"); // 1 件だけ残す
        let q2 = build_questions(&bare);
        assert!(!q2.contains_key("past_contradiction"));
        assert!(!q2.keys().any(|k| k.starts_with("secret:")));
        // **1 人でも逸脱は問える** — 旧「混入」は 2 人を要したが、人物像から外れた振る舞いは
        // その人物 1 人で成立する (2026-09-21 の差し替え)。
        assert!(q2.contains_key("profile_deviation"), "1 人でも逸脱は問える");
        assert!(q2.contains_key("profile_contradiction"), "1 件でも矛盾は問える");

        // profile が 0 件ならどちらも問わない (材料が無い)。
        let mut no_profiles = query(false);
        no_profiles.snapshot.character_profiles.clear();
        let q3 = build_questions(&no_profiles);
        assert!(!q3.contains_key("profile_deviation"));
        assert!(!q3.contains_key("profile_contradiction"));
    }

    /// 質問文は実データで測って通った文言。**変えたら再測定する** — live で一文を落としたら
    /// 確度が 0.87 → 0.51 まで落ちたので、ここは GM_SYSTEM と同じ扱いで固定する。
    #[test]
    fn question_text_is_pinned_to_the_measured_wording() {
        let q = build_questions(&query(false));

        let absent = &q["absent_speaker"].instructions;
        assert!(
            absent.contains("話題に出るだけ"),
            "この一文の有無で 0.87 → 0.51 まで動いた (live 実測)"
        );
        assert!(absent.contains("character_names"), "id と表示名の対応が要る");

        let moved = &q["moved_without_op"].instructions;
        assert!(
            moved.contains("扉に手をかけただけ"),
            "移動の意思と完了を分ける一文 (実測 0.04 / 0.74 の分離)"
        );

        // **論点は 1 つ**。「別の人物の特徴として書かれているもの」を問う旧版は実プレイ
        // 43 ターンで分離しなかった (真相開示のたびに 0.5 超え = ミステリでは特徴の共有が
        // 手がかりそのもの)。厳密化して条件を足した版は判別力が半分になった。
        let deviation = &q["profile_deviation"].instructions;
        assert!(deviation.contains("人物像から外れた"));
        assert!(
            !deviation.contains("別の人物"),
            "論点を 2 つにしない (厳密化は実測で判別力を落とした)"
        );

        // criteria は全問に付ける (付けないと精度が落ちる)。
        for (id, question) in &q {
            assert!(question.criteria.is_some(), "{id} に criteria が無い");
        }
    }

    /// 秘匿の質問は関係ごとに割る (一括だと公開情報に引っ張られる)。
    #[test]
    fn secret_questions_are_split_per_relation() {
        let mut qq = query(false);
        qq.snapshot.secret_relations.insert(
            "reiko".to_string(),
            ("関係".to_string(), "母であり娘".to_string()),
        );
        let q = build_questions(&qq);
        assert!(q["secret:genzo"].instructions.contains("祖父であり父"));
        assert!(q["secret:reiko"].instructions.contains("母であり娘"));
        assert!(
            q["secret:genzo"].instructions.contains("岬源蔵"),
            "id でなく表示名で問う (語りは名前で書かれる)"
        );
    }

    /// state は空の欄を持たない。材料が無いのに欄だけあると、モデルが「無い」ことを
    /// 判断材料にしてしまう。
    #[test]
    fn state_omits_empty_sections() {
        let mut bare = query(false);
        bare.snapshot.npc_inventory.clear();
        bare.snapshot.character_profiles.clear();
        bare.snapshot.secret_relations.clear();
        bare.snapshot.past_narrations.clear();

        let v = build_state(&bare);
        assert!(v.get("npc_inventory").is_none());
        assert!(v.get("character_profiles").is_none());
        assert!(v.get("secret_relations").is_none());
        assert!(v.get("past_narrations").is_none());
        // 常に要る欄は残る。
        assert_eq!(v["location"], "cafe_counter");
        assert_eq!(v["narration"], "あかりは俯いた。");
        assert!(v["present"].as_array().unwrap().contains(&"akari".into()));
    }

    /// 過去の語りは予算を超えたら**古い方から**落ちる (直近が残る)。
    #[test]
    fn past_narrations_drop_the_oldest_first() {
        let mut q = query(false);
        q.snapshot.past_narrations = vec![
            "古".repeat(MAX_PAST_CHARS),
            "中".repeat(MAX_PAST_CHARS / 2),
            "新".to_string(),
        ];
        let v = build_state(&q);
        let past = v["past_narrations"].as_str().unwrap();
        assert!(past.contains('新'), "直近は必ず残る");
        assert!(!past.contains('古'), "溢れた古い方が落ちる");
        assert!(past.chars().count() <= MAX_PAST_CHARS + 2);

        // 1 本しか無くそれが予算超過でも、空にはしない (落とすと検査そのものが無意味になる)。
        let mut single = query(false);
        single.snapshot.past_narrations = vec!["長".repeat(MAX_PAST_CHARS * 2)];
        let v2 = build_state(&single);
        assert!(!v2["past_narrations"].as_str().unwrap().is_empty());
    }

    /// 答えを軸へ解く。未回答は落とす (0.0 に潰さない)。
    #[test]
    fn interpret_maps_answers_to_axes() {
        let response = llm_client::jev::JevResponse {
            model: "jev-1.13.0".into(),
            answers: BTreeMap::from([
                ("absent_speaker".to_string(), 0.93),
                ("handed_unowned".to_string(), 0.10),
                ("secret:genzo".to_string(), 0.80),
                ("unknown_question".to_string(), 0.99),
            ]),
            usage: llm_client::Usage {
                prompt: 931,
                completion: 128,
                cache_read: 0,
                cost_usd: None,
            },
        };
        let report = interpret(&response, DEFAULT_THRESHOLD);

        assert_eq!(report.findings.len(), 3, "未知の質問 id は載せない");
        assert_eq!(report.prompt_tokens, 931);

        let flagged = report.flagged();
        assert_eq!(flagged.len(), 2);
        assert_eq!(flagged[0].axis, Axis::AbsentSpeaker, "スコアの高い順");
        assert_eq!(flagged[1].axis, Axis::SecretLeak);

        // 秘匿は還流に乗せない軸として型で判る。
        assert!(Axis::SecretLeak.is_advisory());
        assert!(!Axis::AbsentSpeaker.is_advisory());
        assert!(!Axis::PastContradiction.is_advisory());
    }

    /// 盤面からスナップショットを作る。**主人公は present に含める** —
    /// `Location.present` は NPC だけを宣言する形なので、そのまま渡すと
    /// 「私」の発話が不在発話として誤検出される (実測 0.5〜0.61)。
    #[test]
    fn snapshot_includes_the_protagonist_in_present() {
        let scenario: Scenario = serde_yaml::from_str(
            r#"
title: t
start: room
protagonist: { name: "主人公", profile: "17歳の探偵役。" }
locations:
  room:
    description: d
    present: [akari]
characters:
  akari: { name: "岬あかり", profile: "17歳。", attributes: { 関係: 孫娘 } }
secret_attributes: [関係]
goal: { kind: flag_is, key: x, value: true }
allowed_flags: [x]
"#,
        )
        .expect("scenario");
        let state = scenario.initial_state(1);
        let snap = snapshot(&state, &scenario, &["T1: 会った。".to_string()]);

        assert!(snap.present.contains(&"player".to_string()), "主人公を含める");
        assert!(snap.present.contains(&"akari".to_string()));
        assert_eq!(snap.character_names["akari"], "岬あかり");
        assert_eq!(snap.character_names["player"], "主人公");
        assert!(snap.character_profiles.contains_key("player"));
        assert_eq!(
            snap.secret_relations["akari"],
            ("関係".to_string(), "孫娘".to_string()),
            "秘匿属性の現在値を state から取る"
        );
        assert_eq!(snap.location, "room");
    }

    /// fake の checker を差せる (依存性逆転の確認)。**検査の失敗は Err で返り、
    /// 呼び出し側がターンを落とさずに済む**。
    #[tokio::test]
    async fn a_failing_checker_returns_err_without_panicking() {
        struct Broken;
        impl ConsistencyChecker for Broken {
            async fn check(
                &self,
                _q: &ConsistencyQuery,
            ) -> Result<ConsistencyReport, HarnessError> {
                Err(HarnessError::Consistency("402 クレジット不足".into()))
            }
        }
        let err = Broken.check(&query(false)).await.unwrap_err();
        assert!(err.to_string().contains("402"));

        struct Silent;
        impl ConsistencyChecker for Silent {
            async fn check(
                &self,
                _q: &ConsistencyQuery,
            ) -> Result<ConsistencyReport, HarnessError> {
                Ok(ConsistencyReport::default())
            }
        }
        let report = Silent.check(&query(false)).await.expect("ok");
        assert!(report.flagged().is_empty());
    }
}
