//! spec 33 P0 — 直前の語りを「可変 user の逐語」から「会話履歴」へ移したとき
//! `cache_read` がどう動くかの live 実測 (実キーが要るので ignore)。
//!
//! 実セーブから **連続する 2 ターン (t1 → t2)** の messages を 3 形で組み、同じ client
//! (= 同じ conv id) で順に投げて各リクエストの `cache_read` を読む。
//!
//! - `A`       : 現行形。語りは可変 user の中 (`recent_narrations_note`)
//! - `B-slide` : 本案形。直前 K ターンを user(行動)/assistant(語り) の組で並べる。
//!   窓は K 固定で 1 ターンずつ滑る (= 現行の `LOREKEEL_RECENT_TURNS` の意味論のまま)
//! - `B-grow`  : 本案形だが窓の先頭を固定し末尾だけ伸ばす (append-only)。滑る窓と比べて
//!   「履歴の存在」と「プレフィックスの延伸」を切り分ける対照
//!
//! state は両ターンともセーブ時点のものを使う (t1 の真の state は残っていない)。可変 user は
//! どの形でも毎ターン変わるので、比べたいプレフィックス (system と履歴) には影響しない。
//!
//! ```text
//! P0_SAVE=<save.yaml> P0_BASE_URL=... P0_MODEL=... P0_API_KEY=... \
//!   cargo test -p harness --test live_spec33_p0 -- --ignored --nocapture
//! ```
//!
//! 結果 (2026-09-23、詳細は failures #107 / specs/33「P0 の結果」):
//! - **OpenAI gpt-5.6-terra** (turn 319 / 354 の 2 セーブで同形): t2 が前ターンから読めるのは
//!   3 形とも **system 部分だけ** (11,141 / 12,113)。B-grow でも共通の履歴部分まで延びない
//! - **Meta muse-spark** (2 周): 数秒後の同一再送は 113 (読めない)、約 10 分後の同一リクエストは
//!   ほぼ全量読める。プレフィックスの途中まで読む (B-grow t2 = 13,169 = system + 共通 3 ターン)
//! - ⇒ 窓が滑る spec 33 の設計どおりでは、どのプロバイダでも現行と差が出ない

use gm_core::{GameState, StateDelta};
use harness::prompt;
use harness::{load_package, load_session, run_turn, DeltaProposer, HarnessError, SavedContent, TurnLog};
use llm_client::{ChatMessage, LlmClient, LlmConfig, Role};
use std::path::Path;
use std::sync::Mutex;

/// messages を記録して空デルタを返す (run_turn に messages を組ませるためだけの proposer)。
struct Capture(Mutex<Option<Vec<ChatMessage>>>);

impl DeltaProposer for Capture {
    async fn propose(&self, messages: &[ChatMessage]) -> Result<StateDelta, HarnessError> {
        let mut g = self.0.lock().unwrap();
        if g.is_none() {
            *g = Some(messages.to_vec());
        }
        Ok(StateDelta::new("", vec![]))
    }
}

struct Ctx {
    save: harness::SessionSave,
    scenario: gm_core::Scenario,
}

/// 現行形の messages (run_turn がそのまま組むもの)。
async fn build_current(ctx: &Ctx, action: &str, narrs: &[String], history: &[TurnLog]) -> Vec<ChatMessage> {
    let cap = Capture(Mutex::new(None));
    let mut state: GameState = ctx.save.state.clone();
    let _ = run_turn(
        &cap,
        &mut state,
        &ctx.scenario,
        action,
        1,
        gm_core::Lang::Ja,
        &ctx.save.pending_lore,
        &ctx.save.pending_checks,
        narrs,
        history,
        &ctx.save.synopsis.entries,
        &ctx.save.facts,
        &[],
    )
    .await;
    let m = cap.0.lock().unwrap().take();
    m.expect("run_turn が proposer を呼ぶ")
}

/// 本案形: 現行形の最後の user から語りの逐語を抜き、規律だけ残し、
/// その前に user(行動)/assistant(語り) の組を並べる。
fn to_history_form(current: Vec<ChatMessage>, narrs: &[String], actions: &[String]) -> Vec<ChatMessage> {
    assert_eq!(narrs.len(), actions.len());
    let mut msgs = current;
    let last = msgs.pop().expect("user がある");
    assert!(matches!(last.role, Role::User));
    let note = prompt::recent_narrations_note(narrs);
    assert!(!note.is_empty() && last.content.contains(&note), "語りの節が user に在る");
    let rule = "\n\n# 直前までの語り（情景はここから継続する。繰り返さないこと）\n\
        直前のターンであなたが語った内容は、この上の会話履歴にある。\
        **既に確立した静的な情景（時刻・天候・部屋の様子・既に済んだ登場・挨拶・相手の初対面の驚きなど）を再び描写しないこと**。\
        同じ説明を二度せず、この続きとして「変化・反応・新しい展開」だけを描いてください。\
        台詞・約束・描いた小物など、履歴に書かれた細部は確定した出来事として扱い、食い違う語りをしないこと。\n";
    let user = last.content.replacen(&note, rule, 1);
    for (a, n) in actions.iter().zip(narrs) {
        msgs.push(ChatMessage::user(format!("# プレイヤーの行動\n{a}")));
        msgs.push(ChatMessage::assistant(n.clone()));
    }
    msgs.push(ChatMessage::user(user));
    msgs
}

fn chars(msgs: &[ChatMessage]) -> usize {
    msgs.iter().map(|m| m.content.chars().count()).sum()
}

#[tokio::test]
#[ignore = "実キーが要る live テスト"]
async fn spec33_p0_cache_read_current_vs_history_form() {
    let (Ok(save_path), Ok(base), Ok(model), Ok(key)) = (
        std::env::var("P0_SAVE"),
        std::env::var("P0_BASE_URL"),
        std::env::var("P0_MODEL"),
        std::env::var("P0_API_KEY"),
    ) else {
        eprintln!("skip: P0_* が無い");
        return;
    };
    let k: usize = std::env::var("P0_K").ok().and_then(|s| s.parse().ok()).unwrap_or(3);

    let save = load_session(Path::new(&save_path)).expect("セーブが読める");
    let SavedContent::Package { path } = &save.content else { panic!("package セーブのみ対応") };
    let pkg = load_package(Path::new(path)).expect("パッケージが読める");
    let ctx = Ctx { save, scenario: pkg.scenario };

    let recent = ctx.save.recent_narrations.clone();
    let hist = ctx.save.history.clone();
    let n = recent.len();
    assert!(n > k, "語りが K+1 本以上要る (n={n})");
    // recent の末尾 = history の末尾ターンの語り、と対応させる。
    let acts: Vec<String> = hist[hist.len() - n..].iter().map(|t| t.player.clone()).collect();
    let next_action = "少し間を置いて、周りの様子をうかがう".to_string();

    // t1: 1 ターン前の視点 (語り recent[..n-1]、行動 acts[n-1] を今まさに打った)
    // t2: 現在 (語り recent[..n]、次の行動)
    let t1_hist = &hist[..hist.len() - 1];
    let slide = |end: usize| (end - k)..end;
    let (s1, s2) = (slide(n - 1), slide(n));
    let (g1, g2) = ((n - 1 - k)..(n - 1), (n - 1 - k)..n);

    let a1 = build_current(&ctx, &acts[n - 1], &recent[s1.clone()], t1_hist).await;
    let a2 = build_current(&ctx, &next_action, &recent[s2.clone()], &hist).await;
    let bs1 = to_history_form(a1.clone(), &recent[s1.clone()], &acts[s1.clone()]);
    let bs2 = to_history_form(a2.clone(), &recent[s2.clone()], &acts[s2.clone()]);
    let a1g = build_current(&ctx, &acts[n - 1], &recent[g1.clone()], t1_hist).await;
    let a2g = build_current(&ctx, &next_action, &recent[g2.clone()], &hist).await;
    let bg1 = to_history_form(a1g, &recent[g1.clone()], &acts[g1.clone()]);
    let bg2 = to_history_form(a2g, &recent[g2.clone()], &acts[g2.clone()]);

    let mut cfg = LlmConfig::new(base, key, model.clone());
    cfg.max_tokens = std::env::var("P0_MAX_TOKENS").ok().and_then(|s| s.parse().ok()).unwrap_or(cfg.max_tokens);
    let client = LlmClient::new(cfg).expect("client");
    eprintln!("model={model} save_turn={} K={k} recent={n}", ctx.save.state.turn);

    let order: Vec<(&str, &Vec<ChatMessage>)> = vec![
        ("A t1", &a1),
        ("A t2", &a2),
        ("A t2 (再送)", &a2),
        ("B-slide t1", &bs1),
        ("B-slide t2", &bs2),
        ("B-slide t2 (再送)", &bs2),
        ("B-grow t1", &bg1),
        ("B-grow t2", &bg2),
        ("B-grow t2 (再送)", &bg2),
    ];
    for (label, msgs) in order {
        let before = client.cache_stat().total_requests;
        let res = client.propose(msgs).await;
        let st = client.cache_stat();
        let ok = if st.total_requests > before { "" } else { " (usage 未記録)" };
        let err = res.err().map(|e| format!(" err={}", e.to_string().chars().take(80).collect::<String>())).unwrap_or_default();
        eprintln!(
            "[P0] {label:<18} msgs={:>2} chars={:>6} prompt={:>6} cache_read={:>6}{ok}{err}",
            msgs.len(),
            chars(msgs),
            st.last_prompt,
            st.last_cache_read
        );
    }
}
