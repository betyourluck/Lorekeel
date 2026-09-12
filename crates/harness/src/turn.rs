//! GM ターンループ。LocalAI `orchestrator.py::_self_repair_loop` と同型。
//!
//! 提案 → 裁定 → 却下なら理由を戻して再生成 → 受理なら原子適用。
//! 正本 (gm_core) が真実を握り、LLM の流暢な嘘はここで構造的に弾かれる。

use gm_core::{
    adjudicate, apply, CheckOutcome, FiredTrigger, GameState, Lang, RejectReason, RollOutcome,
    Scenario, StateDelta, Verdict, PLAYER,
};

use crate::memoria::MemoryFragment;
use llm_client::ChatMessage;

use crate::error::HarnessError;
use crate::proposer::DeltaProposer;
use crate::prompt;

/// 経緯ログの 1 エントリ (chronicle)。「経過を忘れる GM」対策 —
/// GM が毎ターン書く 1 行要約 ([`gm_core::StateDelta`] の `summary`、無ければ narration 冒頭) を
/// 呼び出し側 (CLI/app) が [`chronicle_entry`] で蓄積し、[`run_turn`] の `history` に渡すと
/// 「これまでの経緯」として prompt に還流される (recent_narration=直前 1 ターンの中期記憶版)。
/// 語り素材であって正本状態ではない (Memoria 同様、可変世界状態は持たない)。Serialize 派生は
/// 将来のセーブ同梱用。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TurnLog {
    /// 適用後のターン番号。
    pub turn: u32,
    /// プレイヤーの行動文 (そのターンの入力)。
    pub player: String,
    /// そのターンの経緯 1 行 (GM の summary、fallback は narration 冒頭)。
    pub summary: String,
    // --- spec 08-B: engine 接地の機械タグ (受理時点の確定事実、LLM 非関与) ---
    // retrieval (spec 08-A) の検索を LLM の summary 品質に依存させないための正本由来タグ。
    // 全て serde default = 旧セーブ (spec 07) のタグ無し TurnLog もそのまま読める。
    /// 適用後の現在地。
    #[serde(default)]
    pub location: String,
    /// 適用後にその場に居た NPC (実効 presence)。
    #[serde(default)]
    pub present: Vec<String>,
    /// このターンに真化したフラグ (`flag_turns` の差分 = op/トリガー/challenge の全経路捕捉)。
    #[serde(default)]
    pub flags_set: Vec<String>,
    /// 技能判定の要約 (「STR 1d20+3=17 vs DC15 成功」)。
    #[serde(default)]
    pub checks: Vec<String>,
    /// 所持品の増減 (apply 前後の inventory 差分、「+祠の鍵」「alice:+花」)。
    #[serde(default)]
    pub items: Vec<String>,
}

/// 受理ターンの engine 事実タグ (spec 08-B)。[`run_turn`] が apply の前後から機械計上し、
/// [`TurnOutcome::Accepted`] で運ぶ。呼び出し側は [`chronicle_entry`] へ渡すだけ。
#[derive(Debug, Clone, Default)]
pub struct ChronicleTags {
    /// 適用後の現在地。
    pub location: String,
    /// 適用後の実効 presence。
    pub present: Vec<String>,
    /// このターンに真化したフラグ。
    pub flags_set: Vec<String>,
    /// 所持品の増減 (「+アイテム」「-アイテム」、NPC は「id:+アイテム」)。
    pub items: Vec<String>,
}

/// 受理ターンから経緯ログの 1 エントリを作る。GM が `summary` を書いていればそれを、
/// 書いていない (弱モデル等) なら narration 冒頭を文字境界安全に切り詰めて使う。
///
/// `beats` は発火した反応ビートの authored narration。**GM は見ていない** (発火は提案後に
/// engine 側で起きる) ので GM の summary には現れない — ここで「出来事」として併記し、
/// 筋書きの出来事が経緯から抜け落ちないようにする。
pub fn chronicle_entry(
    turn: u32,
    player: &str,
    summary: &str,
    narration: &str,
    beats: &[String],
    tags: &ChronicleTags,
    checks: &[CheckOutcome],
) -> TurnLog {
    let mut summary = if summary.trim().is_empty() {
        let flat = narration.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut head: String = flat.chars().take(80).collect();
        if flat.chars().count() > 80 {
            head.push('…');
        }
        head
    } else {
        summary.trim().to_string()
    };
    let beats: Vec<String> = beats
        .iter()
        .filter(|b| !b.trim().is_empty())
        .map(|b| b.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    if !beats.is_empty() {
        summary.push_str(&format!("／出来事: {}", beats.join("、")));
    }
    // 判定の authored 結末文 (#41)。GM の summary は結果確定前に書かれる (「試みた」止まり)
    // ので、確定した結末を中期記憶にも併記する (ビートの「出来事」と同じ理由)。
    let outcomes: Vec<String> = checks
        .iter()
        .filter(|c| !c.narration.trim().is_empty())
        .map(|c| c.narration.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    if !outcomes.is_empty() {
        summary.push_str(&format!("／判定の結末: {}", outcomes.join("、")));
    }
    TurnLog {
        turn,
        player: player.to_string(),
        summary,
        location: tags.location.clone(),
        present: tags.present.clone(),
        flags_set: tags.flags_set.clone(),
        checks: checks
            .iter()
            .map(|c| {
                format!(
                    "{} {} 1d{}{:+}={} vs DC{} {}",
                    c.entity,
                    c.stat,
                    c.sides,
                    c.modifier,
                    c.total,
                    c.dc,
                    if c.success { "成功" } else { "失敗" }
                )
            })
            .collect(),
        items: tags.items.clone(),
    }
}

/// 盤面の判定様式 (spec 16) から、LLM の op 語彙 (schema) で**使わせない**判定 op を導く。
/// percentile → 加算式 `check` を隠す / additive (既定) → `check_under` を隠す。
/// app/CLI が `LlmClient::set_excluded_ops` に渡す (new_game と campaign 遷移時)。
pub fn excluded_check_ops(scenario: &Scenario) -> Vec<String> {
    match scenario.check_style {
        gm_core::CheckStyle::Percentile => vec!["check".to_string()],
        gm_core::CheckStyle::Additive => vec!["check_under".to_string()],
    }
}

/// 直前の語りを何ターン分そのまま GM に渡すかの既定 (`LOREKEEL_RECENT_TURNS` 未設定時)。
/// 1 = 2026-06-24 以来の従来挙動 (直前 1 ターンだけ)。**既定を 3 にした根拠**: GM は毎ターン
/// messages を組み立て直すので、逐語で持つのはこの層だけ — 要約 1 行 (chronicle) に書かれ
/// なかった台詞・約束の細部は、ここに無ければその場で失われる (2026-09-13 ユーザー報告
/// 「矛盾しない GM が Web 版より忘れる」の主因)。代償は可変側 (キャッシュに乗らない) の
/// 入力トークンで、1 本 300〜800 字 = 3 本で 1〜2k トークン程度。
pub const RECENT_NARRATIONS_DEFAULT: usize = 3;
/// 上限。これ以上は chronicle / synopsis (要約) の役割で、逐語で運ぶ層ではない。
pub const RECENT_NARRATIONS_MAX: usize = 20;
/// 逐語で運ぶ合計の字数予算。K を大きくしても可変側の課金がここで頭打ちになる
/// (最新の 1 本は予算を越えても必ず残す)。
pub const RECENT_NARRATIONS_BUDGET: usize = 8000;

/// `LOREKEEL_RECENT_TURNS` の解釈 (純関数)。未設定・空・不正・0 は既定、上限で丸める。
pub fn parse_recent_turns(raw: Option<&str>) -> usize {
    match raw.map(str::trim).filter(|s| !s.is_empty()).and_then(|s| s.parse::<usize>().ok()) {
        Some(0) | None => RECENT_NARRATIONS_DEFAULT,
        Some(n) => n.min(RECENT_NARRATIONS_MAX),
    }
}

/// 実効 K。env から読む (app は app_data/.env、CLI は repo .env)。
pub fn recent_turns_limit() -> usize {
    parse_recent_turns(crate::env_var("RECENT_TURNS").as_deref())
}

/// 語りのリングに 1 本積む (古い順・末尾が直前)。件数 `limit` と字数予算 [`RECENT_NARRATIONS_BUDGET`]
/// で古い方から落とす。空の語りは積まない。
pub fn push_recent_narration(ring: &mut Vec<String>, text: String, limit: usize) {
    if text.trim().is_empty() {
        return;
    }
    ring.push(text);
    let limit = limit.max(1);
    while ring.len() > limit {
        ring.remove(0);
    }
    while ring.len() > 1
        && ring.iter().map(|s| s.chars().count()).sum::<usize>() > RECENT_NARRATIONS_BUDGET
    {
        ring.remove(0);
    }
}

/// 直前の語りの末尾に、後から確定した出来事 (決断の結末・対決の digest) を継ぎ足す —
/// [`carryover_narration`] を最後の 1 本へ適用する。空なら新しく 1 本になる。
pub fn extend_last_narration(ring: &mut Vec<String>, beats: &[String], checks: &[CheckOutcome]) {
    let base = ring.pop().unwrap_or_default();
    let merged = carryover_narration(&base, beats, checks);
    if !merged.trim().is_empty() {
        ring.push(merged);
    }
}

/// 再開時に GM へ渡す直前の語りの列: 新欄 `recent_narrations` があればそれ、無ければ
/// (2026-09-13 より前のセーブ) `last_narration` 1 本だけ。
pub fn seed_recent_narrations(recent: Vec<String>, last: &str) -> Vec<String> {
    if !recent.is_empty() {
        recent
    } else if last.trim().is_empty() {
        Vec::new()
    } else {
        vec![last.to_string()]
    }
}

/// 次ターンへ持ち越す「直前までの語り」を組む。GM の narration に、**GM が見ていない**
/// authored テキスト 2 種を連結する: 発火ビート (筋書きの出来事、#27 のトリガー版) と
/// **判定の結末文** (`CheckOutcome.narration`、#41) — 出目は apply 後に確定するので GM の
/// 語りは「試みる」止まりで、authored 結末 (「見事に仕留めた」) はプレイヤーにだけ表示される。
/// ここで継続文脈に含めないと GM が出来事・結末を知らないまま続きを語る。
pub fn carryover_narration(narration: &str, beats: &[String], checks: &[CheckOutcome]) -> String {
    let beats: Vec<&String> = beats.iter().filter(|b| !b.trim().is_empty()).collect();
    // 結末文を持つ判定だけ (素の Check は check_outcome_note が次ターンに「語れ」と要求する
    // 別経路 — こちらは「語られ済みの事実を知らせる」経路で、役割を分ける)。
    let outcomes: Vec<&str> = checks
        .iter()
        .filter(|c| !c.narration.trim().is_empty())
        .map(|c| c.narration.as_str())
        .collect();
    if beats.is_empty() && outcomes.is_empty() {
        return narration.to_string();
    }
    let mut s = narration.trim_end().to_string();
    if !beats.is_empty() {
        s.push_str("\n（直後に筋書きの出来事が起きた）");
        for b in beats {
            s.push('\n');
            s.push_str(b.trim());
        }
    }
    if !outcomes.is_empty() {
        s.push_str("\n（直後に判定の結末が確定した）");
        for o in outcomes {
            s.push('\n');
            s.push_str(o.trim());
        }
    }
    s
}

/// 対決の決着 digest (spec 18 Phase C)。何交換あっても GM にはこの 1 行だけを渡す —
/// トークン経済の要 (HP 等の現在値は state_brief が別途運ぶので数値は重複させない)。
/// 呼び出し側 (app/CLI) が `carryover_narration` の beats と chronicle summary に併記する。
pub fn contest_digest(end: &gm_core::ContestEnd) -> String {
    let desc = if end.description.trim().is_empty() {
        end.contest.as_str()
    } else {
        end.description.trim()
    };
    let how = match end.reason.as_str() {
        "until" => "決着した",
        "goal" => "決着した (結末に到達)",
        _ => "決着つかず打ち切られた",
    };
    format!(
        "対決「{desc}」は {} 交換 (勝ち {} / 負け {} / 分け {}) で{how}",
        end.rounds, end.wins, end.losses, end.ties
    )
}

/// 受理適用の直後に engine 事実からタグを機械計上する (spec 08-B)。LLM は関与しない。
/// `state` は適用後、`inv_before` は適用前の inventory の写し。
fn chronicle_tags(
    state: &GameState,
    scenario: &Scenario,
    inv_before: &std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
) -> ChronicleTags {
    // このターンに真化したフラグ = flag_turns がこのターン番号を刻んだもの
    // (op / トリガー効果 / challenge 帰結の全経路を apply が捕捉済み)。
    let flags_set: Vec<String> = state
        .flag_turns
        .iter()
        .filter(|(_, &t)| t == state.turn)
        .map(|(k, _)| k.clone())
        .collect();
    // 所持品の増減 (前後差分)。player は素のまま、NPC は "id:" を前置。
    let mut items: Vec<String> = Vec::new();
    let empty = std::collections::BTreeSet::new();
    let entities: std::collections::BTreeSet<&String> =
        inv_before.keys().chain(state.inventory.keys()).collect();
    for eid in entities {
        let before = inv_before.get(eid).unwrap_or(&empty);
        let after = state.inventory.get(eid).unwrap_or(&empty);
        let prefix = if eid == PLAYER { String::new() } else { format!("{eid}:") };
        for gained in after.difference(before) {
            items.push(format!("{prefix}+{gained}"));
        }
        for lost in before.difference(after) {
            items.push(format!("{prefix}-{lost}"));
        }
    }
    ChronicleTags {
        location: state.location.clone(),
        present: scenario.present_at(state).into_iter().collect(),
        flags_set,
        items,
    }
}

/// 1 ターンの結末。
// Accepted は語り+判定+タグを丸ごと運ぶので Rejected よりずっと大きいが、ターン毎に 1 個
// 生まれてすぐ消える一時値 — Box 化の複雑さに見合わない (clippy::large_enum_variant は承知)。
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum TurnOutcome {
    /// 受理されて state に適用された。
    Accepted {
        narration: String,
        /// GM 自身が書いたこのターンの経緯 1 行 (StateDelta.summary、非検証)。
        /// 呼び出し側が [`chronicle_entry`] で経緯ログに積む素。
        summary: String,
        rolls: Vec<RollOutcome>,
        /// この適用で行われた技能判定の結果。次ターンの語りに還流される。
        checks: Vec<CheckOutcome>,
        /// 可変量ダイス (`roll_stat`) の監査記録 (spec 16)。提示層が「SAN -4 (1d6=4)」を出す素。
        stat_rolls: Vec<gm_core::StatRollOutcome>,
        /// この適用で発火した反応ビート (Phase C)。`narration` を語りに注入する。
        fired: Vec<FiredTrigger>,
        /// 受理までに要した試行回数 (1 = 一発合格)。
        attempts: u32,
        /// 受理前にやり直した各試行の**原因** (試行順)。空なら一発合格。提示層が「なぜ筋を通すのに
        /// N 回かかったか」を author に見せる素 (Grok 等で却下が多い時の診断)。
        ///
        /// **却下 (裁定) と パース失敗 (壊れた出力) の二種がある** — 当初は前者だけを積んでおり、
        /// 後者は `continue` するだけで理由が捨てられていた (attempts だけ増えて提示層に
        /// 「却下された試行:」の空欄が出る、2026-07-21 実プレイ発見)。原因の型で区別する。
        retries: Vec<RetryCause>,
        /// engine 事実の機械タグ (spec 08-B)。呼び出し側が [`chronicle_entry`] へ渡す。
        tags: ChronicleTags,
        // spec 20: GM の既成事実提案フィールドは撤去した (実測 0/45・0/20 の絶対ゼロ —
        // 語り手に記録係を兼ねさせるのが構造的に無理。failures.md #65)。ターンからは何も返らない。
    },
    /// 最大試行回数まで却下され続けた。**state は無傷**。理由は構造化 (表示は localize)。
    Rejected {
        last_reasons: Vec<RejectReason>,
        attempts: u32,
    },
}

impl TurnOutcome {
    pub fn is_accepted(&self) -> bool {
        matches!(self, TurnOutcome::Accepted { .. })
    }
}

/// 受理までに**やり直した原因** (spec: 自己修復の診断)。「なぜ N 回かかったか」は二種あり、
/// どちらも author の診断材料になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryCause {
    /// 裁定で却下された。理由は構造化 (`RejectReason`) — 文面は提示層が `localize` する。
    Rejected(Vec<RejectReason>),
    /// **構造化出力が壊れて読めなかった** (`LlmError::Parse`)。裁定にすら到達していない。
    /// 典型: モデルが散文で断った / CoT 混入 (#30) / max_tokens 切断 / ops の型崩れ (#52)。
    /// `detail` はパーサの説明 (構造化理由が存在しないので文字列で運ぶ)。
    Malformed { detail: String },
}

/// 1 ターンを回す。
///
/// `max_attempts` 回まで提案を裁定し、却下されるたびに理由を messages に積んで再生成させる。
/// 受理されたら `state` に原子適用して [`TurnOutcome::Accepted`]、尽きたら [`TurnOutcome::Rejected`]
/// (このとき `state` は一切変わっていない)。
///
/// `recalled_lore` は memoria_bridge: 直前ターンの発火で Memoria から recall された伏線。
/// 今回の語りに「思い出す様子」として織り込ませるため prompt に注入する (空なら注入しない)。
/// `recent_checks` は直前ターンの技能判定の結果。出目は apply 後に確定するので同一ターンの
/// narration に間に合わない → 次ターンの prompt に還流し、GM に結果へ沿って語らせる。
/// `recent_narrations` は直前 K ターンの語り (古い順・末尾が直前。K は呼び出し側が [`recent_turns_limit`] で決める。2026-09-13 までは直前 1 ターンだけだった)。継続文脈として渡し、既出情景の繰り返しを防ぐ
/// (毎ターン messages を新規構築するので LLM は自分の直前の語りを記憶していない)。
/// `history` は経緯ログ (chronicle)。過去ターンの 1 行要約列を「これまでの経緯」として
/// 注入し、GM が数ターン前の経過を保持する (recent_narration の中期記憶版)。
/// `synopsis` はあらすじ (spec 10)。圧縮済みの章 segment 列を「これまでのあらすじ」として
/// 注入し、chronicle の予算からあふれた古い物語の連続性を保持する (長期記憶)。spec 14 で
/// 可変 user から**独立した 2 本目の leading system** へ分離 (append-only = 章追加の間
/// byte 安定 → 第二のキャッシュ段。提示位置は「history の前」から「state の前」へ)。
/// `party` は多人数プレイの参加者 (spec 23 Phase B)。2 人以上なら GM_SYSTEM の末尾に
/// 多人数接地 ([`prompt::party_note`]) を足す — 単騎 (空/1 人) では 1 バイトも変わらない
/// (安定プレフィックス不変)。party はセッション中不変なのでキャッシュも保たれる。
#[allow(clippy::too_many_arguments)]
pub async fn run_turn<P: DeltaProposer>(
    proposer: &P,
    state: &mut GameState,
    scenario: &Scenario,
    player_action: &str,
    max_attempts: u32,
    lang: Lang,
    recalled_lore: &[MemoryFragment],
    recent_checks: &[CheckOutcome],
    recent_narrations: &[String],
    history: &[TurnLog],
    synopsis: &[crate::SynopsisEntry],
    facts: &[crate::FactEntry],
    party: &[prompt::PartyMember],
) -> Result<TurnOutcome, HarnessError> {
    // 盤面と現在状態を毎ターン新規に提示する (state は正本の唯一の真実)。
    // history=過去ターンの経緯、recalled_lore=思い出された伏線、recent_checks=直前判定の結果、
    // recent_narrations=直前 K ターンの語り (継続文脈、繰り返し禁止) を語りに還流する。
    let mut system = prompt::gm_system_prompt(scenario, prompt::dev_mode_enabled());
    // 多人数接地は最初の system ブロックの**末尾**に足す (前方の安定プレフィックスを
    // 動かさない。party はセッション不変なのでこのブロック自体もセッション内で安定)。
    let party_block = prompt::party_note(party);
    if !party_block.is_empty() {
        system.push_str("\n\n");
        system.push_str(&party_block);
    }
    let mut messages = vec![
        // dev モード (LOREKEEL_DEV_MODE) なら DEV_META を先頭に足す (env 直読み、signature 不変)。
        ChatMessage::system(system),
    ];
    // spec 14 Phase B: append-only あらすじ (spec 10) は可変 user に混ぜず、独立した
    // **2 本目の leading system** として出す — user は state_brief が毎ターン変わるので
    // byte 0 から可変 = 中に置くとキャッシュに乗らない。分離すれば章追加の間は
    // `[system(静的), system(synopsis)]` が byte 安定 = 第二のキャッシュ段
    // (Anthropic 多段 breakpoint / OpenAI・Grok 自動延伸 / Gemini は inline 降格 = D4)。
    // 空なら出さない (breakpoint を無駄に使わない)。
    let synopsis_block = prompt::synopsis_note(synopsis);
    if !synopsis_block.is_empty() {
        messages.push(ChatMessage::system(synopsis_block.trim_start().to_string()));
    }
    messages.push(ChatMessage::user(format!(
        "{}{}{}{}{}{}{}\n\n# プレイヤーの行動\n{}",
        prompt::state_brief(state, scenario),
        // spec 20 既成事実: state_brief の直後・可変 user 側 (編集で変わるので system に
        // 置くとキャッシュを壊す)。従属規律ヘッダ + スコア降順 (UI と同じ並び)。
        crate::facts::facts_note(facts).unwrap_or_default(),
        // #49: 直前ターンで移動していたら、置いていかれた NPC を固有名で否定接地する
        // (GM 自身の移動語りの「同行の素振り」が recent_narration/chronicle 経由で
        // presence を汚染するのへの対抗。一般規律は具体の語りに負ける)。
        prompt::moved_note(scenario, state, history),
        // spec 08-A: 現在の文脈 (行動文 + 現在地 + presence) をクエリに、直近は無条件・
        // それより古い経緯は関連するものだけ想起する二層注入。
        prompt::history_note(
            history,
            &prompt::HistoryQuery {
                action: player_action,
                location: &state.location,
                present: scenario.present_at(state).into_iter().collect(),
            }
        ),
        prompt::check_outcome_note(recent_checks),
        prompt::recalled_lore_note(recalled_lore),
        prompt::recent_narrations_note(recent_narrations),
        player_action
    )));

    let mut last_reasons = Vec::new();
    // 受理前にやり直した原因 (試行順)。受理時に提示層へ渡し「なぜ N 回かかったか」を見せる。
    // 却下 (裁定) と パース失敗 (壊れた出力) の両方を積む — 後者を捨てると空欄が出る。
    let mut retries: Vec<RetryCause> = Vec::new();

    for attempt in 1..=max_attempts.max(1) {
        // 壊れた構造化出力 (JSON パース失敗) は却下と同じく「raw を戻して再提出」させる —
        // 「パース失敗は raw を保持し再生成の燃料にする」(llm_client #4) の結線 (#40)。
        // 実測: Gemini が `"ops": "\n"` 等を出した時、従来はターンが丸ごとエラーで蒸発した。
        let delta = match proposer.propose(&messages).await {
            Ok(d) => d,
            Err(HarnessError::Proposer(llm_client::LlmError::Parse { raw, source }))
                if attempt < max_attempts.max(1) =>
            {
                // 原因を提示層へ運ぶ (捨てると「却下された試行:」の空欄になる)。
                retries.push(RetryCause::Malformed { detail: source.to_string() });
                messages.push(ChatMessage::assistant(raw));
                // 文言は**理由を先頭に立てる** (#93)。旧文は「JSON として壊れていて読めなかった」と
                // 断定したうえで「ops は要素が 1 つでも必ず配列」と続けており、実際の失敗が幻の
                // op 名だったとき **構文は正しく ops も既に配列** = LLM が正しくやっていることを
                // 指示して真因に一言も触れない誤誘導になっていた (単一オブジェクトは受け側が
                // 救済する #64 ので、その注意書き自体がもう要らない)。
                messages.push(ChatMessage::user(format!(
                    "あなたの前回の出力はスキーマに従っておらず読み取れなかった。
理由: {source}
この理由を直したうえで、同じ内容を**正しい JSON だけ**で再提出せよ (narration / summary / ops)。理由に op 名の一覧が示されているなら、**使えるのはその名前だけ** — 行動の意図を新しい op 名にしてはいけない (「肉を渡す」なら give_item、「食べる」なら remove_item のように既存の op へ翻訳する)。前置き・注釈・フェンスは不要。"
                )));
                continue;
            }
            Err(e) => return Err(e),
        };

        match adjudicate(state, scenario, &delta) {
            Verdict::Accept => {
                // spec 08-B: 所持品差分の計上用に apply 前の inventory を写す (小さい map)。
                let inv_before = state.inventory.clone();
                // adjudicate が通ったので apply は成功するはず。RNG はここで決定論的に振られ、
                // 適用後に発火した反応ビート (Phase C) も返る。
                let out = apply(state, scenario, &delta)
                    .expect("adjudicate 済みなら apply は成功する");
                let tags = chronicle_tags(state, scenario, &inv_before);
                return Ok(TurnOutcome::Accepted {
                    narration: delta.narration,
                    summary: delta.summary,
                    rolls: out.rolls,
                    checks: out.checks,
                    stat_rolls: out.stat_rolls,
                    fired: out.fired,
                    attempts: attempt,
                    retries,
                    tags,
                });
            }
            Verdict::Reject { reasons } => {
                // 履歴の一貫性のため、LLM が出した提案 (の痕跡) と却下理由を会話に積む。
                push_rejection(&mut messages, &delta, &reasons, lang);
                retries.push(RetryCause::Rejected(reasons.clone()));
                last_reasons = reasons;
            }
        }
    }

    Ok(TurnOutcome::Rejected {
        last_reasons,
        attempts: max_attempts.max(1),
    })
}

/// 却下された提案を assistant 発話として、修正指示を user 発話として積む (self_repair の核)。
/// 却下理由は `lang` でレンダリングして LLM に戻す。
fn push_rejection(
    messages: &mut Vec<ChatMessage>,
    delta: &StateDelta,
    reasons: &[RejectReason],
    lang: Lang,
) {
    // LLM 自身の直前の提案を会話履歴に残す (何を直すかの参照点になる)。
    let echoed = serde_json::to_string(delta)
        .unwrap_or_else(|_| delta.narration.clone());
    messages.push(ChatMessage::assistant(echoed));
    messages.push(ChatMessage::user(prompt::rejection_feedback(reasons, lang)));
}
