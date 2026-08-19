//! spec 24 Phase D: 挿絵プロンプト書きの live 実測 (実キーが要るので ignore)。
//!
//! 同梱 `packages/lakeside_manor` を読み、開幕の場面 (語り = 場所説明) と「語りあり」の 2 通りで
//! `build_image_prompt_request` → GM 設定の `generate` を 1 回ずつ回し、出てきたプロンプトを
//! 目視する。見るもの: 語りに無い人物が混じらないか / 形式 (prose 1〜3 文 / tags 50 語) /
//! 前置きが付かないか。
//!
//! ```text
//! LLM_BASE_URL=... LLM_API_KEY=... LLM_MODEL=... cargo test -p harness --test live_image_prompt -- --ignored --nocapture
//! ```

use harness::{build_image_prompt_request, image_prompt_messages, ImagePromptStyle};
use llm_client::{LlmClient, LlmConfig};

#[tokio::test]
#[ignore = "実キー (LLM_*) が要る live テスト"]
async fn image_prompt_writer_on_lakeside_manor() {
    dotenvy::dotenv().ok();
    let Ok(cfg) = LlmConfig::from_env() else {
        eprintln!("skip: LLM_* が無い");
        return;
    };
    let client = LlmClient::new(cfg).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/lakeside_manor");
    let loaded = harness::load_package(&root).expect("lakeside_manor が読める");
    let scenario = loaded.scenario;
    let state = scenario.initial_state(7);

    let narration = "玄関ホールに足を踏み入れると、埃をかぶったシャンデリアが薄暗い天井からぶら下がっている。\
                     呼び鈴を鳴らしても応える者はない——扉は最初から開いていた。石造りの床には、誰かの足跡が\
                     乾いた泥のまま残っている。";
    for (label, style, text) in [
        ("prose / 開幕 (語り無し)", ImagePromptStyle::Prose, ""),
        ("prose / 語りあり", ImagePromptStyle::Prose, narration),
        ("tags / 語りあり", ImagePromptStyle::Tags, narration),
    ] {
        let req = build_image_prompt_request(&scenario, &state, text, "anime style", style);
        let t0 = std::time::Instant::now();
        let out = client.generate(image_prompt_messages(&req)).await.expect("generate が通る");
        let out = llm_client::strip_reasoning_blocks(&out).trim().to_string();
        eprintln!("--- {label} ({:.1}s) ---\n{out}\n", t0.elapsed().as_secs_f32());
        assert!(!out.is_empty());
    }
}
