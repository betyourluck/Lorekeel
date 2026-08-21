//! # llm_client — クラウド LLM ナレーター脚
//!
//! 三権分立の「**LLM=提案**」脚。OpenAI 互換 chat/completions を `base_url`+`api_key` で抽象化し、
//! **tool-use 強制**で gm_core の [`StateDelta`](gm_core::StateDelta) を構造化出力させる。
//!
//! 正本 (gm_core) は LLM 非依存のまま。この crate は LLM の提案を **型に絞り込んで** 渡すだけで、
//! 裁定 (`adjudicate`) は一切しない。却下→再生成のループは上位 (harness) が担う。
//!
//! LocalAI `core_agent/src/llm_client.py` の移植:
//! - `generate`        → [`LlmClient::generate`]
//! - `generate_json`   → [`LlmClient::generate_structured`] (+ tool-use 強制)
//! - `LLMSettings`     → [`LlmConfig`]
//! - `_strip_code_fence` / フェンス除去 → [`parse::strip_code_fence`]

mod anthropic;
mod canonical;
mod client;
mod config;
mod error;
mod gemini;
mod openai_compat;
mod parse;
mod responses;
mod wire;

pub use client::{CachePoint, CacheStat, LlmClient};
pub use config::{Effort, LlmConfig, Provider, ToolMode};
pub use error::LlmError;
pub use parse::strip_reasoning_blocks;
pub use wire::{ChatMessage, Role};

use gm_core::StateDelta;

/// LLM に StateDelta を出させる時のツール名。
pub const EMIT_DELTA_TOOL: &str = "emit_delta";

const EMIT_DELTA_DESCRIPTION: &str = "\
今ターンの語り (narration) と、世界状態への変更要求 (ops) を提出する。\
narration は情景・NPC 台詞を自由に書いてよい。\
ops は構造化された要求のみで、エンジンが全件検証する。\
存在しないアイテムの取得や、達成していない移動を ops に書いても却下されるので、嘘の状態変更を書いてはならない。";

/// gm_core の型から機械生成した [`StateDelta`] の JSON Schema を返す。
///
/// **手書きしない** ── 規格 (schema) と実装 (Rust 型) の乖離は北極星「矛盾しない」に反する。
/// schemars が serde 属性 (`#[serde(tag = "op")]` 等) を尊重して単一真実源から導出する。
///
/// **自己完結化** ([`inline_schema_defs`]): schemars は `ops` 要素を `#/definitions/StateOp` への
/// `$ref` で出すが、tool-call grammar へ schema をコンパイルするサーバ (xAI Grok 等) は `$ref`/
/// `definitions`/`$schema` を解決しない (docs に記載なし) → `ops` を制約できず空デルタになる。
/// `$ref` を実体に inline し `definitions`/`$schema` を落として**どのプロバイダでも自己完結**にする
/// (Anthropic は $ref を解決できるが、Grok/OpenAI 厳格系は自己完結を要する。互換性の上位互換)。
pub fn state_delta_schema() -> serde_json::Value {
    // 既定 = additive 盤面: percentile 用の check_under を隠す (従来盤面は無風、spec 16)。
    state_delta_schema_excluding(&["check_under"])
}

/// [`state_delta_schema`] の判定様式対応版 (spec 16)。`extra_banned` に盤面が使わない
/// 判定 op (`check` / `check_under`) を渡すと AUTHORED_ONLY_OPS と合算で oneOf から落とす。
pub fn state_delta_schema_excluding(extra_banned: &[&str]) -> serde_json::Value {
    let schema = schemars::schema_for!(StateDelta);
    let value = serde_json::to_value(schema).expect("schemars 生成スキーマは必ず JSON 化できる");
    let inlined = inline_schema_defs(&value);
    filter_ops(inlined, extra_banned)
}

/// `ops` の oneOf から **authored 専権 op** ([`gm_core::AUTHORED_ONLY_OPS`]) + 追加除外を除く。
///
/// 専権 op は LLM が提案しても `adjudicate` が必ず却下する (trigger 効果でのみ実行)。schema に残すと
/// LLM が使い続けて却下→再生成ループで詰まる (特に constrained decoding な Grok は grammar に含めて
/// しまう)。除外すれば **LLM はそもそも提案できない** (Grok でも grammar に出ない=構造的遮断)。
/// 追加除外 (spec 16) は判定様式スイッチ: 使わない様式の判定 op を同じ機構で隠す。
fn filter_ops(mut schema: serde_json::Value, extra_banned: &[&str]) -> serde_json::Value {
    if let Some(variants) = schema
        .pointer_mut("/properties/ops/items/oneOf")
        .and_then(|v| v.as_array_mut())
    {
        variants.retain(|variant| {
            let op = variant
                .get("properties")
                .and_then(|p| p.get("op"))
                .and_then(|o| o.get("enum"))
                .and_then(|e| e.as_array())
                .and_then(|a| a.first())
                .and_then(|s| s.as_str());
            !matches!(op, Some(name)
                if gm_core::AUTHORED_ONLY_OPS.contains(&name) || extra_banned.contains(&name))
        });
    }
    schema
}

/// JSON Schema の `$ref` (`#/definitions/X` / `#/$defs/X`) を実体に inline し、
/// `definitions`/`$defs`/`$schema` キーを除去して**自己完結スキーマ**にする。
///
/// tool-call grammar コンパイラ (Grok 等) が参照解決をしない問題への対処。`seen` で展開中の
/// 定義名を追い、循環参照は空 object に落として無限再帰を防ぐ (現状の gm_core 型は非再帰だが安全側)。
pub fn inline_schema_defs(schema: &serde_json::Value) -> serde_json::Value {
    let defs = schema
        .get("definitions")
        .or_else(|| schema.get("$defs"))
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    inline_value(schema, &defs, &mut Vec::new())
}

fn inline_value(
    value: &serde_json::Value,
    defs: &serde_json::Map<String, serde_json::Value>,
    seen: &mut Vec<String>,
) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            // `$ref` は実体へ差し替え (循環は空 object で打ち切り)。
            if let Some(Value::String(r)) = map.get("$ref") {
                let name = r
                    .strip_prefix("#/definitions/")
                    .or_else(|| r.strip_prefix("#/$defs/"));
                if let Some(name) = name {
                    if seen.iter().any(|s| s == name) {
                        return Value::Object(serde_json::Map::new());
                    }
                    if let Some(def) = defs.get(name) {
                        seen.push(name.to_string());
                        let inlined = inline_value(def, defs, seen);
                        seen.pop();
                        return inlined;
                    }
                }
            }
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                // メタ/参照キーは自己完結スキーマから落とす。
                if matches!(k.as_str(), "$ref" | "definitions" | "$defs" | "$schema") {
                    continue;
                }
                out.insert(k.clone(), inline_value(v, defs, seen));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(|v| inline_value(v, defs, seen)).collect()),
        other => other.clone(),
    }
}

impl LlmClient {
    /// 1 ターン分の [`StateDelta`] を LLM に提案させる便宜メソッド。
    ///
    /// `messages` は呼び出し側 (harness) が構築する ── GM ペルソナ・シナリオ要約・
    /// 直近の却下理由 (再生成時) を system/user に積む責務は上位レイヤにある。
    /// この crate は transport + 構造化出力の保証だけを担う。
    pub async fn generate_delta(
        &self,
        messages: Vec<ChatMessage>,
    ) -> Result<StateDelta, LlmError> {
        // 判定様式 (spec 16): 盤面が使わない判定 op を schema から落とす (client 設定)。
        let banned: Vec<&str> = self.excluded_ops().iter().map(|s| s.as_str()).collect();
        let delta = self
            .generate_structured::<StateDelta>(
                messages,
                EMIT_DELTA_TOOL,
                EMIT_DELTA_DESCRIPTION,
                state_delta_schema_excluding(&banned),
            )
            .await?;
        // narration は非検証ゆえ、漏れた tool-call マークアップを提示前に掃除する
        // (ops は検証済の別フィールドで無改変)。
        Ok(StateDelta::new(
            parse::sanitize_narration(&delta.narration),
            delta.ops,
        ))
    }
}

// =============================================================================
// PoC: ナレーター脚の実証 (Red→Green)
// ネットワークは実キー必要で非決定的。壊れる ser/de 境界 (wire/parse/schema/config)
// を決定論的に固める。実 API 通しは「実クラウド通しプレイ」フェーズで行う。
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{
        ChatRequest, ChatResponse, Choice, FunctionCallResponse, ResponseMessage, ToolCallResponse,
    };
    use gm_core::StateOp;

    fn user_msgs() -> Vec<ChatMessage> {
        vec![ChatMessage::system("あなたはGM"), ChatMessage::user("引き出しを開ける")]
    }

    /// OpenAI 互換 wire 応答 → canonical (Phase A seam)。テストも本番と同じ decode を通す
    /// (= 新経路そのものが回帰テストされる)。
    fn canon(resp: ChatResponse) -> canonical::ChatResponse {
        openai_compat::decode(resp).expect("decode 成功前提のテスト")
    }

    /// 【spec 10: 要約モデルの別指定】SUMMARY_LLM_* の解決 — model か base_url が指定されて
    /// いれば Some (未指定フィールドは GM 本体設定から継承)、どちらも無ければ None
    /// (= GM の client 共用)。provider は明示 > **実効 base_url** からの自動判定
    /// (本体の provider を継がない — url が変われば話すべきプロトコルも変わる)。
    #[test]
    fn summary_overrides_inherit_base_and_detect_provider() {
        let base = LlmConfig::new("https://api.anthropic.com/v1", "sk-main", "claude-opus-4-8");
        assert_eq!(base.provider, Provider::Anthropic);

        // どちらも未指定 → None (GM 共用)。
        assert!(LlmConfig::summary_overrides(&base, None, None, None, None).unwrap().is_none());

        // model だけ差し替え → base_url/api_key は継承、provider は実効 url (anthropic) のまま。
        let cheap = LlmConfig::summary_overrides(
            &base, None, None, Some("claude-haiku-4-5-20251001".into()), None)
            .unwrap()
            .expect("model 指定で有効");
        assert_eq!(cheap.model, "claude-haiku-4-5-20251001");
        assert_eq!(cheap.base_url, base.base_url, "base_url は継承");
        assert_eq!(cheap.api_key, "sk-main", "api_key は継承");
        assert_eq!(cheap.provider, Provider::Anthropic);

        // base_url ごと差し替え → provider は新 url から再判定 (本体の anthropic を継がない)。
        let local = LlmConfig::summary_overrides(
            &base, Some("http://localhost:8080/v1".into()), None, Some("gemma".into()), None)
            .unwrap()
            .unwrap();
        assert_eq!(local.provider, Provider::OpenAiCompat, "実効 url から再判定");

        // tool_mode も同じ規律 — url が同じなら継ぎ (明示設定を黙って捨てない)、変われば再判定。
        let mut meta_base = LlmConfig::new("https://api.llama.com/compat/v1", "k", "Llama-4");
        assert_eq!(meta_base.tool_mode, ToolMode::Auto, "Meta は自動判定で Auto");
        let same_url = LlmConfig::summary_overrides(&meta_base, None, None, Some("Llama-3".into()), None)
            .unwrap()
            .unwrap();
        assert_eq!(same_url.tool_mode, ToolMode::Auto, "同じ接続先なら答えも同じ");
        meta_base.tool_mode = ToolMode::Off; // 受領者が LLM_USE_TOOLS=false を明示した状態
        let kept = LlmConfig::summary_overrides(&meta_base, None, None, Some("Llama-3".into()), None)
            .unwrap()
            .unwrap();
        assert_eq!(kept.tool_mode, ToolMode::Off, "明示設定を黙って捨てない");
        let moved = LlmConfig::summary_overrides(
            &meta_base, Some("https://api.openai.com/v1".into()), None, Some("gpt-4o-mini".into()), None)
            .unwrap()
            .unwrap();
        assert_eq!(moved.tool_mode, ToolMode::Forced, "url が変われば再判定");

        // provider 明示は自動判定に勝つ / 不正値は Err。
        let forced = LlmConfig::summary_overrides(
            &base, Some("http://proxy.example/v1".into()), None, Some("m".into()),
            Some("anthropic".into()))
            .unwrap()
            .unwrap();
        assert_eq!(forced.provider, Provider::Anthropic);
        assert!(LlmConfig::summary_overrides(
            &base, None, None, Some("m".into()), Some("banana".into())).is_err());
    }

    /// 【スキーマ機械生成】StateDelta schema が narration/ops を持ち、
    /// 全 StateOp バリアントの判別子 "op" を含む (規格=実装の単一真実源)。
    #[test]
    fn schema_is_generated_from_canonical_types() {
        let schema = state_delta_schema();
        let s = serde_json::to_string(&schema).unwrap();
        // StateDelta のフィールド
        assert!(s.contains("narration"), "narration プロパティが schema に無い");
        assert!(s.contains("ops"), "ops プロパティが schema に無い");
        // StateOp の内部タグと各 op 値 (serde rename_all=snake_case)
        for op in [
            "add_item", "remove_item", "set_flag", "move", "request_roll",
            "adjust_stat", "scale_stat",
        ] {
            assert!(s.contains(op), "op '{op}' が schema に無い (型と乖離)");
        }
        // 【自己完結 (Grok 対応)】$ref/definitions/$schema を残さない (tool-call grammar コンパイラが
        // 参照解決しないサーバでも ops を制約できるよう inline 済み)。
        assert!(!s.contains("$ref"), "$ref を inline で消す: {s}");
        assert!(!s.contains("definitions") && !s.contains("$defs"), "definitions を落とす");
        assert!(!s.contains("$schema"), "$schema メタを落とす");
    }

    /// 【$ref inline の健全性】inline_schema_defs が参照を実体へ展開し、ops 配列要素の中に
    /// 各 op の判別子が直接現れる ($ref 経由でなく自己完結)。
    #[test]
    fn inline_schema_defs_resolves_refs_self_contained() {
        let schema = state_delta_schema();
        // ops プロパティの items が $ref でなく実体 (oneOf の枝) を持つ。
        let ops_items = &schema["properties"]["ops"]["items"];
        assert!(ops_items.get("$ref").is_none(), "ops.items は $ref でなく実体");
        let dump = serde_json::to_string(ops_items).unwrap();
        assert!(dump.contains("add_item") && dump.contains("set_flag"), "op 実体が ops.items 内に inline");
    }

    /// 【spec 20 の結論】既成事実の**追記チャネルは schema から撤去した** — 実測 0/45・0/20 で
    /// GM は書かず (契機を三度書き直しても不発)、**語り手に記録係を兼ねさせるのが構造的に無理**
    /// と結論した (failures.md #65)。露出したままだと使われないフィールドの description が
    /// 毎リクエストのトークンを食い、たまに使われて不揃いな行が混ざる。旧デルタ互換は不変。
    #[test]
    fn schema_has_no_facts_channel() {
        let schema = state_delta_schema();
        assert!(
            schema["properties"]["facts"].is_null(),
            "GM の既成事実チャネルは撤去済み (ユーザーが宣言する欄になった)"
        );
        // 提出フィールドは narration / summary / ops の 3 つ。
        for keep in ["narration", "summary", "ops"] {
            assert!(!schema["properties"][keep].is_null(), "{keep} は残る");
        }
        let old: gm_core::StateDelta =
            serde_json::from_str(r#"{"narration":"x","ops":[]}"#).expect("旧デルタは parse 可能");
        assert!(old.ops.is_empty());
    }

    /// 【#64 単要素スカラーの救済】LLM は要素が 1 つのとき配列を省いて書くことが実在する
    /// (実測: Claude が `"facts": "…"` とスカラーで書いた)。従来は
    /// `invalid type: string, expected a sequence` で **delta 全体がパース失敗 → 再送 →
    /// 内容ごと失われる**。facts は撤去したが、**同じ罠は `ops` にもある**ので救済は残す。
    /// schema は配列のまま (指示は正しく出す)、受け側だけ寛容にする。
    #[test]
    fn scalar_written_where_an_array_was_expected_is_rescued() {
        // 単一 op をオブジェクトで書く出力を救済する。
        let d: gm_core::StateDelta = serde_json::from_str(
            r#"{"narration":"n","ops":{"op":"set_flag","key":"a","value":true}}"#,
        )
        .expect("スカラー (単一オブジェクト) の ops を救済して parse できる");
        assert_eq!(d.ops.len(), 1);

        // 正しい配列形は当然そのまま通る (救済で壊さない)。
        let d: gm_core::StateDelta =
            serde_json::from_str(r#"{"narration":"n","ops":[]}"#).unwrap();
        assert!(d.ops.is_empty());
    }

    /// 【authored 専権 op の除外】LLM 向け schema は set_presence/grant_skill 等を **提案肢に出さない**
    /// (露出すると LLM が使い続けて却下→再生成ループで詰まる。Grok の constrained decoding 対策の核心)。
    /// LLM が使える op (add_item 等) は残る。
    #[test]
    fn schema_excludes_authored_only_ops() {
        let schema = state_delta_schema();
        let variants = schema["properties"]["ops"]["items"]["oneOf"]
            .as_array()
            .expect("ops.items.oneOf は配列");
        let op_tags: Vec<String> = variants
            .iter()
            .filter_map(|v| v["properties"]["op"]["enum"][0].as_str().map(String::from))
            .collect();
        // authored 専権 op は1つも露出しない。
        for banned in gm_core::AUTHORED_ONLY_OPS {
            assert!(!op_tags.iter().any(|t| t == banned), "authored 専権 op '{banned}' を schema から除外");
        }
        // LLM が使える代表 op は残る。
        for keep in ["add_item", "set_flag", "move", "adjust_stat", "check", "attempt_challenge"] {
            assert!(op_tags.iter().any(|t| t == keep), "提案可能な op '{keep}' は残す");
        }
        // 既定 (additive 盤面) では percentile 用 check_under を隠す (spec 16)。
        assert!(!op_tags.iter().any(|t| t == "check_under"), "既定 schema に check_under は出ない");
    }

    /// 【spec 16: 判定様式による schema 入替】percentile 盤面 (excluding check) では
    /// check_under が露出し加算式 check が消える — 使わない様式を構造的に混ぜさせない
    /// (filter_authored_only_ops と同じ機構 = Grok の grammar からも消える)。
    #[test]
    fn schema_excluding_swaps_check_ops_by_style() {
        let op_tags = |schema: &serde_json::Value| -> Vec<String> {
            schema["properties"]["ops"]["items"]["oneOf"]
                .as_array()
                .expect("ops.items.oneOf は配列")
                .iter()
                .filter_map(|v| v["properties"]["op"]["enum"][0].as_str().map(String::from))
                .collect()
        };
        // percentile 盤面: check を隠し check_under を出す。
        let p = state_delta_schema_excluding(&["check"]);
        let tags = op_tags(&p);
        assert!(tags.iter().any(|t| t == "check_under"), "percentile は check_under を露出");
        assert!(!tags.iter().any(|t| t == "check"), "percentile は加算式 check を隠す");
        // 専権 op の除外は様式と独立に常に効く (roll_stat = 第6例も)。
        for banned in gm_core::AUTHORED_ONLY_OPS {
            assert!(!tags.iter().any(|t| t == banned), "'{banned}' は常に除外");
        }
    }

    /// 【リクエスト整形】generate_structured が tool を載せ、tool_choice で強制する。
    #[test]
    fn request_forces_the_emit_delta_tool() {
        let req = ChatRequest {
            model: "m".into(),
            messages: user_msgs(),
            temperature: Some(0.1),
            max_tokens: Some(256),
            max_completion_tokens: None,
            tools: vec![wire::Tool {
                kind: wire::ToolKind::Function,
                function: wire::FunctionDef {
                    name: EMIT_DELTA_TOOL.into(),
                    description: "d".into(),
                    parameters: state_delta_schema(),
                },
            }],
            tool_choice: Some(wire::ToolChoice::force(EMIT_DELTA_TOOL)),
            reasoning_effort: None,
        };
        let body = serde_json::to_value(&req).unwrap();
        assert_eq!(body["tool_choice"]["type"], "function");
        assert_eq!(body["tool_choice"]["function"]["name"], EMIT_DELTA_TOOL);
        assert_eq!(body["tools"][0]["function"]["name"], EMIT_DELTA_TOOL);
        // f32→JSON は精度差が出るので近似比較 (明示時は temperature を送る)。
        assert!(
            (body["temperature"].as_f64().unwrap() - 0.1).abs() < 1e-3,
            "明示時は temperature を送る"
        );
        // ツール無しの generate ではキーごと消える (skip_serializing_if)。
        let plain = ChatRequest {
            model: "m".into(),
            messages: user_msgs(),
            temperature: None,
            max_tokens: Some(256),
            max_completion_tokens: None,
            tools: vec![],
            tool_choice: None,
            reasoning_effort: None,
        };
        let pbody = serde_json::to_value(&plain).unwrap();
        assert!(pbody.get("tools").is_none(), "ツール無しなら tools は出さない");
        assert!(pbody.get("tool_choice").is_none());
        // temperature 未設定 (None) なら **キーごと送らない** (新しめモデルは送ると 400)。
        assert!(
            pbody.get("temperature").is_none(),
            "temperature None なら省略する (claude-opus-4-8 等は temperature 非対応)"
        );
    }

    fn response_with_tool_args(args: &str) -> ChatResponse {
        ChatResponse {
            usage: None,
            choices: vec![Choice {
                message: ResponseMessage {
                    content: None,
                    tool_calls: vec![ToolCallResponse {
                        function: FunctionCallResponse {
                            arguments: args.into(),
                            name: None,
                        },
                        id: None,
                    }],
                },
                finish_reason: None,
            }],
        }
    }

    /// content だけの互換応答 (tool_calls 無し) を作るテストヘルパ。
    fn response_with_content(content: &str) -> ChatResponse {
        ChatResponse {
            usage: None,
            choices: vec![Choice {
                message: ResponseMessage { content: Some(content.into()), tool_calls: vec![] },
                finish_reason: None,
            }],
        }
    }

    /// 【主経路】tool_calls の arguments (JSON 文字列) を StateDelta に解決する。
    #[test]
    fn parses_state_delta_from_tool_call() {
        let resp = response_with_tool_args(
            r#"{"narration":"古い引き出しが軋む","ops":[{"op":"set_flag","key":"drawer_opened","value":true}]}"#,
        );
        let delta: StateDelta = parse::extract(&canon(resp)).unwrap();
        assert_eq!(delta.narration, "古い引き出しが軋む");
        assert_eq!(delta.ops.len(), 1);
        assert!(matches!(
            &delta.ops[0],
            StateOp::SetFlag { key, value: true } if key == "drawer_opened"
        ));
    }

    /// 【フォールバック】tool_calls 不在でも content のフェンス JSON から解決する
    /// (tool_choice を尊重しないサーバ/モデルへの保険、Python generate_json 同型)。
    #[test]
    fn falls_back_to_fenced_json_in_content() {
        let resp = response_with_content("```json\n{\"narration\":\"扉を調べる\",\"ops\":[]}\n```");
        let delta: StateDelta = parse::extract(&canon(resp)).unwrap();
        assert_eq!(delta.narration, "扉を調べる");
        assert!(delta.ops.is_empty());
    }

    /// 【no-tools モード】tool_calls 無しで content が素の JSON / prose 包みでも StateDelta に解決する
    /// (さくら AI Engine 等 tool_choice 非対応サーバの経路。LLM_USE_TOOLS=false)。
    #[test]
    fn parses_state_delta_from_plain_and_prose_content() {
        // 素の JSON (コードフェンス無し)。さくら gpt-oss-120b の実観測形。
        let raw = response_with_content(r#"{"narration":"教室に入る","ops":[]}"#);
        let d: StateDelta = parse::extract(&canon(raw)).unwrap();
        assert_eq!(d.narration, "教室に入る");
        assert!(d.ops.is_empty());

        // prose に前後を包まれても balanced な `{...}` で救済。
        let prose = response_with_content("はい:\n{\"narration\":\"了解\",\"ops\":[]} 以上");
        let d2: StateDelta = parse::extract(&canon(prose)).unwrap();
        assert_eq!(d2.narration, "了解");
    }

    /// 【推論モデルの no-tools 救済 (#30)】Gemma 等は `<thought>...</thought>` で CoT を吐き、
    /// その中に JSON 断片 (`{"op":...}`) を書いてから ```json フェンスで本体を出す。旧コードは
    /// (a) フェンスが先頭に無く strip_code_fence が効かない (b) first '{' が thought 内の断片に
    /// 釣られる の二重欠陥で parse 失敗していた。推論ブロック除去で解決する。
    #[test]
    fn reasoning_block_then_fenced_json_resolves() {
        let raw = "<thought>* Player asks for math materials.\n\
                   * Ops: `[{\"op\": \"adjust_stat\", \"entity\": \"moka\", \"key\": \"好感度\", \"delta\": 1}]`\
                   </thought>```json\n\
                   { \"narration\": \"モカは教材棚を顎で指した。\", \"ops\": [ { \"op\": \"adjust_stat\", \"entity\": \"moka\", \"key\": \"好感度\", \"delta\": 1 } ] }\n\
                   ```";
        let resp = response_with_content(raw);
        let d: StateDelta = parse::extract(&canon(resp)).unwrap();
        assert_eq!(d.narration, "モカは教材棚を顎で指した。");
        assert_eq!(d.ops.len(), 1, "thought 内の断片でなく本体の ops を拾う");
    }

    /// 【最後の object を採る (#30)】StateDelta は narration/ops が serde(default) なので無関係な
    /// 断片 `{...}` すら空デルタとして parse 成功する。タグ無しで前置き断片が混じっても、
    /// 「答えは推論の後に来る」原則で **最後の balanced object** を採り本体を拾う (first '{' では空を拾う罠)。
    #[test]
    fn last_balanced_json_object_wins() {
        let raw = "consider {\"op\":\"x\"} then answer:\n{\"narration\":\"本体\",\"ops\":[]}";
        let resp = response_with_content(raw);
        let d: StateDelta = parse::extract(&canon(resp)).unwrap();
        assert_eq!(d.narration, "本体", "前置き断片でなく最後の object を採る");
    }

    /// 【JSON モード指示】json_instruction は schema (narration/ops) と「JSON だけ出せ」旨を含む。
    #[test]
    fn json_instruction_carries_schema_and_directive() {
        let s = crate::openai_compat::json_instruction(&state_delta_schema());
        assert!(s.contains("JSON"), "JSON 出力指示を含む");
        assert!(s.contains("narration") && s.contains("ops"), "schema (narration/ops) を含む");
    }

    /// 【既定 tool-use】LlmConfig::new / 未設定時は use_tools=true (OpenAI/Anthropic 既定経路)。
    #[test]
    fn config_defaults_to_tool_use() {
        assert_eq!(LlmConfig::new("u", "k", "m").tool_mode, ToolMode::Forced, "既定は名前指定の強制");
    }

    /// 【#40 の救済は content 経路でも効く / 実機 Meta の形】ツールを強制できないサーバでは
    /// モデルが content に JSON を書く。schema を知らないと `ops` を配列でなく文字列にする
    /// (2026-08-08 Meta 実機 = ops が改行 1 文字の文字列)。救済は tool 経路と content 経路の
    /// **両方**に要る。
    #[test]
    fn ops_as_string_is_rescued_in_content_path_too() {
        // 実機の形をプログラムで組む (テスト側のエスケープ取り違えを排除する)。
        let obj = serde_json::json!({ "narration": "語り", "ops": "\n" }).to_string();
        // 素の JSON / 前後に散文 / フェンス — どの包み方でも拾う。
        for (label, text) in [
            ("素の JSON", obj.clone()),
            ("後ろに散文", format!("{obj}\n\nこの後に補足を書きました。")),
            ("前に散文", format!("以下が結果です:\n{obj}")),
            ("フェンス", format!("```json\n{obj}\n```")),
        ] {
            let resp = canonical::ChatResponse {
                text: Some(text),
                tool_calls: Vec::new(),
                finish: canonical::Finish::Stop,
                usage: Default::default(),
            };
            let d: StateDelta = parse::extract(&resp).unwrap_or_else(|e| panic!("{label}: {e}"));
            assert!(d.ops.is_empty(), "{label}: 空白のみの ops は空配列へ");
            assert_eq!(d.narration, "語り", "{label}");
        }
    }

    /// 【エラーの選び方が診断そのもの】型付きパースは前から読むので、**後ろが壊れた**応答でも
    /// 先に見つかった型の不一致を報告してしまう。素の JSON としても読めないなら詰まっている
    /// のは型ではなく構文 — そちらを返す。
    ///
    /// 旧実装は救済の失敗を `.map_err(|_| first)` で握り潰しており、画面には「ops が文字列」と
    /// 出続けるのに実際の詰まりは別、という**恒久的に真因が見えない**状態を作っていた。
    #[test]
    fn truncated_output_reports_the_syntax_error_not_the_type_symptom() {
        // narration は読めるが ops の途中で切れている (出力上限で途中終了した応答の形)。
        // 型付きパースは ops に**先に**到達して「配列であるべき」と言うが、真の詰まりは
        // 「本文が閉じていない」ほう。この 2 つを取り違えると調査が症状の側へ逸れる。
        let truncated = "{\"narration\": \"長い語り\", \"ops\": \"";
        let resp = canonical::ChatResponse {
            text: Some(truncated.to_string()),
            tool_calls: Vec::new(),
            finish: canonical::Finish::Length,
            usage: Default::default(),
        };
        let err = parse::extract::<StateDelta>(&resp).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("EOF") || msg.contains("eof") || msg.contains("end of input"),
            "途中で切れたことが読める文面であること: {msg}"
        );
        assert!(msg.contains("--- raw ---"), "raw を再生成の燃料として保持する (#34)");
    }

    /// 【ops が文字列に化ける崩れの救済 (#40)】Gemini 実プレイで観測: `"ops": "\n"` (配列で
    /// あるべき場所に文字列) を出し、パース失敗で 9 ターン中 4 ターンが丸ごと蒸発した。
    /// 決定論的に救済する — 空白のみの文字列 → 空配列、JSON 配列の二重エンコード → その配列。
    #[test]
    fn ops_as_string_is_rescued() {
        let resp = response_with_tool_args(r#"{"narration":"夜が更ける","ops":"\n"}"#);
        let d: StateDelta = parse::extract(&canon(resp)).unwrap();
        assert_eq!(d.narration, "夜が更ける");
        assert!(d.ops.is_empty(), "空白のみの ops 文字列は空配列として救済");

        let resp = response_with_tool_args(
            r#"{"narration":"n","ops":"[{\"op\":\"set_flag\",\"key\":\"k\",\"value\":true}]"}"#,
        );
        let d: StateDelta = parse::extract(&canon(resp)).unwrap();
        assert_eq!(d.ops.len(), 1, "二重エンコードされた ops は配列として救済");
    }

    /// 【再生成の燃料】壊れた JSON は **decode 境界で** raw を保持した Parse エラーになる
    /// (却下→再生成ループが raw を LLM に戻せること = self_repair 同型の前提。
    /// Phase A で「arguments 文字列の 1 回だけの parse」が adapter decode へ移った)。
    #[test]
    fn malformed_json_keeps_raw_for_repair() {
        let resp = response_with_tool_args(r#"{"narration":"壊れた,"ops":["#);
        let err = openai_compat::decode(resp).unwrap_err();
        match err {
            LlmError::Parse { raw, .. } => assert!(raw.contains("壊れた"), "raw を再生成用に保持すべき"),
            other => panic!("Parse エラーであるべき: {other:?}"),
        }
    }

    /// 【200 なのに形が合わない応答は本文を保持する (#34)】Gemini 実プレイで観測:
    /// 長セッション中に HTTP 200 で `choices[0]` に `message` が無い応答が返り、
    /// `resp.json()` 直はデコード失敗時に**本文を捨てる**ため
    /// 「missing field `message` at line 1 column 76」だけが残り真因 (content filter /
    /// 長さ切れ / quota 系の変形応答) が診断不能だった。text→parse に統一し raw を保持する。
    #[test]
    fn decode_chat_body_keeps_raw_on_shape_mismatch() {
        let body = r#"{"choices":[{"finish_reason":"content_filter","index":0}],"created":1}"#;
        let err = client::decode_chat_body(body.to_string()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("content_filter"), "応答本文 (真因) が surface される: {msg}");
        assert!(msg.contains("message"), "何が欠けたか (missing field) も出る: {msg}");
    }

    /// 【narration 掃除】モデルが narration 本文に漏らした tool-call マークアップを除去する
    /// (実プレイで観測: `</narration>` / `<parameter name="ops">` が語りに混入)。
    #[test]
    fn sanitizes_leaked_tool_markup_from_narration() {
        // 観測された実例: narration の末尾に閉じタグ + ops の format token が漏れた。
        let leaked = "けらけら、と笑う。夕日の中で小さく光っていた。</narration>\n<parameter name=\"ops\">[{\"op\":\"set_flag\"}]";
        let clean = parse::sanitize_narration(leaked);
        assert_eq!(clean, "けらけら、と笑う。夕日の中で小さく光っていた。");
        assert!(!clean.contains("</narration>") && !clean.contains("<parameter"));

        // 先頭の開きタグも剥がす。
        assert_eq!(parse::sanitize_narration("<narration>本文だけ</narration>"), "本文だけ");
        // Anthropic XML 関数呼び出しタグも切る。
        assert!(!parse::sanitize_narration("語り<function_calls><invoke name=\"emit_delta\">").contains('<'));
        // 正常な narration は無改変。
        assert_eq!(parse::sanitize_narration("普通の語り。"), "普通の語り。");
    }

    /// 【症状と修正の分離】tool_call は valid JSON なので extract はタグ混入を素通りさせる
    /// (= 症状)。sanitize で初めて掃除される。ops は別フィールドで無改変。
    #[test]
    fn extract_passes_leaked_tags_sanitize_cleans_them() {
        let args = r#"{"narration":"語り。</narration><parameter name=\"ops\">[]","ops":[{"op":"set_flag","key":"f","value":true}]}"#;
        let resp = response_with_tool_args(args);
        let delta: StateDelta = parse::extract(&canon(resp)).unwrap();
        assert!(delta.narration.contains("</narration>"), "extract 単体ではタグが残る (症状)");
        assert_eq!(delta.ops.len(), 1, "ops は valid な別フィールドで正常");
        assert_eq!(parse::sanitize_narration(&delta.narration), "語り。", "sanitize で掃除");
    }

    /// 【空応答】tool_calls も content も無ければ NoStructuredOutput。
    #[test]
    fn empty_message_is_no_structured_output() {
        let resp = ChatResponse {
            usage: None,
            choices: vec![Choice {
                message: ResponseMessage { content: None, tool_calls: vec![] },
                finish_reason: None,
            }],
        };
        let err = parse::extract::<StateDelta>(&canon(resp)).unwrap_err();
        assert!(matches!(err, LlmError::NoStructuredOutput));
    }

    /// 【フェンス除去】```json / ``` の両形を剥がす。
    #[test]
    fn strips_code_fences() {
        assert_eq!(parse::strip_code_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(parse::strip_code_fence("```\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(parse::strip_code_fence("{\"a\":1}"), "{\"a\":1}");
    }

    /// 【config】環境変数から構築。api_key 欠落は Config エラー。
    #[test]
    fn config_requires_api_key() {
        // 既存環境を汚さないよう、このテスト内だけで設定/復元。
        std::env::remove_var("LLM_API_KEY");
        let err = LlmConfig::from_env().unwrap_err();
        assert!(matches!(err, LlmError::Config(_)), "api_key 欠落は Config エラー");

        let cfg = LlmConfig::new("https://api.example.com/v1/", "sk-test", "gpt-4o-mini");
        assert_eq!(cfg.chat_endpoint(), "https://api.example.com/v1/chat/completions");
    }

    /// 【一過性判定】5xx/429 はリトライ対象、4xx (除 429) はしない。EmptyResponse は
    /// Phase D で一過性へ昇格 (推論モデルの空応答 = 思考の再抽選で回復しうる)。
    #[test]
    fn transient_classification() {
        assert!(LlmError::Api { status: 503, body: String::new() }.is_transient());
        assert!(LlmError::Api { status: 429, body: String::new() }.is_transient());
        assert!(!LlmError::Api { status: 400, body: String::new() }.is_transient());
        assert!(!LlmError::NoStructuredOutput.is_transient());
        assert!(LlmError::EmptyResponse.is_transient(), "空応答は再抽選に乗る (Phase D)");
    }

    // --- Anthropic ネイティブ経路 (prompt caching, #44) --------------------------

    /// 【provider 自動判定】api.anthropic.com はネイティブ Messages API (キャッシュの効く経路)、
    /// それ以外は従来の OpenAI 互換。LLM_PROVIDER 明示が無ければ base_url から決まる。
    #[test]
    fn provider_autodetects_anthropic_from_base_url() {
        let native = LlmConfig::new("https://api.anthropic.com/v1", "sk", "claude-opus-4-8");
        assert_eq!(native.provider, Provider::Anthropic);
        assert_eq!(native.messages_endpoint(), "https://api.anthropic.com/v1/messages");

        let compat = LlmConfig::new("https://api.example.com/v1/", "sk", "gpt-4o-mini");
        assert_eq!(compat.provider, Provider::OpenAiCompat);
    }

    /// 【cache_control の配置】ネイティブリクエストは安定プレフィックス (tools→system) の
    /// **末尾 = 最後の system ブロック**にだけ ephemeral breakpoint を置く。可変な user
    /// メッセージには置かない (毎ターン別内容 → 読まれないキャッシュ書込 1.25× の無駄)。
    #[test]
    fn anthropic_request_places_cache_control_on_system_tail() {
        let req = anthropic::encode(&canonical::ChatRequest {
            model: "claude-opus-4-8".into(),
            messages: user_msgs(),
            tools: vec![canonical::ToolSpec {
                name: EMIT_DELTA_TOOL.into(),
                description: "d".into(),
                parameters: state_delta_schema(),
            }],
            tool_choice: canonical::ToolChoice::Specific(EMIT_DELTA_TOOL.into()),
            temperature: None,
            max_tokens: 4096,
            effort: None,
        });
        let body = serde_json::to_value(&req).unwrap();

        // system は配列で、末尾ブロックに cache_control: ephemeral。
        let system = body["system"].as_array().expect("system はブロック配列");
        assert_eq!(system.last().unwrap()["cache_control"]["type"], "ephemeral");
        // messages 側には cache_control を置かない (どのブロックにも無い)。
        let msgs = serde_json::to_string(&body["messages"]).unwrap();
        assert!(!msgs.contains("cache_control"), "可変部に breakpoint を置かない: {msgs}");
        // system は messages に混ざらない (ネイティブは先頭 system 専用フィールド)。
        assert!(!msgs.contains("system"), "system ロールは messages に出さない");
        // tools はネイティブ形 (input_schema)、tool_choice は {type:tool, name} で強制。
        assert_eq!(body["tools"][0]["name"], EMIT_DELTA_TOOL);
        assert!(body["tools"][0]["input_schema"].is_object(), "schema は input_schema キー");
        assert_eq!(body["tool_choice"]["type"], "tool");
        assert_eq!(body["tool_choice"]["name"], EMIT_DELTA_TOOL);
        // temperature 未設定なら送らない (claude-opus-4-8 は temperature 非対応)。
        assert!(body.get("temperature").is_none());
        assert_eq!(body["max_tokens"], 4096);
        // effort 未設定 (opt-in) なら thinking/output_config もキーごと送らない = 現行動作。
        assert!(body.get("thinking").is_none());
        assert!(body.get("output_config").is_none());
    }

    /// 【spec 14 Phase A】Anthropic 多段 breakpoint: 先頭から連続する leading system
    /// **メッセージ毎**に cache_control を置く (API 上限の 4 個まで、先頭から)。
    /// 1 本 (静的のみ) は従来と同数 = 回帰なし。2 本 (静的 + synopsis) で二段キャッシュ —
    /// 章追加ターンは第二段だけ失効し、第一段 (静的) は生き残る。
    #[test]
    fn anthropic_places_breakpoint_per_leading_system_capped_at_four() {
        let mk = |n: usize| {
            let mut msgs: Vec<ChatMessage> =
                (0..n).map(|i| ChatMessage::system(format!("S{i}"))).collect();
            msgs.push(ChatMessage::user("行動"));
            canonical::ChatRequest {
                model: "claude-opus-4-8".into(),
                messages: msgs,
                tools: Vec::new(),
                tool_choice: canonical::ToolChoice::None,
                temperature: None,
                max_tokens: 4096,
                effort: None,
            }
        };
        let marks = |n: usize| -> Vec<bool> {
            let body = serde_json::to_value(anthropic::encode(&mk(n))).unwrap();
            body["system"]
                .as_array()
                .expect("system はブロック配列")
                .iter()
                .map(|b| b.get("cache_control").is_some())
                .collect()
        };
        assert_eq!(marks(1), vec![true], "1 本 → 1 個 (既存挙動の回帰なし)");
        assert_eq!(marks(2), vec![true, true], "静的 + synopsis → 2 breakpoint (二段キャッシュ)");
        assert_eq!(
            marks(5),
            vec![true, true, true, true, false],
            "cap 4: 先頭 4 個まで、5 本目には置かない"
        );
    }

    /// 【ネイティブ応答の解決】tool_use ブロックの input (JSON オブジェクト) を canonical へ
    /// **恒等写像**し (D2 — 文字列化→再パースの往復を廃止)、単一抽出経路 parse::extract →
    /// StateDelta。usage は canonical Usage に正規化される
    /// (検証は cache_read > 0 = #44 の Green 判定)。
    #[test]
    fn anthropic_response_resolves_tool_use_and_usage() {
        let body = r#"{
            "content": [
                {"type": "text", "text": "考え中"},
                {"type": "tool_use", "id": "tu_1", "name": "emit_delta",
                 "input": {"narration": "扉が軋む", "ops": [{"op":"set_flag","key":"door_open","value":true}]}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 321, "output_tokens": 55,
                      "cache_creation_input_tokens": 0, "cache_read_input_tokens": 8021}
        }"#;
        let resp = client::decode_messages_body(body.to_string()).unwrap();
        let usage = resp.usage.clone().expect("usage をパースする");
        assert_eq!(usage.cache_read_input_tokens, 8021, "キャッシュ読取が計上される");
        assert_eq!(usage.input_tokens, 321);

        let canonical_resp = anthropic::decode(resp);
        assert_eq!(canonical_resp.usage.cache_read, 8021, "canonical Usage に正規化");
        // 【spec 14 D5 / failures #58】Anthropic native の input_tokens は**非キャッシュ分のみ**
        // (総入力 = input + cache_read + cache_creation)。canonical の prompt は「総入力」に
        // 正規化する — さもないと hit rate (cached/prompt) が 1 を超える (実測 ratio=6.88)。
        assert_eq!(
            canonical_resp.usage.prompt,
            321 + 8021,
            "prompt = 総入力 (input_tokens + cache_read + cache_creation) に正規化"
        );
        assert_eq!(canonical_resp.finish, canonical::Finish::ToolUse);
        assert_eq!(canonical_resp.tool_calls[0].id, "tu_1");
        assert_eq!(canonical_resp.tool_calls[0].name, "emit_delta");
        let delta: StateDelta = parse::extract(&canonical_resp).unwrap();
        assert_eq!(delta.narration, "扉が軋む");
        assert_eq!(delta.ops.len(), 1);
    }

    // --- spec 12 Phase A: canonical + adapter seam --------------------------------

    /// 【Phase A encode】canonical → OpenAI 互換 wire。tool-use なら tools + tool_choice 強制、
    /// no-tools (#29) なら tools を送らず json_instruction を messages **末尾の system** に積む
    /// (従来 generate_structured 内の分岐の移設 = 挙動不変)。
    #[test]
    fn canonical_encode_openai_tool_use_and_no_tools() {
        let req = canonical::ChatRequest {
            model: "m".into(),
            messages: user_msgs(),
            tools: vec![canonical::ToolSpec {
                name: EMIT_DELTA_TOOL.into(),
                description: "d".into(),
                parameters: state_delta_schema(),
            }],
            tool_choice: canonical::ToolChoice::Specific(EMIT_DELTA_TOOL.into()),
            temperature: None,
            max_tokens: 256,
            effort: None,
        };

        let with_tools = openai_compat::encode(&req, ToolMode::Forced);
        let body = serde_json::to_value(&with_tools).unwrap();
        assert_eq!(body["tools"][0]["function"]["name"], EMIT_DELTA_TOOL);
        assert_eq!(body["tool_choice"]["function"]["name"], EMIT_DELTA_TOOL);
        assert_eq!(with_tools.messages.len(), 2, "tool-use では指示メッセージを足さない");

        let no_tools = openai_compat::encode(&req, ToolMode::Off);
        let nbody = serde_json::to_value(&no_tools).unwrap();
        assert!(nbody.get("tools").is_none(), "no-tools では tools を送らない");
        assert!(nbody.get("tool_choice").is_none());
        let last = no_tools.messages.last().unwrap();
        assert_eq!(last.role, Role::System, "json_instruction は末尾の system");
        assert!(last.content.contains("JSON Schema"), "schema を載せた指示を積む");
    }

    /// 【Phase B effort】LLM_EFFORT 設定時のみ `thinking: adaptive` + `output_config.effort` を
    /// 送る (形は公式例に固定 — effort は output_config の**中**。budget_tokens は送らない =
    /// Opus 4.8/Sonnet 5 で 400)。未設定なら**キーごと送らない** = 現行動作 (opt-in)。
    #[test]
    fn anthropic_encode_effort_is_opt_in() {
        let mut req = canonical::ChatRequest {
            model: "claude-opus-4-8".into(),
            messages: user_msgs(),
            tools: vec![canonical::ToolSpec {
                name: EMIT_DELTA_TOOL.into(),
                description: "d".into(),
                parameters: state_delta_schema(),
            }],
            tool_choice: canonical::ToolChoice::Specific(EMIT_DELTA_TOOL.into()),
            temperature: None,
            max_tokens: 64000,
            effort: Some(Effort::XHigh),
        };
        let body = serde_json::to_value(anthropic::encode(&req)).unwrap();
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["output_config"]["effort"], "xhigh");
        assert!(body["thinking"].get("budget_tokens").is_none(), "budget_tokens は送らない");
        assert!(body.get("effort").is_none(), "effort はトップレベルに置かない");

        req.effort = None;
        let body = serde_json::to_value(anthropic::encode(&req)).unwrap();
        assert!(body.get("thinking").is_none(), "未設定なら送らない = 現行動作");
        assert!(body.get("output_config").is_none());
    }

    /// 【Phase B config】LLM_EFFORT の語彙パース (純粋)。5 段階 + 表記ゆれ、不正値は
    /// **黙って無視せず** Config エラー (「効いているつもり」の静かな漏出を防ぐ #44 の教訓)。
    #[test]
    fn effort_parses_vocabulary_and_rejects_unknown() {
        assert_eq!(Effort::parse("high").unwrap(), Effort::High);
        assert_eq!(Effort::parse(" XHIGH ").unwrap(), Effort::XHigh);
        assert_eq!(Effort::parse("x-high").unwrap(), Effort::XHigh);
        assert_eq!(Effort::parse("max").unwrap(), Effort::Max);
        assert!(matches!(Effort::parse("banana"), Err(LlmError::Config(_))));
    }

    /// 【Phase B 警告】effort 設定時の headroom 不足 (max_tokens は thinking+output の合算上限
    /// = combined、claude-api リファレンス接地) と temperature 併用 (対象モデルは 400) を
    /// **非 fatal** で surface する (純粋・テスト可。from_env が stderr へ出す)。
    #[test]
    fn config_warnings_surface_headroom_and_temperature_conflicts() {
        let mut cfg = LlmConfig::new("https://api.anthropic.com/v1", "sk", "claude-opus-4-8");
        assert!(cfg.warnings().is_empty(), "effort 未設定なら警告なし (現行構成は無音)");

        cfg.effort = Some(Effort::High);
        let w = cfg.warnings(); // max_tokens は既定 4096 のまま
        assert_eq!(w.len(), 1, "headroom 警告: {w:?}");
        assert!(w[0].contains("16000"), "推奨値を名指しする: {w:?}");

        cfg.temperature = Some(0.7);
        assert_eq!(cfg.warnings().len(), 2, "temperature 併用も警告");

        cfg.max_tokens = 64000;
        cfg.temperature = None;
        assert!(cfg.warnings().is_empty(), "headroom 確保 + temperature 撤去で無音");
    }

    /// 【Phase A decode】OpenAI 互換 wire → canonical。finish_reason/usage を正規化し、
    /// arguments (JSON 文字列) を **decode 境界で 1 回だけ**オブジェクト化する (写経元 D2)。
    /// finish=length は empty-response 防御 (Phase D) の判定材料。
    #[test]
    fn canonical_decode_openai_maps_finish_usage_and_args() {
        let body = r#"{
            "choices": [{"finish_reason": "length",
                         "message": {"tool_calls": [{"id": "c1",
                             "function": {"name": "emit_delta",
                                          "arguments": "{\"narration\":\"n\",\"ops\":[]}"}}]}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 2,
                      "prompt_tokens_details": {"cached_tokens": 7}}
        }"#;
        let wire_resp = client::decode_chat_body(body.to_string()).unwrap();
        let resp = openai_compat::decode(wire_resp).unwrap();
        assert_eq!(resp.finish, canonical::Finish::Length);
        assert_eq!(resp.usage.cache_read, 7, "cached_tokens → canonical cache_read");
        assert_eq!(resp.usage.prompt, 10);
        let call = &resp.tool_calls[0];
        assert_eq!(call.id, "c1");
        assert_eq!(call.name, "emit_delta");
        assert!(call.args.is_object(), "args は decode 境界でオブジェクト化 (D2)");
    }

    /// 【壊れた応答は raw を保持 (#34 同型)】2xx なのに形が合わない Messages 応答も
    /// 本文を保持した Parse エラーになる (真因診断 + 再生成の燃料)。
    #[test]
    fn anthropic_decode_keeps_raw_on_shape_mismatch() {
        let err = client::decode_messages_body(r#"{"content": "not-an-array"}"#.into()).unwrap_err();
        match err {
            LlmError::Parse { raw, .. } => assert!(raw.contains("not-an-array")),
            other => panic!("Parse エラーであるべき: {other:?}"),
        }
    }

    // --- Grok 方言 + empty-response 防御 (spec 12 Phase D) ----------------------------

    /// 【Phase D Grok】reasoning_effort は対象モデル (grok-4.3/4.5) に**既定で送る** (opt-out) —
    /// 未送出だと xAI 既定 (4.3=常時思考/4.5=high) が適用され、思考が max_tokens (合算上限) を
    /// 食い潰して空デルタ/タイムアウトになる (grok-4.3 実測の真因仮説、上流 repo と同判断)。
    /// 既定 4.3→none / 4.5→low。LLM_EFFORT 明示は尊重 (xhigh/max は Grok 未対応で high へ丸め)。
    /// 非対象 (fast 系/他モデル) には送らない。
    #[test]
    fn grok_reasoning_effort_defaults_and_clamps() {
        use openai_compat::reasoning_effort as f;
        // 既定 (LLM_EFFORT なし): 対象モデルにだけ明示送出。
        assert_eq!(f("grok-4.3", None, true), Some("none"), "4.3 は none (常時思考を切る)");
        assert_eq!(f("grok-4.5", None, true), Some("low"), "4.5 は low (none 不可)");
        assert_eq!(f("grok-4-1-fast-non-reasoning", None, true), None, "fast 系には送らない");
        assert_eq!(f("gpt-4o-mini", None, true), None, "他モデルには送らない");
        // LLM_EFFORT 明示は尊重、Grok の語彙 (low/medium/high) へ丸める。
        assert_eq!(f("grok-4.3", Some(Effort::Medium), true), Some("medium"));
        assert_eq!(f("grok-4.5", Some(Effort::XHigh), true), Some("high"), "xhigh は high へ丸め");
        assert_eq!(f("grok-4.5", Some(Effort::Max), true), Some("high"));

        // encode 経由でも wire に載る (対象モデルのみ)。
        let mut req = canonical::ChatRequest {
            model: "grok-4.3".into(),
            messages: user_msgs(),
            tools: vec![canonical::ToolSpec {
                name: EMIT_DELTA_TOOL.into(),
                description: "d".into(),
                parameters: state_delta_schema(),
            }],
            tool_choice: canonical::ToolChoice::Specific(EMIT_DELTA_TOOL.into()),
            temperature: None,
            max_tokens: 4096,
            effort: None,
        };
        let body = serde_json::to_value(openai_compat::encode(&req, ToolMode::Forced)).unwrap();
        assert_eq!(body["reasoning_effort"], "none");
        req.model = "gpt-4o-mini".into();
        let body = serde_json::to_value(openai_compat::encode(&req, ToolMode::Forced)).unwrap();
        assert!(body.get("reasoning_effort").is_none(), "非対象にはキーごと送らない");
    }

    // --- プロバイダ別の送り分け 3 軸 (2026-08-08) --------------------------------------
    //
    // OpenAI 互換は「メッセージの形」の互換であって「パラメータの寿命・実装範囲」ではない。
    // 同じ encode の中に、モデル名/ホストで割れる軸が 3 本ある:
    //   ① 出力上限の**欄名** (gpt-5/o 系は max_completion_tokens)
    //   ② reasoning_effort の**明示** (gpt-5 + tools は "none" を送らないと 400)
    //   ③ tool_choice の**実装範囲** (Meta は "auto" 一値のみ)
    // 3 本とも「ワイヤに何が現れるか」で凍結する。

    /// テスト用の canonical リクエスト (単一ツール強制 = 本番の構造化出力と同形)。
    fn compat_req(model: &str) -> canonical::ChatRequest {
        canonical::ChatRequest {
            model: model.into(),
            messages: user_msgs(),
            tools: vec![canonical::ToolSpec {
                name: EMIT_DELTA_TOOL.into(),
                description: "d".into(),
                parameters: state_delta_schema(),
            }],
            tool_choice: canonical::ToolChoice::Specific(EMIT_DELTA_TOOL.into()),
            temperature: None,
            max_tokens: 4096,
            effort: None,
        }
    }

    /// 【中継越しでも送り分けが効く】OpenRouter 等のアグリゲータは `vendor/model` 表記を使う。
    ///
    /// **送り分け 3 軸のうち 2 軸はモデル名で判定する**ので、接頭辞が付いた瞬間に素通りして
    /// 既定へ落ちる — `openai/gpt-5.6-luna` なら旧欄 `max_tokens` を送って 400 (#76)、
    /// 思考も明示しないので併用でまた 400 (#77)。**中継を挟むと、直接なら踏まない罠を
    /// 同じ順で踏み直す**。接頭辞が無いときの挙動は変えない (直接続の回帰なし)。
    #[test]
    fn send_splitting_survives_aggregator_model_prefixes() {
        use openai_compat::{reasoning_effort as effort, uses_max_completion_tokens as new_field};

        // 中継越しでも素の名前と同じ判定になること (左: 中継 / 右: 直接)。
        for (via_relay, direct) in [
            ("openai/gpt-5.6-luna", "gpt-5.6-luna"),
            ("openai/o3-mini", "o3-mini"),
            ("x-ai/grok-4.3", "grok-4.3"),
            ("meta-llama/llama-4-maverick", "llama-4-maverick"),
        ] {
            assert_eq!(new_field(via_relay), new_field(direct), "欄名: {via_relay}");
            assert_eq!(effort(via_relay, None, true), effort(direct, None, true), "思考: {via_relay}");
        }
        // 具体値も固定する (等値だけだと両方 false へ退行しても通ってしまう)。
        assert!(new_field("openai/gpt-5.6-luna"), "中継越しの gpt-5 も新欄を使う");
        assert_eq!(effort("openai/gpt-5.6-luna", None, true), Some("none"));
        assert_eq!(effort("x-ai/grok-4.3", None, true), Some("none"));

        // パス風のローカルモデル名では誤爆しない (剥がしてもどの族にも一致しない)。
        assert!(!new_field("models/foo.gguf"));
        assert_eq!(effort("models/foo.gguf", None, true), None);
    }

    /// 【軸① 出力上限の欄名】gpt-5 系 / o 系には `max_completion_tokens`、それ以外には
    /// `max_tokens`。**ワイヤには常に片方だけ**現れる。
    ///
    /// 旧欄は gpt-5 系 / o 系が 400 (`Unsupported parameter: 'max_tokens' is not supported
    /// with this model. Use 'max_completion_tokens' instead.`) で拒否し、新欄は知らない互換
    /// サーバ (llama.cpp / vLLM / gpt-oss / さくら) がある — **全面置換はできない**。
    #[test]
    fn token_limit_field_splits_by_model_family() {
        for reasoning in ["gpt-5.6-luna", "gpt-5.6-terra", "o3-mini"] {
            let body =
                serde_json::to_value(openai_compat::encode(&compat_req(reasoning), ToolMode::Forced))
                    .unwrap();
            assert_eq!(body["max_completion_tokens"], 4096, "{reasoning}");
            assert!(body.get("max_tokens").is_none(), "{reasoning}: 旧欄は 400 になる");
        }
        for legacy in ["gpt-4o-mini", "grok-4.3", "gpt-oss-120b", "Llama-4-Maverick"] {
            let body =
                serde_json::to_value(openai_compat::encode(&compat_req(legacy), ToolMode::Forced))
                    .unwrap();
            assert_eq!(body["max_tokens"], 4096, "{legacy}");
            assert!(
                body.get("max_completion_tokens").is_none(),
                "{legacy}: 新欄を知らない互換サーバが落ちる"
            );
        }
    }

    /// 【軸② gpt-5 の思考】ツールを送る周だけ `reasoning_effort: "none"` を**明示**する。
    ///
    /// **キーを省いても回避できない** — 省くとサーバ側の既定の思考が効き、
    /// `Function tools with reasoning_effort are not supported ... in /v1/chat/completions`
    /// で 400 になる (「黙っていることは値を決めていないことではない」)。
    /// **ツールを送らない周では触らない** — 制約は併用に掛かっており思考自体ではないので、
    /// 一律に止めるとツール無しの生成 (あらすじ要約) の思考まで殺す。
    #[test]
    fn gpt5_pins_effort_to_none_only_on_rounds_that_send_tools() {
        let mut req = compat_req("gpt-5.6-luna");
        req.effort = Some(Effort::High); // 利用者が段階を選んでいても、併用は許されない
        let body = serde_json::to_value(openai_compat::encode(&req, ToolMode::Forced)).unwrap();
        assert_eq!(body["tools"][0]["function"]["name"], EMIT_DELTA_TOOL);
        assert_eq!(body["reasoning_effort"], "none");

        // tools 無し (generate / あらすじ要約) では触らない = サーバ既定の思考を使う。
        req.tools.clear();
        let body = serde_json::to_value(openai_compat::encode(&req, ToolMode::Forced)).unwrap();
        assert!(body.get("tools").is_none());
        assert!(body.get("reasoning_effort").is_none(), "併用でなければ思考を殺さない");

        // ToolMode::Off (tools を送らない #29 経路) も同じ — ツールを送らないので触らない。
        let body = serde_json::to_value(openai_compat::encode(&compat_req("gpt-5.6-luna"), ToolMode::Off))
            .unwrap();
        assert!(body.get("reasoning_effort").is_none(), "no-tools 経路でも併用ではない");
    }

    /// 【軸③ tool_choice の実装範囲】`ToolMode::Auto` は**素の `"auto"`** を送り tools は残す。
    ///
    /// Meta (api.llama.com) は `only "auto" is supported for tool_choice. "none", "required",
    /// and named function choices are not currently supported` と名指しで 400 を返す
    /// (2026-08-08 実機)。強制が接地に降格するが、①提示するツールが emit_delta 1 本だけ
    /// ②GM_SYSTEM の提出行が mode 非依存 ③`parse::extract` のフェンス JSON フォールバック
    /// の三枚が受け皿になる。**`Off` へ落とすのは過剰** (実測 narration 量 tool-use = no-tools
    /// の 1.60× ＝ 品質のダウングレード)。
    #[test]
    fn auto_tool_mode_sends_bare_auto_and_keeps_the_tool() {
        let body =
            serde_json::to_value(openai_compat::encode(&compat_req("Llama-4-Maverick"), ToolMode::Auto))
                .unwrap();
        assert_eq!(body["tool_choice"], "auto", "オブジェクト形ではなく素の文字列");
        assert_eq!(body["tools"][0]["function"]["name"], EMIT_DELTA_TOOL, "定義は残す");

        // **強制できない = モデルがツールを使わない道を選べる**。実機 (Meta) はツール定義を
        // 見た上で content に JSON を書き、schema を知らないので ops を文字列にした。
        // ゆえに Auto では schema も prompt に載せる。ただし Off の文面 (「このサーバは
        // ツール呼び出しに対応していません」) を流用してはならない — ツール利用を自分で妨げる。
        let auto = openai_compat::encode(&compat_req("Llama-4-Maverick"), ToolMode::Auto);
        let hint = &auto.messages.last().expect("指示文が積まれること").content;
        assert!(hint.contains("JSON Schema") && hint.contains("narration"), "schema を載せる");
        assert!(hint.contains("emit_delta"), "まずツールで提出させる: {hint}");
        assert!(hint.contains("必ず配列"), "実機の崩れ (ops が文字列) を名指しで防ぐ: {hint}");
        assert!(
            !hint.contains("対応していません"),
            "Off の文面を流用しない (ツール利用を自分で妨げる): {hint}"
        );
        // 末尾に積む = 安定プレフィックスは動かない (キャッシュ影響なし)。
        assert_eq!(auto.messages[0].role, Role::System, "先頭の静的 system は不動");
        assert_eq!(auto.messages.last().unwrap().role, Role::System);

        // Forced は従来どおり名前指定 (回帰なし)。
        let forced =
            serde_json::to_value(openai_compat::encode(&compat_req("gpt-4o-mini"), ToolMode::Forced))
                .unwrap();
        assert_eq!(forced["tool_choice"]["type"], "function");
        assert_eq!(forced["tool_choice"]["function"]["name"], EMIT_DELTA_TOOL);

        // ツールを持たない generate では Auto でも tool_choice を送らない (送る相手がいない)。
        let mut plain = compat_req("Llama-4-Maverick");
        plain.tools.clear();
        let body = serde_json::to_value(openai_compat::encode(&plain, ToolMode::Auto)).unwrap();
        assert!(body.get("tool_choice").is_none());
    }

    /// 【ToolMode の決定】明示 (`LLM_TOOL_MODE`) > 旧キー (`LLM_USE_TOOLS=false`) > base_url 自動。
    ///
    /// **旧キーの `true` は自動判定を上書きしない** — あれは「tools を使う」の意味しか持たず
    /// 名前指定が通るかは言っていない。既存 .env が `LLM_USE_TOOLS=true` のまま Meta を指しても
    /// 正しく `Auto` に落ちる (受領者が設定を書き換えずに済む)。
    #[test]
    fn tool_mode_resolution_prefers_explicit_then_legacy_then_host() {
        use config::resolve_tool_mode as r;
        let meta = "https://api.llama.com/compat/v1";
        let openai = "https://api.openai.com/v1";

        // 自動判定: ホストで割る (モデル名では割らない — 拒否はサーバの実装範囲であって
        // モデルの性質ではなく、中継越しの同じモデルでは名前指定が通る)。
        assert_eq!(r(None, None, meta).unwrap(), ToolMode::Auto);
        assert_eq!(r(None, None, openai).unwrap(), ToolMode::Forced);
        assert_eq!(r(None, None, "http://localhost:8080/v1").unwrap(), ToolMode::Forced);

        // 旧キー: false 系だけを見る。true は自動判定に委ねる。
        assert_eq!(r(None, Some("false".into()), openai).unwrap(), ToolMode::Off);
        assert_eq!(r(None, Some("off".into()), openai).unwrap(), ToolMode::Off);
        assert_eq!(r(None, Some("true".into()), meta).unwrap(), ToolMode::Auto, "true は上書きしない");

        // 明示は両方に勝つ。誤値は起動時に弾く (ネットワーク前)。
        assert_eq!(r(Some("forced".into()), Some("false".into()), meta).unwrap(), ToolMode::Forced);
        assert_eq!(r(Some("auto".into()), None, openai).unwrap(), ToolMode::Auto);
        assert_eq!(r(Some("off".into()), None, openai).unwrap(), ToolMode::Off);
        assert!(r(Some("required".into()), None, openai).is_err(), "語彙外は Config エラー");
    }

    /// 【400 からの自己降格】`tool_choice` を名指しした 400 で一段降格する (`Forced→Auto→Off`)。
    ///
    /// ホスト名の自動判定は**既知の口しか救えない** — 中継や自前プロキシ越しの Meta は
    /// base_url から判別できないので、実際の拒否から学ぶ経路を残す。判定を部分文字列一致に
    /// するのは互換サーバのエラー JSON の形が揃っていないため (`param` を持たないものがある)。
    /// 誤検知の代償は「一段降格して 1 回再送する」だけ。
    #[test]
    fn tool_choice_rejection_walks_down_the_capability_ladder() {
        // 実機の 400 本文 (2026-08-08)。
        let meta_400 = r#"{"error":{"code":null,"message":"only \"auto\" is supported for `tool_choice`. \"none\", \"required\", and named function choices are not currently supported","param":"tool_choice","type":"invalid_request_error"}}"#;
        assert!(openai_compat::blames_tool_choice(meta_400));
        // 無関係な 400 で降格させない (発火条件が緩すぎると余計な往復と品質低下を招く)。
        assert!(!openai_compat::blames_tool_choice(
            r#"{"error":{"message":"Unsupported parameter: 'max_tokens'","param":"max_tokens"}}"#
        ));

        // ラダーは一方向で、底 (Off) から先は無い = 無限降格しない。
        assert_eq!(ToolMode::Forced.downgrade(), Some(ToolMode::Auto));
        assert_eq!(ToolMode::Auto.downgrade(), Some(ToolMode::Off));
        assert_eq!(ToolMode::Off.downgrade(), None, "底では降格せず 400 をそのまま返す");
    }

    /// 【出力上限の検出 (2026-08-08 で EmptyResponse から分離)】text 空 ∧ tool_calls 空 ∧
    /// finish==length だけを `OutputTruncated` にする。**一過性にしない** — 上限は入力に対して
    /// 決定的なので再送すれば同じ所で切れる (リトライはバックオフと課金だけを増やし、画面には
    /// 「空の応答」としか出ないので受領者が上限に辿り着けなかった)。次の一手が
    /// 「LLM_MAX_TOKENS をいくつまで上げるか」なので、現在値を文面に載せる。
    /// length 以外の空応答は従来どおり素通し (generate/extract が非リトライで surface)。
    #[test]
    fn output_truncation_is_not_retried_and_names_the_limit() {
        let length_empty = client::decode_chat_body(
            r#"{"choices":[{"finish_reason":"length","message":{"content":""}}]}"#.to_string(),
        )
        .unwrap();
        let resp = openai_compat::decode(length_empty).unwrap();
        let err = openai_compat::reject_empty_reasoning(resp, 4096).unwrap_err();
        assert!(matches!(err, LlmError::OutputTruncated { limit: 4096 }));
        assert!(!err.is_transient(), "同じ入力なら同じ所で切れる = 再試行に乗せない");
        assert!(err.to_string().contains("4096"), "上げ幅を決められるよう現在値を出す: {err}");

        // finish が length でない空応答は弾かない (従来の経路のまま)。
        let stop_empty = client::decode_chat_body(
            r#"{"choices":[{"finish_reason":"stop","message":{"content":""}}]}"#.to_string(),
        )
        .unwrap();
        let resp = openai_compat::decode(stop_empty).unwrap();
        assert!(openai_compat::reject_empty_reasoning(resp, 4096).is_ok());

        // 本文か tool_calls があれば length でも正常 (途中切れは呼び出し側の解釈に任せる)。
        let with_text = client::decode_chat_body(
            r#"{"choices":[{"finish_reason":"length","message":{"content":"途中まで"}}]}"#
                .to_string(),
        )
        .unwrap();
        let resp = openai_compat::decode(with_text).unwrap();
        assert!(openai_compat::reject_empty_reasoning(resp, 4096).is_ok());
    }

    // --- Gemini ネイティブ経路 (spec 12 Phase C) --------------------------------------

    /// 【Phase C 判定】Provider 三値化の境界。Gemini ネイティブは
    /// generativelanguage.googleapis.com を含み **`/openai` を含まない**時のみ —
    /// OpenAI 互換エンドポイント (`.../v1beta/openai/`) の既存利用者を壊さない (rev4 の罠)。
    /// 判定不能なプロキシホストは OpenAiCompat に落ちる (安全側)。
    #[test]
    fn provider_detects_gemini_native_but_not_compat_endpoint() {
        assert_eq!(
            Provider::detect("https://generativelanguage.googleapis.com"),
            Provider::Gemini
        );
        assert_eq!(
            Provider::detect("https://generativelanguage.googleapis.com/v1beta"),
            Provider::Gemini
        );
        assert_eq!(
            Provider::detect("https://generativelanguage.googleapis.com/v1beta/openai/"),
            Provider::OpenAiCompat,
            "互換エンドポイント利用者を壊さない"
        );
        assert_eq!(
            Provider::detect("https://my-gemini-proxy.example.com/v1"),
            Provider::OpenAiCompat,
            "判定不能ホストは安全側 (明示 LLM_PROVIDER=gemini を誘導)"
        );

        // endpoint はホスト直 / /v1beta 込みの両方の base_url を受ける (受領者ゼロ設定)。
        let mut cfg = LlmConfig::new(
            "https://generativelanguage.googleapis.com",
            "sk",
            "gemini-2.5-flash",
        );
        assert_eq!(cfg.provider, Provider::Gemini);
        assert_eq!(
            cfg.gemini_endpoint(),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent"
        );
        cfg.base_url = "https://generativelanguage.googleapis.com/v1beta/".into();
        assert_eq!(
            cfg.gemini_endpoint(),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent",
            "/v1beta を二重に付けない"
        );
    }

    /// 【Phase C encode】canonical → generateContent。system は systemInstruction (D3)、
    /// assistant は role "model"、単一ツール強制は mode ANY + allowedFunctionNames (K2)、
    /// キーは camelCase。temperature None なら送らない。
    #[test]
    fn gemini_encode_maps_system_tools_and_roles() {
        let mut msgs = user_msgs();
        msgs.push(ChatMessage::assistant("以前の語り"));
        msgs.push(ChatMessage::user("続き"));
        let req = canonical::ChatRequest {
            model: "gemini-2.5-flash".into(),
            messages: msgs,
            tools: vec![canonical::ToolSpec {
                name: EMIT_DELTA_TOOL.into(),
                description: "d".into(),
                parameters: state_delta_schema(),
            }],
            tool_choice: canonical::ToolChoice::Specific(EMIT_DELTA_TOOL.into()),
            temperature: None,
            max_tokens: 4096,
            effort: None,
        };
        let body = serde_json::to_value(gemini::encode(&req)).unwrap();

        // system は systemInstruction へ (model ターンに畳まない = D3)。
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "あなたはGM");
        let contents = body["contents"].as_array().unwrap();
        let roles: Vec<&str> = contents.iter().map(|c| c["role"].as_str().unwrap()).collect();
        assert_eq!(roles, vec!["user", "model", "user"], "assistant は model ロール");

        // 単一ツール強制 = ANY + allowedFunctionNames (K2 の写像)。
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["name"],
            EMIT_DELTA_TOOL
        );
        assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "ANY");
        assert_eq!(
            body["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
            EMIT_DELTA_TOOL
        );

        // camelCase + temperature None は送らない。
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 4096);
        assert!(body["generationConfig"].get("temperature").is_none());

        // 【Phase C.5a (#52)】schema は Gemini サブセットへ適応 — oneOf は黙って落とされ
        // 制約が消える (実測: ops:[1,2,3] 捏造) ため anyOf へ付け替え、バリアント制約を保つ。
        let params = serde_json::to_string(&body["tools"][0]["functionDeclarations"][0]["parameters"]).unwrap();
        assert!(!params.contains("oneOf"), "oneOf は Gemini に送らない: {params}");
        assert!(params.contains("anyOf"), "anyOf へ付け替えてバリアント制約を保つ");
        assert!(params.contains("set_flag"), "op バリアントの実体は保たれる");
    }

    /// 【spec 13 Phase A】静的プレフィックス fingerprint: 同プレフィックス→同 key (可変 user は
    /// 無関係)、system/model/tools のどれが変わっても別 key (campaign 遷移や別 package で明示
    /// キャッシュを作り直すため)。
    #[test]
    fn gemini_fingerprint_keys_static_prefix_only() {
        let mk = |sys: &str, model: &str, tool_desc: &str, action: &str| canonical::ChatRequest {
            model: model.into(),
            messages: vec![ChatMessage::system(sys), ChatMessage::user(action)],
            tools: vec![canonical::ToolSpec {
                name: EMIT_DELTA_TOOL.into(),
                description: tool_desc.into(),
                parameters: state_delta_schema(),
            }],
            tool_choice: canonical::ToolChoice::Specific(EMIT_DELTA_TOOL.into()),
            temperature: None,
            max_tokens: 4096,
            effort: None,
        };
        let base = mk("GM_A", "gemini-3.5-flash", "d", "行動X");
        // 可変 user が違っても fingerprint は不変 (cache は静的プレフィックスだけを含む)。
        assert_eq!(
            gemini::fingerprint(&base),
            gemini::fingerprint(&mk("GM_A", "gemini-3.5-flash", "d", "全然ちがう行動Y")),
            "user メッセージは fingerprint に影響しない"
        );
        // system / model / tools のどれが変わっても別 key。
        assert_ne!(
            gemini::fingerprint(&base),
            gemini::fingerprint(&mk("GM_B", "gemini-3.5-flash", "d", "行動X")),
            "system 変化 = 別シナリオ → 別 key"
        );
        assert_ne!(
            gemini::fingerprint(&base),
            gemini::fingerprint(&mk("GM_A", "gemini-2.5-flash", "d", "行動X")),
            "model 変化 → 別 key (cache は model 固有)"
        );
        assert_ne!(
            gemini::fingerprint(&base),
            gemini::fingerprint(&mk("GM_A", "gemini-3.5-flash", "d2", "行動X")),
            "tools 変化 → 別 key"
        );
    }

    /// 【spec 13 Phase A】`cached=None` は従来 encode と完全一致 (回帰ゼロ)。`Some(name)` は
    /// systemInstruction/tools を送らず cachedContent を参照し、可変 contents と mode ANY (強制
    /// 指定) は request 側に残す。
    #[test]
    fn gemini_encode_with_cache_omits_prefix_and_references_cache() {
        let req = canonical::ChatRequest {
            model: "gemini-3.5-flash".into(),
            messages: user_msgs(),
            tools: vec![canonical::ToolSpec {
                name: EMIT_DELTA_TOOL.into(),
                description: "d".into(),
                parameters: state_delta_schema(),
            }],
            tool_choice: canonical::ToolChoice::Specific(EMIT_DELTA_TOOL.into()),
            temperature: None,
            max_tokens: 4096,
            effort: None,
        };
        // None = 従来 body と同一 (cached_content は skip される)。
        assert_eq!(
            serde_json::to_value(gemini::encode(&req)).unwrap(),
            serde_json::to_value(gemini::encode_with_cache(&req, None)).unwrap(),
            "cached None は従来 encode と完全一致"
        );
        // Some = プレフィックス省略 + cache 参照。
        let body =
            serde_json::to_value(gemini::encode_with_cache(&req, Some("cachedContents/xyz".into())))
                .unwrap();
        assert_eq!(body["cachedContent"], "cachedContents/xyz");
        assert!(body.get("systemInstruction").is_none(), "system は cache 側 (二重送信しない)");
        assert!(body.get("tools").is_none(), "tool 宣言も cache 側");
        // Gemini は cachedContent 参照時に tool_config を request に載せると 400 (Phase D live)。
        assert!(body.get("toolConfig").is_none(), "強制指定 (mode ANY) も cache 側 — request には出さない");
        assert!(
            !body["contents"].as_array().unwrap().is_empty(),
            "可変 contents は request に残る"
        );
    }

    /// 【spec 14 Phase A / D4】Gemini は **1 本目**の leading system だけを静的
    /// (systemInstruction / cachedContent / fingerprint / サイズゲート) として扱い、
    /// 2 本目以降 (append-only synopsis) は **inline contents (user) へ非キャッシュで**送る —
    /// synopsis を pin すると章追加毎に fingerprint が変わり cachedContent を再作成する
    /// storage churn になる (D1 の「synopsis を安定 message にした」意図と逆)。
    #[test]
    fn gemini_excludes_second_leading_system_from_cache() {
        let mk = |synopsis: &str| canonical::ChatRequest {
            model: "gemini-3.5-flash".into(),
            messages: vec![
                ChatMessage::system("あなたはGM"),
                ChatMessage::system(synopsis),
                ChatMessage::user("行動"),
            ],
            tools: vec![canonical::ToolSpec {
                name: EMIT_DELTA_TOOL.into(),
                description: "d".into(),
                parameters: state_delta_schema(),
            }],
            tool_choice: canonical::ToolChoice::Specific(EMIT_DELTA_TOOL.into()),
            temperature: None,
            max_tokens: 4096,
            effort: None,
        };
        let req = mk("# これまでのあらすじ\n第一章: 村に着いた。");

        // systemInstruction は 1 本目のみ、2 本目は contents 先頭に user として inline。
        let body = serde_json::to_value(gemini::encode(&req)).unwrap();
        let parts = body["systemInstruction"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1, "静的 (1 本目) だけが systemInstruction: {parts:?}");
        assert_eq!(parts[0]["text"], "あなたはGM");
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents[0]["role"], "user", "synopsis は inline contents へ降格");
        assert!(
            contents[0]["parts"][0]["text"].as_str().unwrap().contains("これまでのあらすじ"),
            "synopsis 本文が inline で残る: {contents:?}"
        );

        // fingerprint / サイズゲートは synopsis 非依存 (章追加で cachedContent を再 pin しない)。
        assert_eq!(
            gemini::fingerprint(&req),
            gemini::fingerprint(&mk("# これまでのあらすじ\n第一章…第二章: 祠に入った。")),
            "synopsis の増減は fingerprint に影響しない"
        );
        assert_eq!(
            gemini::static_prefix_chars(&req),
            gemini::static_prefix_chars(&mk("倍以上に伸びた synopsis ののび太ののび太ののび太")),
            "サイズゲートも静的 (1 本目) のみで測る"
        );

        // cachedContents create body にも synopsis は入らない。
        let create = serde_json::to_value(gemini::build_create_request(&req, 900)).unwrap();
        let create_parts = create["systemInstruction"]["parts"].as_array().unwrap();
        assert_eq!(create_parts.len(), 1, "cache に pin するのは静的だけ");
        assert_eq!(create_parts[0]["text"], "あなたはGM");

        // cachedContent 参照時も synopsis は可変 contents として request に残る。
        let cached =
            serde_json::to_value(gemini::encode_with_cache(&req, Some("cachedContents/x".into())))
                .unwrap();
        assert!(
            cached["contents"][0]["parts"][0]["text"]
                .as_str()
                .unwrap()
                .contains("これまでのあらすじ"),
            "cache 参照時も synopsis は inline: {cached:?}"
        );
    }

    /// 【spec 13 Phase B】cache 判定: 無効/サイズゲート未満は Bypass、handle 無し(十分大)は Create、
    /// fingerprint 一致は Reuse、不一致(scenario 変化)は Create(作り直し)。純関数 (HTTP 非依存)。
    #[test]
    fn gemini_decide_cache_action_reuse_create_bypass() {
        use gemini::{decide_cache_action, CacheAction, CacheHandle};
        let (big, small, min) = (5000usize, 100usize, 4000usize);
        assert!(matches!(decide_cache_action(false, min, big, 1, None), CacheAction::Bypass), "無効化");
        assert!(matches!(decide_cache_action(true, min, small, 1, None), CacheAction::Bypass), "サイズゲート未満");
        assert!(matches!(decide_cache_action(true, min, big, 1, None), CacheAction::Create), "handle 無し→作成");
        let h = CacheHandle { name: "cachedContents/x".into(), fingerprint: 42 };
        match decide_cache_action(true, min, big, 42, Some(&h)) {
            CacheAction::Reuse(n) => assert_eq!(n, "cachedContents/x", "fp 一致→既存を参照"),
            other => panic!("Reuse を期待: {other:?}"),
        }
        assert!(
            matches!(decide_cache_action(true, min, big, 99, Some(&h)), CacheAction::Create),
            "fp 不一致 (別シナリオ)→作り直し"
        );
    }

    /// 【spec 13 Phase B】cachedContents create body: model は `models/` プレフィックス必須、
    /// 静的プレフィックス (systemInstruction + tools) を含み ttl を秒形式で、**可変 contents は含めない**。
    #[test]
    fn gemini_build_create_request_extracts_static_prefix() {
        let req = canonical::ChatRequest {
            model: "gemini-3.5-flash".into(),
            messages: user_msgs(),
            tools: vec![canonical::ToolSpec {
                name: EMIT_DELTA_TOOL.into(),
                description: "d".into(),
                parameters: state_delta_schema(),
            }],
            tool_choice: canonical::ToolChoice::Specific(EMIT_DELTA_TOOL.into()),
            temperature: None,
            max_tokens: 4096,
            effort: None,
        };
        let create = serde_json::to_value(gemini::build_create_request(&req, 900)).unwrap();
        assert_eq!(create["model"], "models/gemini-3.5-flash", "create は models/ プレフィックス必須");
        assert_eq!(create["ttl"], "900s");
        assert_eq!(create["systemInstruction"]["parts"][0]["text"], "あなたはGM", "静的 system を含む");
        assert_eq!(
            create["tools"][0]["functionDeclarations"][0]["name"], EMIT_DELTA_TOOL,
            "tool 宣言を含む"
        );
        // 強制指定 (mode ANY) も cache に載せる (Gemini は request 側 tool_config を 400 で拒否)。
        assert_eq!(
            create["toolConfig"]["functionCallingConfig"]["mode"], "ANY",
            "tool_config も cache 側"
        );
        assert!(create.get("contents").is_none(), "可変 contents は cache に含めない");
        assert!(gemini::static_prefix_chars(&req) > 100, "静的プレフィックスの文字数を測れる");
    }

    /// 【Phase C decode】functionCall.args (最初からオブジェクト = D2 恒等) を canonical へ、
    /// id は **client 単位の単調 seq** から `call_{seq}_{index}` を合成 (rev4 Must 4 —
    /// リクエスト毎リセットの call_0 は却下→再生成で衝突する)。usage の
    /// cachedContentTokenCount → cache_read、MAX_TOKENS → Finish::Length。
    #[test]
    fn gemini_decode_synthesizes_ids_and_maps_usage() {
        let body = r#"{
            "candidates": [{
                "content": {"parts": [
                    {"functionCall": {"name": "emit_delta",
                                      "args": {"narration": "霧が晴れる", "ops": []}}}
                ], "role": "model"},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 900, "candidatesTokenCount": 40,
                              "cachedContentTokenCount": 700}
        }"#;
        let resp = client::decode_gemini_body(body.to_string()).unwrap();
        let a = gemini::decode(resp.clone(), 5);
        assert_eq!(a.tool_calls[0].id, "call_5_0", "seq + index で合成");
        assert_eq!(a.tool_calls[0].name, "emit_delta");
        assert_eq!(a.finish, canonical::Finish::ToolUse, "functionCall があれば tool_use 扱い");
        assert_eq!(a.usage.cache_read, 700, "暗黙キャッシュの計数を正規化");
        let delta: StateDelta = parse::extract(&a).unwrap();
        assert_eq!(delta.narration, "霧が晴れる");

        // 別リクエスト (seq 進行) では id が衝突しない。
        let b = gemini::decode(resp, 6);
        assert_ne!(a.tool_calls[0].id, b.tool_calls[0].id);

        // MAX_TOKENS → Length (empty-response 防御 Phase D の判定材料)。
        let cut = client::decode_gemini_body(
            r#"{"candidates":[{"finishReason":"MAX_TOKENS"}]}"#.to_string(),
        )
        .unwrap();
        assert_eq!(gemini::decode(cut, 0).finish, canonical::Finish::Length);
    }

    /// 【ブロック理由の surface (2026-07-18 実プレイ発見)】Gemini は安全フィルタで弾いた時も
    /// **200 + 空応答**で返す — 従来 decode は promptFeedback.blockReason を deserialize すら
    /// せず、候補段の finishReason (SAFETY/RECITATION 等) も Other に潰していたため、何で
    /// 弾かれても一律「LLM が空の応答を返した」になり診断不能だった (あらすじ要約の恒久失敗)。
    /// プロンプト段 / 候補段の両方から理由を拾い、Blocked (非一過性 = 同じ内容の再送では
    /// 回復しない) として surface する。本文か functionCall が在る応答・MAX_TOKENS は対象外。
    #[test]
    fn gemini_block_reason_surfaces_safety_and_prompt_feedback() {
        // プロンプト段: promptFeedback.blockReason (candidates 自体が無い)。
        let p = client::decode_gemini_body(
            r#"{"promptFeedback":{"blockReason":"PROHIBITED_CONTENT"}}"#.to_string(),
        )
        .unwrap();
        assert_eq!(gemini::block_reason(&p).as_deref(), Some("PROHIBITED_CONTENT"));

        // 候補段: 本文ゼロ + finishReason=SAFETY。
        let c = client::decode_gemini_body(
            r#"{"candidates":[{"finishReason":"SAFETY"}]}"#.to_string(),
        )
        .unwrap();
        assert_eq!(gemini::block_reason(&c).as_deref(), Some("SAFETY"));

        // 本文がある応答 (STOP)・思考使い切り (MAX_TOKENS) はブロックではない
        // (後者は EmptyResponse 一過性昇格 = 再抽選の管轄、Phase D)。
        let ok = client::decode_gemini_body(
            r#"{"candidates":[{"content":{"parts":[{"text":"了解"}]},"finishReason":"STOP"}]}"#
                .to_string(),
        )
        .unwrap();
        assert!(gemini::block_reason(&ok).is_none());
        let cut = client::decode_gemini_body(
            r#"{"candidates":[{"finishReason":"MAX_TOKENS"}]}"#.to_string(),
        )
        .unwrap();
        assert!(gemini::block_reason(&cut).is_none());

        // Blocked は非一過性 (リトライで回復しない = 無駄な再送をしない)。
        assert!(!LlmError::Blocked { reason: "SAFETY".into() }.is_transient());
    }

    // --- OpenAI 互換経路のキャッシュ計測 + xAI sticky routing (#45) ------------------

    /// 【互換 usage のパース】OpenAI/xAI/Gemini 互換の `usage.prompt_tokens_details.cached_tokens`
    /// を読める (xAI の Green 判定 = cached > 0)。usage が無い/形が違うサーバでも壊れない。
    #[test]
    fn compat_usage_cached_tokens_parse() {
        let body = r#"{
            "choices": [{"message": {"content": "了解"}}],
            "usage": {"prompt_tokens": 9000, "completion_tokens": 120,
                      "prompt_tokens_details": {"cached_tokens": 8100}}
        }"#;
        let resp = client::decode_chat_body(body.to_string()).unwrap();
        let usage = resp.usage.expect("usage をパースする");
        assert_eq!(usage.prompt_tokens_details.unwrap().cached_tokens, 8100);
        assert_eq!(usage.prompt_tokens, 9000);

        // usage 無し (ローカル互換サーバ等) でも従来どおり decode できる。
        let plain = client::decode_chat_body(
            r#"{"choices":[{"message":{"content":"ok"}}]}"#.to_string(),
        )
        .unwrap();
        assert!(plain.usage.is_none());
        assert_eq!(plain.choices[0].message.content.as_deref(), Some("ok"));
    }

    /// 【会話 ID】クライアント毎に一意な conv_id を持つ (xAI のキャッシュはサーバ単位 →
    /// x-grok-conv-id で同一サーバに sticky routing しないと同一プレフィックスでも miss)。
    #[test]
    fn conv_id_is_unique_per_client_and_stable_within() {
        let cfg = LlmConfig::new("https://api.x.ai/v1", "sk", "grok-4");
        let a = LlmClient::new(cfg.clone()).unwrap();
        let b = LlmClient::new(cfg).unwrap();
        assert!(!a.conv_id().is_empty());
        assert_ne!(a.conv_id(), b.conv_id(), "クライアント (=セッション) 毎に別 ID");
        assert_eq!(a.conv_id(), a.conv_id(), "同一クライアント内では不変");
    }

    // --- キャッシュ健全性の計測 (GUI 警告の材料) ------------------------------------

    /// 【CacheStat の計数則】miss で連続 miss が伸び、1 回のヒットで 0 にリセットされる
    /// (GUI の「キャッシュ経路が壊れているかも」警告 = total>=2 かつ consecutive_misses>=3 の材料)。
    /// 初回リクエストは書き込みゆえ miss が正常 — total_requests で判定側が除外できる。
    /// 【spec 14 Phase C】累積 hit_tokens/total_tokens (hit rate = cached/prompt、
    /// cache_creation は cached に含めない定義 = D5) も同時に積む。
    #[test]
    fn cache_stat_counts_misses_and_resets_on_hit() {
        let mut s = CacheStat::default();
        assert_eq!((s.total_requests, s.consecutive_misses, s.last_cache_read), (0, 0, 0));

        s.record(0, 100); // 初回 = 書き込み (miss が正常)
        s.record(0, 110);
        s.record(0, 120);
        assert_eq!(s.total_requests, 3);
        assert_eq!(s.consecutive_misses, 3, "連続 miss が積み上がる");
        assert_eq!(s.last_cache_read, 0);

        s.record(8100, 9000); // ヒットで復帰
        assert_eq!(s.consecutive_misses, 0, "1 回のヒットで連続 miss はリセット");
        assert_eq!(s.last_cache_read, 8100);
        assert_eq!(s.total_requests, 4);

        s.record(0, 200); // 再び miss — 1 から数え直し
        assert_eq!(s.consecutive_misses, 1);

        // spec 14 D5: 累積 = セッション全体の hit rate の分子/分母。
        assert_eq!(s.hit_tokens, 8100, "累積 cached = cache_read の総和");
        assert_eq!(s.total_tokens, 100 + 110 + 120 + 9000 + 200, "累積 prompt = input の総和");
    }

    /// 【spec 14 Phase C】per-request のリングバッファは**有界** — 長セッション (100 ターン超)
    /// で常駐メモリが伸びないよう直近 N 件だけ保持し、古い方から捨てる (曲線の可視化用)。
    #[test]
    fn cache_stat_recent_ring_buffer_is_bounded() {
        let mut s = CacheStat::default();
        for i in 0..(CacheStat::RECENT_CAP as u64 + 8) {
            s.record(i, 100 + i);
        }
        assert_eq!(s.recent.len(), CacheStat::RECENT_CAP, "上限で頭打ち (無制限履歴は持たない)");
        // 最古の 8 件が落ち、末尾は最新の記録。
        assert_eq!(s.recent.first().map(|p| p.cached), Some(8), "古い方から捨てる");
        let last = s.recent.last().unwrap();
        assert_eq!(
            (last.cached, last.prompt),
            (CacheStat::RECENT_CAP as u64 + 7, 100 + CacheStat::RECENT_CAP as u64 + 7),
            "末尾 = 直近のリクエスト"
        );
    }

    /// 【クライアントのスナップショット】新規クライアントの cache_stat は零値
    /// (リクエストを一度もしていない = 警告判定は発火しない)。clone スナップショットで
    /// lock を持ち帰らない (GUI が毎ターン読める)。
    #[test]
    fn client_cache_stat_starts_at_zero() {
        let cfg = LlmConfig::new("https://api.anthropic.com/v1", "sk", "claude-opus-4-8");
        let client = LlmClient::new(cfg).unwrap();
        let s = client.cache_stat();
        assert_eq!(s.total_requests, 0);
        assert_eq!(s.consecutive_misses, 0);
        assert_eq!(s.last_cache_read, 0);
        assert_eq!((s.hit_tokens, s.total_tokens), (0, 0), "累積も零値 (spec 14)");
        assert!(s.recent.is_empty(), "リングバッファも空");
    }

    /// 【stray system の降格】ネイティブは先頭 system のみ対応 → 先頭以外の system
    /// (no-tools の json_instruction 等が万一混じった場合) は user に落として壊さない。
    #[test]
    fn anthropic_demotes_non_leading_system_to_user() {
        let msgs = vec![
            ChatMessage::system("GM人格"),
            ChatMessage::system("盤面"),
            ChatMessage::user("行動"),
            ChatMessage::system("後付け指示"),
        ];
        let req = anthropic::encode(&canonical::ChatRequest {
            model: "m".into(),
            messages: msgs,
            tools: Vec::new(),
            tool_choice: canonical::ToolChoice::None,
            temperature: None,
            max_tokens: 256,
            effort: None,
        });
        let body = serde_json::to_value(&req).unwrap();
        assert_eq!(body["system"].as_array().unwrap().len(), 2, "先頭連続 system は system ブロックへ");
        let roles: Vec<&str> = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, vec!["user", "user"], "先頭以外の system は user に降格");
    }

    // --- Responses ワイヤ (Perplexity Agent API `/v1/responses`、2026-08-20) ------------

    /// 【Provider 自動判定】`api.perplexity.ai` には `/chat/completions` の口が**無い**
    /// (404・本文なし = アプリの実観測・Fuseforks 2026-08-19 と同じ)。互換の口は
    /// `/router/v1/chat/completions` (有料クレジット要) で、鍵だけで使えるのは
    /// `/v1/responses` (Agent API の OpenAI Responses 互換別名)。ホストで判定し、
    /// `/router/` を含む base_url は互換のまま (Gemini の `/openai` 例外と同じ形)。
    #[test]
    fn provider_detects_perplexity_as_responses_but_router_as_compat() {
        assert_eq!(Provider::detect("https://api.perplexity.ai/v1"), Provider::Responses);
        assert_eq!(Provider::detect("https://api.perplexity.ai"), Provider::Responses);
        assert_eq!(
            Provider::detect("https://api.perplexity.ai/router/v1"),
            Provider::OpenAiCompat,
            "Router API は chat/completions 互換 — 互換利用者を壊さない"
        );
        let mut cfg = LlmConfig::new("https://api.perplexity.ai/v1/", "sk", "perplexity/deepseek-v4-flash-0731");
        assert_eq!(cfg.provider, Provider::Responses);
        assert_eq!(cfg.responses_endpoint(), "https://api.perplexity.ai/v1/responses");
        cfg.base_url = "https://api.perplexity.ai".into();
        assert_eq!(
            cfg.responses_endpoint(),
            "https://api.perplexity.ai/v1/responses",
            "ホスト直でも /v1 を補う (受領者ゼロ設定)"
        );
        // 明示指定の語彙。
        assert_eq!(
            Provider::parse_env("responses", "LLM_PROVIDER").unwrap(),
            Provider::Responses
        );
        assert_eq!(
            Provider::parse_env("perplexity", "LLM_PROVIDER").unwrap(),
            Provider::Responses
        );
    }

    /// 【Provider 自動判定 / Meta】`api.meta.ai` は互換口も持つが、**キャッシュ計測は
    /// Responses 口 (`/v1/responses`) でだけ確実に返る** (2026-08-21 probe: 互換経路は
    /// cached 不安定 (2026-08-09 実測)・Responses は input 1107 中 cached 1073。Fuseforks
    /// spec 37 と同一の実測)。muse-spark 系は TRPG 勢の主力プロバイダで、キャッシュ警告の
    /// 誤発火は受領者の不安に直結する → 口ごと Responses へ。api.llama.com は未測ゆえ従来。
    #[test]
    fn provider_detects_meta_as_responses() {
        assert_eq!(Provider::detect("https://api.meta.ai/v1/"), Provider::Responses);
        assert_eq!(Provider::detect("https://api.meta.ai"), Provider::Responses);
        assert_eq!(
            Provider::detect("https://api.llama.com/compat/v1"),
            Provider::OpenAiCompat,
            "llama.com の Responses 口は未測 — 互換のまま"
        );
    }

    /// 【CacheStat の床】キャッシュ最小プレフィックス未満のリクエストは**構造的にヒット
    /// 不可能** (Perplexity 8192 ブロック床 #80 / Anthropic 最小 4096) なので、miss として
    /// 数えない — 数えると GUI 警告が「壊れてもいないのに」鳴る (実害: TRPG 主力の安価
    /// プロバイダでユーザーが不安になる)。ヒット時のリセット・累積・リングは従来どおり。
    #[test]
    fn cache_stat_floor_gates_miss_counting() {
        let mut s = CacheStat { floor: 8192, ..CacheStat::default() };
        s.record(0, 7000); // 床未満 — キャッシュ対象外のサイズ
        s.record(0, 7100);
        s.record(0, 7200);
        assert_eq!(s.consecutive_misses, 0, "床未満の miss は数えない (警告が鳴らない)");
        assert_eq!(s.total_requests, 3, "リクエスト数と累積は正直に積む");
        assert_eq!(s.total_tokens, 7000 + 7100 + 7200);
        s.record(0, 9000); // 床以上の miss は真の miss
        assert_eq!(s.consecutive_misses, 1);
        s.record(8200, 9000); // ヒットでリセット (床に関係なく)
        assert_eq!(s.consecutive_misses, 0);
        // 既定 floor=0 は従来挙動 (全 miss を数える) — 既存テストが保証。
        // floor=MAX = キャッシュ不安定プロバイダ (Meta): miss を一切数えない。
        let mut m = CacheStat { floor: u64::MAX, ..CacheStat::default() };
        m.record(0, 100_000);
        assert_eq!(m.consecutive_misses, 0, "不安定プロバイダは警告の対象外");
        m.record(500, 1000);
        assert_eq!(m.last_cache_read, 500, "ヒットは正直に載る");
    }

    /// 【cache_floor の表】実測/公式で確定した床だけを持つ (盛ると真の miss を隠す)。
    #[test]
    fn cache_floor_table_matches_measured_providers() {
        let f = |url: &str| client::cache_floor(&LlmConfig::new(url, "k", "m"));
        assert_eq!(f("https://api.perplexity.ai/v1"), 8192, "#80 ブロック床");
        assert_eq!(f("https://api.meta.ai/v1/"), u64::MAX, "不安定 — miss を数えない");
        assert_eq!(f("https://api.anthropic.com"), 4096, "#44 公式最小");
        assert_eq!(f("https://api.x.ai/v1"), 0, "未確定は従来どおり全 miss を数える");
    }

    /// 【encode】canonical → Responses wire。probe で確定した形 (2026-08-20 実測):
    /// `input` は `{type:"message", role, content}` の列 (system ロール可) / 関数ツールは
    /// **flat** (`{type:"function", name, description, parameters}`、互換層の `function` 入れ子
    /// ではない) / 単一ツール強制は **`tool_choice: "required"`** — 名指し
    /// `{type:function,name}` は 200 で受理されるが**黙殺**される (message が返った) ので、
    /// 提示ツールが 1 本の Kataribe では `required` が等価で唯一効く形 / `store: false` 常送 /
    /// 出力上限は `max_output_tokens` / temperature は明示時のみ / tools 無しなら tool_choice 無し。
    #[test]
    fn responses_encode_flat_tools_required_and_store_false() {
        let req = compat_req("perplexity/deepseek-v4-flash-0731");
        let wire_req = responses::encode(&req, responses::Flavor::OpenAi);
        let body = serde_json::to_value(&wire_req).unwrap();
        assert_eq!(body["model"], "perplexity/deepseek-v4-flash-0731");
        assert_eq!(body["store"], false, "接続先に会話を保持させない");
        assert_eq!(body["max_output_tokens"], 4096);
        assert!(body.get("temperature").is_none(), "None なら送らない");
        assert!(body.get("max_tokens").is_none(), "chat/completions の欄名は使わない");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "system");
        assert_eq!(input[0]["content"], "あなたはGM");
        assert_eq!(input.last().unwrap()["role"], "user");
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], EMIT_DELTA_TOOL, "flat — `function` の入れ子ではない");
        assert!(tools[0].get("function").is_none());
        assert!(tools[0]["parameters"]["properties"]["ops"].is_object());
        assert_eq!(body["tool_choice"], "required", "名指しは黙殺されるので required");

        // tools 無し (generate) なら tools も tool_choice も出ない。
        let plain = canonical::ChatRequest {
            tools: Vec::new(),
            tool_choice: canonical::ToolChoice::None,
            ..req
        };
        let body = serde_json::to_value(responses::encode(&plain, responses::Flavor::OpenAi)).unwrap();
        assert!(body.get("tools").is_none());
        assert!(body.get("tool_choice").is_none());
    }

    /// 【encode / Meta 方言】同じ Responses 形でもホストで受理集合が割れる (#78 の Responses 版。
    /// 2026-08-21 probe): `tool_choice: required` は 400 名指し拒否 → **欄ごと送らない** /
    /// 代わりに schema を prompt 末尾へ (互換経路 ToolMode::Auto の処方) / effort の受理語彙に
    /// `max` が無い → xhigh へ丸める / `store: false` は 200 受理 = 共通で送る。
    /// Perplexity (OpenAi flavor) は従来と不変。
    #[test]
    fn responses_encode_meta_flavor_omits_tool_choice_and_grounds_schema() {
        let mut req = compat_req("muse-spark-1.2-contributor");
        req.effort = Some(crate::config::Effort::Max);
        let body = serde_json::to_value(responses::encode(&req, responses::Flavor::Meta)).unwrap();
        assert!(body.get("tool_choice").is_none(), "Meta は auto のみ — 欄ごと送らない");
        assert_eq!(body["reasoning"]["effort"], "xhigh", "max は受理語彙に無い → 丸める");
        assert_eq!(body["store"], false, "store は probe で受理 = 共通");
        let input = body["input"].as_array().unwrap();
        let last = input.last().unwrap();
        assert_eq!(last["role"], "system", "schema の保険は末尾 system");
        let content = last["content"].as_str().unwrap();
        assert!(content.contains("emit_delta") && content.contains("JSON Schema"), "{content}");
        assert!(content.contains("`ops` は必ず配列"), "実機の崩れどころを名指し (#78)");
        // OpenAi flavor は不変: required が出て schema 注記は無い。
        let mut plain = compat_req("perplexity/deepseek-v4-flash-0731");
        plain.effort = Some(crate::config::Effort::Max);
        let body = serde_json::to_value(responses::encode(&plain, responses::Flavor::OpenAi)).unwrap();
        assert_eq!(body["tool_choice"], "required");
        assert_eq!(body["reasoning"]["effort"], "max", "Perplexity 側の語彙は触らない");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.last().unwrap()["role"], "user", "注記なし = 従来と同形");
        // flavor 判定はホスト。
        assert_eq!(responses::flavor_for("https://api.meta.ai/v1/"), responses::Flavor::Meta);
        assert_eq!(responses::flavor_for("https://api.perplexity.ai/v1"), responses::Flavor::OpenAi);
    }

    /// 【decode】Responses wire → canonical。`output` は種別の混在列: `function_call` の
    /// `arguments` (JSON 文字列) は境界で 1 回だけ parse / `message` の `output_text` が text /
    /// usage は `input_tokens`・`output_tokens`・`input_tokens_details.cached_tokens` /
    /// `status: incomplete` は Length (OutputTruncated の判定材料) / 未知種別 (`search_results`
    /// 等) は落として壊さない / 形が合わなければ raw を保持 (#34 同型)。
    #[test]
    fn responses_decode_function_call_message_usage_and_status() {
        let body = r#"{
            "id":"r1","status":"completed","model":"perplexity/deepseek-v4-flash-0731",
            "output":[
              {"type":"search_results","queries":["x"],"results":[]},
              {"type":"function_call","id":"fc1","status":"completed","name":"emit_delta",
               "call_id":"call_abc","arguments":"{\"narration\":\"扉が開く\",\"ops\":[{\"op\":\"set_flag\",\"key\":\"door_open\",\"value\":true}]}"}
            ],
            "usage":{"input_tokens":2169,"output_tokens":162,"total_tokens":2331,
                     "input_tokens_details":{"cached_tokens":700},
                     "output_tokens_details":{"reasoning_tokens":0},
                     "cost":{"currency":"USD","total_cost":0.00032}}
        }"#;
        let resp = client::decode_responses_body(body.to_string()).unwrap();
        let c = responses::decode(resp).unwrap();
        assert_eq!(c.tool_calls.len(), 1);
        assert_eq!(c.tool_calls[0].id, "call_abc");
        assert_eq!(c.tool_calls[0].name, "emit_delta");
        assert_eq!(c.finish, canonical::Finish::ToolUse);
        assert_eq!(c.usage.prompt, 2169);
        assert_eq!(c.usage.completion, 162);
        assert_eq!(c.usage.cache_read, 700);
        let delta: StateDelta = parse::extract(&c).unwrap();
        assert_eq!(delta.narration, "扉が開く");
        assert!(matches!(delta.ops[0], StateOp::SetFlag { .. }));

        // message だけ (ツールを使わなかった回) は text に落ち、finish は Stop。
        let body = r#"{"status":"completed","output":[
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"こんにちは","annotations":[]}]}
        ]}"#;
        let c = responses::decode(client::decode_responses_body(body.to_string()).unwrap()).unwrap();
        assert_eq!(c.text.as_deref(), Some("こんにちは"));
        assert!(c.tool_calls.is_empty());
        assert_eq!(c.finish, canonical::Finish::Stop);
        assert_eq!(c.usage.prompt, 0, "usage 無しでも壊れない");

        // incomplete → Length。空なら OutputTruncated に乗る (openai_compat と同じ防御を共有)。
        let body = r#"{"status":"incomplete","output":[]}"#;
        let c = responses::decode(client::decode_responses_body(body.to_string()).unwrap()).unwrap();
        assert_eq!(c.finish, canonical::Finish::Length);
        assert!(matches!(
            openai_compat::reject_empty_reasoning(c, 4096),
            Err(LlmError::OutputTruncated { limit: 4096 })
        ));

        // 壊れた arguments は raw を保持した Parse エラー (再生成の燃料)。
        let body = r#"{"status":"completed","output":[{"type":"function_call","name":"emit_delta","call_id":"c","arguments":"{\"narration\": "}]}"#;
        let err = responses::decode(client::decode_responses_body(body.to_string()).unwrap()).unwrap_err();
        assert!(matches!(err, LlmError::Parse { raw, .. } if raw.contains("narration")));

        // 形が合わない本文 (HTML 等) は raw ごと保持。
        let err = client::decode_responses_body("<html>502</html>".to_string()).unwrap_err();
        assert!(matches!(err, LlmError::Parse { raw, .. } if raw.contains("502")));
    }
}

