//! AI 編集 (spec 29) のツール往復を Anthropic の実 API に通す live テスト (2026-10-05)。
//!
//! spec 29 Phase E は Anthropic の鍵が無く Grok で代用したので、Anthropic 経路の編集ループは
//! 実 API を一度も通っていなかった。2026-10-05 に直した 2 つの形を、**ループを 20 周回さずに**
//! 直接組んだ履歴で確かめる:
//! - 1 周に 2 本呼んだ結果 + 直後の通知 = 1 つの user メッセージにまとまる (連続 user の解消)
//! - まとめの周 = 同じ tools + `tool_choice: none` (道具を見せたまま呼ばせない)
//!
//! 実キーが要るので **既定は ignore**。鍵は `ANTHROPIC_API_KEY`、モデルは
//! `LIVE_ANTHROPIC_MODEL` (既定 `claude-opus-5-5`)。値はログに出さない。
//!
//! ```text
//! cargo test -p llm_client --test live_anthropic_edit -- --ignored --nocapture
//! ```

use llm_client::{ChatMessage, LlmClient, LlmConfig, Provider, ToolCall, ToolSpec};
use serde_json::json;

fn key() -> Option<String> {
    std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.trim().is_empty())
}

fn tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "read".into(),
            description: "パッケージ内のファイルを行番号つきで読む".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
        },
        ToolSpec {
            name: "grep".into(),
            description: "パッケージ内を正規表現で検索する".into(),
            parameters: json!({"type":"object","properties":{"pattern":{"type":"string"}},"required":["pattern"],"additionalProperties":false}),
        },
    ]
}

/// 1 周に 2 本呼び、結果 2 本の直後に上限の通知を積んだ履歴 (編集ループのまとめの周の直前と同じ形)。
fn history() -> Vec<ChatMessage> {
    vec![
        ChatMessage::system("あなたは YAML ファイルの編集者です。道具で調べてから答えます。"),
        ChatMessage::user("package.yaml の主人公の HP を確かめて"),
        ChatMessage::assistant_tool_calls(
            "",
            vec![
                ToolCall { id: "toolu_live_a".into(), name: "read".into(), args: json!({"path": "package.yaml"}), thought_signature: None },
                ToolCall { id: "toolu_live_b".into(), name: "grep".into(), args: json!({"pattern": "HP"}), thought_signature: None },
            ],
        ),
        ChatMessage::tool_result("toolu_live_a", "read", "1: title: 湖畔の洋館\n2: player:\n3:   stats: { HP: 10, SAN: 60 }"),
        ChatMessage::tool_result("toolu_live_b", "grep", "package.yaml:3:   stats: { HP: 10, SAN: 60 }"),
        ChatMessage::user("道具の呼び出しが上限に達しました。分かったことを 1 行で報告してください。"),
    ]
}

#[tokio::test]
#[ignore = "実キー (ANTHROPIC_API_KEY) が要る live テスト"]
async fn anthropic_accepts_merged_results_and_wrap_up_round() {
    let Some(key) = key() else {
        eprintln!("skip: ANTHROPIC_API_KEY が無い");
        return;
    };
    let model = std::env::var("LIVE_ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-opus-5-5".into());
    let cfg = LlmConfig::new("https://api.anthropic.com/v1", key, model.clone());
    assert_eq!(cfg.provider, Provider::Anthropic, "ホストから自動判定");
    let client = LlmClient::new(cfg).unwrap();

    // まとめの周: 同じ tools + none。呼び出しは返らず、本文で報告が返る。
    let turn = client.chat_wrap_up(history(), tools()).await.expect("まとめの周が通る");
    eprintln!("[{model}] wrap_up: finish={:?} text={:?}", turn.finish, turn.text);
    assert!(turn.tool_calls.is_empty(), "none なので呼ばない: {:?}", turn.tool_calls);
    assert!(turn.text.as_deref().is_some_and(|t| t.contains("10")), "結果を読んで答える: {:?}", turn.text);

    // 通常の周 (auto) でも同じ履歴の形が通る。
    let turn = client.chat(history(), tools()).await.expect("auto の周が通る");
    eprintln!("[{model}] auto: finish={:?} calls={} text={:?}", turn.finish, turn.tool_calls.len(), turn.text);
}
