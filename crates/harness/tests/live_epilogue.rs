//! spec 11 エピローグ生成の live 実測 (実キーが要るので ignore)。
//!
//! 動機 (2026-09-06 ユーザー報告「エピローグが生成されない。前はそんなことなかった」):
//! `generate_epilogue` は GM の client で素の `generate` を **30 秒** (`EPILOGUE_TIMEOUT_SECS`)
//! で切り、失敗は eprintln のみ (リリースビルドでは見えない)。GM 設定に `LLM_EFFORT` が
//! 残っていると思考が長くなり 30 秒に収まらない、が当初の仮説。ここでは同じ素材・同じ経路で
//! **タイムアウトを掛けずに** 所要時間を測り、effort あり／なしで比べる。
//!
//! 結果 (Meta muse-spark-1.3、各 3 回): effort あり 51.2 / 31.8 / 28.4 秒、なし 28.3 / 24.4 /
//! 47.8 秒 = **effort の有無で差が無く、モデルの素の遅さとばらつきが 30 秒を越えていた**
//! (仮説は棄却)。既定を 120 秒へ (`EPILOGUE_TIMEOUT_SECS`)、実経路は 28 秒で通過。
//!
//! ```text
//! APP_ENV_PATH="$APPDATA/jp.lorekeel.app/.env" cargo test -p harness --test live_epilogue -- --ignored --nocapture
//! ```
//! (`APP_ENV_PATH` 未指定なら repo の `.env`)。

use harness::{build_epilogue_request, epilogue_messages, SynopsisEntry, TurnLog};
use llm_client::{LlmClient, LlmConfig};

fn history() -> Vec<TurnLog> {
    let lines = [
        (1, "洋館の玄関へ", "玄関ホールに入り、埃と泥の足跡を見つけた。呼び鈴に応える者はいない。"),
        (2, "足跡を追う", "足跡は書斎へ続いていた。書斎の机に湖山の日記が開かれたまま残っていた。"),
        (3, "日記を読む", "日記は湖の底の『向こう側』について書かれ、途中から筆致が乱れていた。目星に成功し隠し引き出しを発見。"),
        (4, "引き出しを開ける", "引き出しから螺旋の紋様が刻まれた真鍮の鍵を得た。"),
        (5, "地下へ", "鍵で地下室の扉を開けた。湿った階段の先に祭壇の間がある。SAN を 1 失った。"),
        (6, "祭壇を調べる", "祭壇の像を覗き込み、像を結ばなかった。SAN を 3 失い、手が震え始めた。"),
        (7, "もう一度覗く", "再び像を覗き、湖山が『向こう側』へ渡った瞬間を目撃した。真相を知った。"),
    ];
    lines
        .iter()
        .map(|(t, p, s)| TurnLog {
            turn: *t,
            player: p.to_string(),
            summary: s.to_string(),
            location: if *t >= 5 { "altar".into() } else { "study".into() },
            present: vec![],
            flags_set: if *t == 7 { vec!["truth_witnessed".into()] } else { vec![] },
            checks: vec![],
            items: if *t == 4 { vec!["+真鍮の鍵".into()] } else { vec![] },
        })
        .collect()
}

#[tokio::test]
#[ignore = "実キー (LLM_*) が要る live テスト"]
async fn epilogue_generation_latency_with_and_without_effort() {
    match std::env::var("APP_ENV_PATH") {
        Ok(p) => {
            dotenvy::from_path_override(&p).expect("APP_ENV_PATH が読める");
        }
        Err(_) => {
            dotenvy::dotenv().ok();
        }
    }
    let Ok(cfg) = LlmConfig::from_env() else {
        eprintln!("skip: LLM_* が無い");
        return;
    };
    eprintln!(
        "config: provider={:?} model={} effort={:?} max_tokens={}",
        cfg.provider, cfg.model, cfg.effort, cfg.max_tokens
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/lakeside_manor");
    let loaded = harness::load_package(&root).expect("lakeside_manor が読める");
    let scenario = loaded.scenario;
    let goal = scenario
        .goals
        .iter()
        .find(|g| g.id == "truth_ending")
        .expect("truth_ending がある");
    let synopsis = vec![SynopsisEntry {
        upto_turn: 2,
        title: "ターン 1〜2".into(),
        text: "調査員は湖畔の洋館を訪ね、無人の玄関から書斎へと足跡を追った。机には湖山の日記が残されていた。".into(),
    }];
    let last = "像の奥で、湖山だったものがゆっくりと振り返った。あなたは目を逸らせない。\
                水面の下から、螺旋がこちらを見ている。夜明け前、あなたは洋館を後にした。";
    let req = build_epilogue_request(&scenario, goal, &synopsis, &history(), last);
    let msgs = epilogue_messages(&req);
    let chars: usize = msgs.iter().map(|m| m.content.chars().count()).sum();
    eprintln!("prompt chars={chars}");

    // ① 設定どおり (LLM_EFFORT が在ればそのまま)
    let mut variants = vec![("as configured", cfg.clone())];
    // ② effort を外した対照群
    if cfg.effort.is_some() {
        let mut c = cfg.clone();
        c.effort = None;
        variants.push(("effort=None", c));
    }
    // ③ 実経路 (`generate_epilogue` = 実効タイムアウト込み) が設定どおりで通るか。
    {
        let client = LlmClient::new(cfg.clone()).unwrap();
        let t0 = std::time::Instant::now();
        let res = harness::generate_epilogue(&client, &req).await;
        eprintln!(
            "--- generate_epilogue (timeout {}s): {:.1}s → {} ---",
            harness::epilogue_timeout_secs(),
            t0.elapsed().as_secs_f32(),
            match &res {
                Ok(t) => format!("Ok {} chars", t.chars().count()),
                Err(e) => format!("Err {e}"),
            }
        );
        assert!(res.is_ok(), "実経路で落ちる: {res:?}");
    }
    for (label, c) in variants {
        let client = LlmClient::new(c).unwrap();
        let t0 = std::time::Instant::now();
        let res = client.generate(msgs.clone()).await;
        let secs = t0.elapsed().as_secs_f32();
        match res {
            Ok(text) => {
                let text = llm_client::strip_reasoning_blocks(&text).trim().to_string();
                eprintln!(
                    "--- {label}: {secs:.1}s, {} chars (limit {}s) ---\n{}\n",
                    text.chars().count(),
                    harness::EPILOGUE_TIMEOUT_SECS,
                    text.chars().take(300).collect::<String>()
                );
            }
            Err(e) => eprintln!("--- {label}: {secs:.1}s, ERROR: {e} ---"),
        }
    }
}
