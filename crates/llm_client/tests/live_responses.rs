//! Perplexity Agent API (`/v1/responses`) の live smoke テスト (2026-08-20)。
//!
//! 実キーが要るので **既定は ignore**。鍵は `PERPLEXITY_API_KEY` (公式文書の名) または
//! `PPLX_API_KEY` から読む — 値はログに出さない。
//!
//! ```text
//! cargo test -p llm_client --test live_responses -- --ignored --nocapture
//! ```
//!
//! 見るもの: ①`generate_structured` が `required` で function_call を取り `StateDelta` に
//! 解決する (ops が配列) ②`generate` (ツール無し) が本文を返す ③HTTP 200 (401 は鍵、
//! 429 はレート)。

use gm_core::StateDelta;
use llm_client::{ChatMessage, LlmClient, LlmConfig, Provider, EMIT_DELTA_TOOL};

fn key() -> Option<String> {
    std::env::var("PERPLEXITY_API_KEY")
        .or_else(|_| std::env::var("PPLX_API_KEY"))
        .ok()
        .filter(|k| !k.trim().is_empty())
}

#[tokio::test]
#[ignore = "実キー (PERPLEXITY_API_KEY / PPLX_API_KEY) が要る live テスト"]
async fn perplexity_responses_round_trip() {
    let Some(key) = key() else {
        eprintln!("skip: PERPLEXITY_API_KEY / PPLX_API_KEY が無い");
        return;
    };
    let cfg = LlmConfig::new(
        "https://api.perplexity.ai/v1",
        key,
        "perplexity/deepseek-v4-flash-0731",
    );
    assert_eq!(cfg.provider, Provider::Responses, "ホストから自動判定");
    let client = LlmClient::new(cfg).unwrap();

    // ① 構造化出力 (実 schema・required)。
    let delta: StateDelta = client
        .generate_structured(
            vec![
                ChatMessage::system(
                    "あなたは TRPG の GM です。語り (narration) と状態変更 (ops) を emit_delta で提出してください。\
                     現在地: cell。出口: cell -> corridor (door_open フラグが必要)。所持品: 鍵。",
                ),
                ChatMessage::user("プレイヤー: 鍵で扉を開けて廊下へ出る"),
            ],
            EMIT_DELTA_TOOL,
            "ターンの語りと状態変更を提出する",
            llm_client::state_delta_schema(),
        )
        .await
        .expect("generate_structured が通る");
    eprintln!(
        "structured: narration_chars={} ops={}",
        delta.narration.chars().count(),
        delta.ops.len()
    );
    assert!(!delta.narration.trim().is_empty());

    // ② プレーン生成 (あらすじ要約の経路)。
    let text = client
        .generate(vec![
            ChatMessage::system("次の出来事を 1 文に要約してください。"),
            ChatMessage::user("主人公は鍵で扉を開け、廊下へ出た。"),
        ])
        .await
        .expect("generate が通る");
    eprintln!("plain: chars={}", text.chars().count());
    assert!(!text.trim().is_empty());

    let stat = client.cache_stat();
    eprintln!("cache_stat: requests={} last_cache_read={}", stat.total_requests, stat.last_cache_read);
    assert_eq!(stat.total_requests, 2, "成功 1 回 = 記録 1 回");
}
