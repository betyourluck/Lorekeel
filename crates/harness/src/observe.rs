//! spec 35 Phase E の計器 — `play --observe <path.jsonl>`。
//!
//! 台本プレイの 1 ターンごとに、GM の提案がどの形で返ったか (段の配列 / 文字列 / 壊れた出力)・
//! 何で却下されたか・受理された語りの段の内訳を jsonl に 1 行書く。
//!
//! **本番のループは変えない**: `LlmClient` を [`Recorder`] で包んで `run_turn` に渡すだけ
//! (`DeltaProposer` の依存性逆転)。語りの形 (`Narration::Segments` / `Raw`) は話者の解決の後には
//! 残らない (セーブにも `TurnOutcome` にも無い) ので、提案を受け取った瞬間に記録するしかない。
//!
//! 出力は測定用で、本文は「」/ `“”` を含む地の文の段だけを載せる (③の手作業分類の素材)。
//! app のログ (本文を書かない規則) とは別物。

use std::sync::Mutex;

use gm_core::{RejectReason, Speaker};
use harness::{DeltaProposer, HarnessError, RetryCause, TurnOutcome};
use llm_client::ChatMessage;
use serde::Serialize;

/// 提案 1 回の形。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum Proposal {
    /// 段の配列 (単一オブジェクトの救済もここ)。`speakers` は話者の欄の生の文字列 (地の文は数えない)。
    Segments { count: usize, speakers: Vec<String> },
    /// 文字列で返った (gm_core の救済で段に分けられる)。
    Raw { chars: usize },
    /// 構造化出力が壊れて読めなかった (`LlmError::Parse`)。
    Malformed,
    /// それ以外の提案者の失敗 (接続など)。
    Error { detail: String },
}

impl Proposal {
    fn of(r: &Result<gm_core::StateDelta, HarnessError>) -> Self {
        match r {
            Ok(d) => match &d.narration {
                gm_core::Narration::Segments(segs) => Proposal::Segments {
                    count: segs.len(),
                    speakers: segs.iter().filter_map(|s| s.speaker.clone()).collect(),
                },
                gm_core::Narration::Raw(text) => Proposal::Raw { chars: text.chars().count() },
            },
            Err(HarnessError::Proposer(llm_client::LlmError::Parse { .. })) => Proposal::Malformed,
            Err(e) => Proposal::Error { detail: e.to_string() },
        }
    }
}

/// 提案者を包み、提案の形を記録する。`enabled` が偽なら何も記録しない (= `--observe` 無しの経路)。
pub(crate) struct Recorder<'a, P> {
    inner: &'a P,
    log: Option<Mutex<Vec<Proposal>>>,
}

impl<'a, P> Recorder<'a, P> {
    pub(crate) fn new(inner: &'a P, enabled: bool) -> Self {
        Self { inner, log: enabled.then(|| Mutex::new(Vec::new())) }
    }

    pub(crate) fn into_proposals(self) -> Vec<Proposal> {
        self.log.map(|m| m.into_inner().unwrap_or_default()).unwrap_or_default()
    }
}

impl<P: DeltaProposer> DeltaProposer for Recorder<'_, P> {
    async fn propose(&self, messages: &[ChatMessage]) -> Result<gm_core::StateDelta, HarnessError> {
        let r = self.inner.propose(messages).await;
        if let Some(log) = &self.log {
            if let Ok(mut g) = log.lock() {
                g.push(Proposal::of(&r));
            }
        }
        r
    }
}

/// やり直し 1 回の原因 (`RetryCause` の jsonl 形)。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RetryRecord {
    Rejected { reasons: Vec<RejectReason> },
    Malformed { detail: String },
}

/// 受理された語りの段の内訳。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct SegCounts {
    /// 地の文。
    pub narr: usize,
    /// 登場人物か主人公の台詞。
    pub dlg: usize,
    /// 名もない人物の台詞。
    pub extra: usize,
}

/// jsonl の 1 行 (1 ターン)。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct TurnRecord {
    pub model: String,
    pub package: String,
    /// 台本の何行目の行動か (1 始まり、空行は数えない)。
    pub index: usize,
    /// ターン後の `GameState.turn` (却下・失敗なら変わらない)。
    pub turn: u32,
    /// 台本の行頭の `[P1]` のような印。誘いの行動を自然な行動と分けて集計するため。
    pub tag: Option<String>,
    pub accepted: bool,
    pub attempts: u32,
    pub proposals: Vec<Proposal>,
    pub retries: Vec<RetryRecord>,
    /// 試行上限で失敗したときの最後の却下理由。
    pub last_reasons: Vec<RejectReason>,
    /// 提案者の失敗でターンが落ちたときの理由。
    pub error: Option<String>,
    pub segs: SegCounts,
    /// 「」/『』/`“”` を含む地の文の段の本文。
    pub quote_lines: Vec<String>,
    pub moved: bool,
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// ターンの外から渡す値 (モデル名・台本の位置・移動・usage の差分)。
pub(crate) struct TurnMeta<'a> {
    pub model: &'a str,
    pub package: &'a str,
    pub index: usize,
    pub tag: Option<String>,
    pub turn: u32,
    pub moved: bool,
    pub usage: (u64, u64, u64),
}

/// 1 ターンの記録を組む (純関数)。
pub(crate) fn turn_record(
    meta: TurnMeta<'_>,
    outcome: &Result<TurnOutcome, HarnessError>,
    proposals: Vec<Proposal>,
) -> TurnRecord {
    let mut r = TurnRecord {
        model: meta.model.to_string(),
        package: meta.package.to_string(),
        index: meta.index,
        turn: meta.turn,
        tag: meta.tag,
        accepted: false,
        attempts: 0,
        proposals,
        retries: Vec::new(),
        last_reasons: Vec::new(),
        error: None,
        segs: SegCounts::default(),
        quote_lines: Vec::new(),
        moved: meta.moved,
        requests: meta.usage.0,
        prompt_tokens: meta.usage.1,
        completion_tokens: meta.usage.2,
    };
    match outcome {
        Ok(TurnOutcome::Accepted { segments, attempts, retries, .. }) => {
            r.accepted = true;
            r.attempts = *attempts;
            r.retries = retries
                .iter()
                .map(|c| match c {
                    RetryCause::Rejected(reasons) => RetryRecord::Rejected { reasons: reasons.clone() },
                    RetryCause::Malformed { detail } => {
                        RetryRecord::Malformed { detail: detail.chars().take(300).collect() }
                    }
                })
                .collect();
            for seg in segments {
                match &seg.speaker {
                    None => {
                        r.segs.narr += 1;
                        if seg.text.contains(['「', '『', '“']) {
                            r.quote_lines.push(seg.text.clone());
                        }
                    }
                    Some(Speaker::Entity(_)) => r.segs.dlg += 1,
                    Some(Speaker::Extra(_)) => r.segs.extra += 1,
                }
            }
        }
        Ok(TurnOutcome::Rejected { last_reasons, attempts }) => {
            r.attempts = *attempts;
            r.last_reasons = last_reasons.clone();
        }
        Err(e) => r.error = Some(e.to_string()),
    }
    r
}

/// 台本の行頭の `[TAG]` を取り出す。TAG は ASCII の英数字・`_`・`-` で 1〜16 字。
/// 印の後に行動が無ければ印として扱わない (行をそのまま行動にする)。
pub(crate) fn split_tag(line: &str) -> (Option<String>, &str) {
    let Some(body) = line.strip_prefix('[') else { return (None, line) };
    let Some(end) = body.find(']') else { return (None, line) };
    let tag = &body[..end];
    let rest = body[end + 1..].trim_start();
    let ok = (1..=16).contains(&tag.len())
        && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !ok || rest.is_empty() {
        return (None, line);
    }
    (Some(tag.to_string()), rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gm_core::{Lang, Scenario, Segment, StateDelta};
    use std::collections::VecDeque;

    /// 台本どおりに結果を返す fake 提案者。
    struct Fake(Mutex<VecDeque<Result<StateDelta, HarnessError>>>);
    impl DeltaProposer for Fake {
        async fn propose(&self, _m: &[ChatMessage]) -> Result<StateDelta, HarnessError> {
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(HarnessError::NoProposal("台本が尽きた".into())))
        }
    }

    fn spoken(lines: &[(&str, &str)]) -> StateDelta {
        let segs = lines
            .iter()
            .map(|(sp, t)| Segment { speaker: (!sp.is_empty()).then(|| sp.to_string()), text: t.to_string() })
            .collect::<Vec<_>>();
        StateDelta::new(segs, vec![])
    }

    fn parse_error() -> HarnessError {
        HarnessError::Proposer(llm_client::LlmError::Parse {
            raw: "壊れた出力".into(),
            source: serde_json::from_str::<StateDelta>("not json").unwrap_err(),
        })
    }

    /// 【①の素】提案の形を 1 回ごとに記録する — 段の配列・文字列・壊れた出力・接続の失敗を区別し、
    /// 話者の欄は生の文字列で残す (誤った却下の仕分けに使う)。`--observe` 無しなら何も記録しない。
    #[tokio::test]
    async fn recorder_records_the_form_of_each_proposal() {
        let fake = Fake(Mutex::new(VecDeque::from(vec![
            Ok(spoken(&[("", "教室。"), ("あかり", "やあ")])),
            Ok(StateDelta::new("凛花「遅い」", vec![])),
            Err(parse_error()),
            Err(HarnessError::NoProposal("切れた".into())),
        ])));
        let rec = Recorder::new(&fake, true);
        for _ in 0..4 {
            let _ = rec.propose(&[]).await;
        }
        assert_eq!(
            rec.into_proposals(),
            vec![
                Proposal::Segments { count: 2, speakers: vec!["あかり".into()] },
                Proposal::Raw { chars: 6 },
                Proposal::Malformed,
                Proposal::Error { detail: "提案が得られない: 切れた".into() },
            ]
        );

        let fake = Fake(Mutex::new(VecDeque::from(vec![Ok(spoken(&[("", "x")]))])));
        let rec = Recorder::new(&fake, false);
        assert!(rec.propose(&[]).await.is_ok(), "包んでも結果はそのまま返る");
        assert!(rec.into_proposals().is_empty(), "無効なら記録しない");
    }

    fn scenario() -> Scenario {
        Scenario::from_yaml(
            r#"
title: t
start: cafe
goal: { kind: always }
characters:
  rinka: { name: 凛花 }
  alice: { name: アリス }
locations:
  cafe: { description: 喫茶店, present: [rinka], exits: [] }
"#,
        )
        .unwrap()
    }

    /// 【②③④の素】run_turn を本物のまま通し、却下の理由・段の内訳・「」を含む地の文を 1 行にまとめる。
    /// 1 回目は不在のアリスの台詞で却下、2 回目は地の文 (店名の「」)・台詞・名もない人物で受理。
    #[tokio::test]
    async fn turn_record_carries_retries_segments_and_quote_lines() {
        let sc = scenario();
        let mut s = sc.initial_state(1);
        let fake = Fake(Mutex::new(VecDeque::from(vec![
            Ok(spoken(&[("alice", "やあ")])),
            Ok(spoken(&[("", "喫茶「夕凪」の看板が鳴る。"), ("rinka", "遅いわね"), ("店員", "いらっしゃい")])),
        ])));
        let rec = Recorder::new(&fake, true);
        let outcome =
            harness::run_turn(&rec, &mut s, &sc, "入る", 3, Lang::Ja, &[], &[], &[], &[], &[], &[], &[]).await;
        let meta = TurnMeta {
            model: "m",
            package: "p",
            index: 1,
            tag: Some("P1".into()),
            turn: s.turn,
            moved: false,
            usage: (2, 100, 10),
        };
        let r = turn_record(meta, &outcome, rec.into_proposals());
        assert!(r.accepted);
        assert_eq!(r.attempts, 2);
        assert_eq!(r.proposals.len(), 2);
        assert_eq!(r.segs, SegCounts { narr: 1, dlg: 1, extra: 1 });
        assert_eq!(r.quote_lines, vec!["喫茶「夕凪」の看板が鳴る。".to_string()]);
        let RetryRecord::Rejected { reasons } = &r.retries[0] else { panic!("却下のはず: {:?}", r.retries) };
        assert!(matches!(reasons[0], RejectReason::SpeakerNotPresent { .. }), "{reasons:?}");
        assert_eq!((r.requests, r.prompt_tokens, r.completion_tokens), (2, 100, 10));

        let line = serde_json::to_string(&r).unwrap();
        assert!(!line.contains('\n'), "jsonl は 1 行");
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["retries"][0]["kind"], "rejected");
        assert_eq!(v["retries"][0]["reasons"][0]["code"], "speaker_not_present");
        assert_eq!(v["proposals"][0]["form"], "segments");
        assert_eq!(v["tag"], "P1");
    }

    /// 【台本の印】行頭の `[P1]` だけを印として取り、行動からは外す。印の形でないものは行動のまま。
    #[test]
    fn split_tag_takes_only_a_bracketed_ascii_prefix() {
        assert_eq!(split_tag("[P1] 源蔵さんはどう思う？"), (Some("P1".into()), "源蔵さんはどう思う？"));
        assert_eq!(split_tag("[probe-2]入る"), (Some("probe-2".into()), "入る"));
        assert_eq!(split_tag("源蔵さんはどう思う？"), (None, "源蔵さんはどう思う？"));
        assert_eq!(split_tag("[気持ち] 黙る"), (None, "[気持ち] 黙る"), "ASCII でない印は印ではない");
        assert_eq!(split_tag("[P1]"), (None, "[P1]"), "行動が無ければ印として扱わない");
        assert_eq!(split_tag("[P1 入る"), (None, "[P1 入る"));
    }
}
