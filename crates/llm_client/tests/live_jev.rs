//! Jev の実ワイヤ検証 (`--ignored`)。
//!
//! `JEV_ACCOUNT_ID` / `JEV_API_TOKEN` を設定して
//! `cargo test -p llm_client --test live_jev -- --ignored --nocapture` で回す。
//!
//! **なぜ live が要るか**: 2026-09-06 に spec 29 で踏んだとおり、adapter の単体テストは
//! wire の「形」しか固定できず、相手の癖 (schema の一部を 400 で拒む / 履歴に余計な欄を
//! 要求する) を捕まえない。しかもここは **Cloudflare 経由という新しい経路**で、実測は
//! すべて PowerShell の使い捨てスクリプトで行った — それは「この実装が通る」証明ではない。

use std::collections::BTreeMap;

use llm_client::jev::{JevClient, NoulQuestion};

fn client() -> Option<JevClient> {
    let c = JevClient::from_env();
    if c.is_none() {
        eprintln!("skip: JEV_ACCOUNT_ID / JEV_API_TOKEN が未設定");
    }
    c
}

/// 実ワイヤで往復し、**日本語の質問が通ること**と**確度が期待の側へ振れること**を見る。
///
/// state は Lorekeel の実盤面 (friday_lemmon) と同じ構造にしてある。閾値は 0.5 でなく
/// 余裕をもって 0.3 / 0.7 で見る — このテストが測りたいのは較正の精度ではなく
/// 「実装が実際に通る」ことなので、モデル更新でわずかに動いても落とさない。
#[tokio::test]
#[ignore]
async fn live_noul_round_trip() {
    let Some(client) = client() else { return };

    let state = serde_json::json!({
        "present": ["player", "akari"],
        "player_inventory": ["メモ帳"],
        "npc_inventory": { "fudemura": ["万年筆", "文庫本"] },
        "character_names": { "player": "あなた(主人公)", "akari": "岬あかり", "genzo": "岬源蔵" },
        "narration": "私はポケットから万年筆を取り出し、あかりに手渡した。\
そのとき、カウンターの奥から源蔵が顔を出した。「あかり、そこで何をしとる」"
    });

    let mut questions = BTreeMap::new();
    questions.insert(
        "absent_speaker".to_string(),
        NoulQuestion::new(
            "`narration` で台詞を話した人物のうち、`present` に含まれない者がいるか。\
`character_names` は id と表示名の対応表であり、主人公 player は「私」として登場する。\n話題に出るだけ・名前が出るだけで台詞を話していない人物は数えない。",
        )
        .with_criteria(
            "present に無い人物が台詞を話している",
            "台詞を話したのは present に含まれる人物だけである",
        ),
    );
    questions.insert(
        "handed_unowned".to_string(),
        NoulQuestion::new(
            "`narration` で主人公が手渡した物は、`player_inventory` に含まれない物か。",
        )
        .with_criteria(
            "player_inventory に無い物を手渡した",
            "手渡した物は player_inventory にある、または何も手渡していない",
        ),
    );
    questions.insert(
        "moved".to_string(),
        NoulQuestion::new("`narration` のなかで、主人公が別の場所へ移動し終えたか。")
            .with_criteria("主人公が別の場所へ移動し終えた", "主人公はまだ同じ場所にいる"),
    );

    let started = std::time::Instant::now();
    let got = client.ask(&state, &questions).await.expect("Jev の呼び出し");
    let elapsed = started.elapsed();

    let absent = got.get("absent_speaker").expect("absent_speaker の答え");
    let handed = got.get("handed_unowned").expect("handed_unowned の答え");
    let moved = got.get("moved").expect("moved の答え");
    eprintln!(
        "[live_jev] model={} elapsed={:?} absent={absent} handed={handed} moved={moved} \
usage(in/out)={}/{}",
        got.model, elapsed, got.usage.prompt, got.usage.completion
    );

    // 源蔵は present に無いのに発話している / 万年筆は player の持ち物ではない。
    assert!(absent > 0.7, "不在者の発話を検出できていない: {absent}");
    assert!(handed > 0.7, "未所持物の譲渡を検出できていない: {handed}");
    // 対照: 移動していないので低いままであること (全部 true に倒れる器ではない)。
    assert!(moved < 0.3, "移動していないのに陽性になった: {moved}");

    assert!(got.usage.prompt > 0, "usage が載っていない");
    assert!(!got.model.is_empty(), "モデル版が空");
}

/// 正常な語りで**陽性を出さない**こと (偽陽性の live 確認)。
/// 却下ループを生むのは偽陽性の側なので、こちらのほうが重要。
#[tokio::test]
#[ignore]
async fn live_clean_narration_stays_low() {
    let Some(client) = client() else { return };

    let state = serde_json::json!({
        "present": ["player", "akari"],
        "player_inventory": ["メモ帳"],
        "character_names": { "player": "あなた(主人公)", "akari": "岬あかり" },
        "narration": "商店街の喧騒のなか、岬あかりは落ち着かない様子で袖口をいじっていた。\
私はメモ帳を取り出し、彼女の言葉を書き留める。「あの店のことで、聞いてほしいことがあって」\
彼女はそう言って口ごもった。"
    });

    let mut questions = BTreeMap::new();
    questions.insert(
        "absent_speaker".to_string(),
        NoulQuestion::new(
            "`narration` で台詞を話した人物のうち、`present` に含まれない者がいるか。\
主人公 player は「私」として登場する。",
        )
        .with_criteria(
            "present に無い人物が台詞を話している",
            "台詞を話したのは present に含まれる人物だけである",
        ),
    );
    questions.insert(
        "handed_unowned".to_string(),
        NoulQuestion::new(
            "`narration` で主人公が手渡した物は、`player_inventory` に含まれない物か。",
        )
        .with_criteria(
            "player_inventory に無い物を手渡した",
            "手渡した物は player_inventory にある、または何も手渡していない",
        ),
    );

    let got = client.ask(&state, &questions).await.expect("Jev の呼び出し");
    let absent = got.get("absent_speaker").expect("答え");
    let handed = got.get("handed_unowned").expect("答え");
    eprintln!("[live_jev] clean: absent={absent} handed={handed}");

    assert!(absent < 0.4, "正常な語りで不在発話が陽性: {absent}");
    assert!(handed < 0.4, "正常な語りで未所持譲渡が陽性: {handed}");
}
