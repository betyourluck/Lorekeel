//! spec 29 Phase A — tool の往復が 4 adapter の wire に正しい形で出ることの PoC。
//!
//! 列は「assistant がツールを 2 本呼ぶ → 結果 2 本 → assistant の本文」。各 adapter で
//! 呼び出しと結果が対で wire に載り、対応 id / 名前が保たれることを固定する。
//! 従来メッセージのバイト同一性は `golden` が別に固定する。

#[cfg(test)]
mod tests {
    use crate::canonical::{ChatRequest, ToolChoice, ToolSpec};
    use crate::config::ToolMode;
    use crate::wire::{ChatMessage, ToolCall};
    use crate::{anthropic, gemini, openai_compat, responses};
    use serde_json::{json, Value};

    fn tools() -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: "read".into(),
                description: "ファイルを読む".into(),
                parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}),
            },
            ToolSpec {
                name: "sd".into(),
                description: "置換".into(),
                parameters: json!({"type":"object","properties":{"pattern":{"type":"string"}}}),
            },
        ]
    }

    fn roundtrip_messages(ids: (&str, &str)) -> Vec<ChatMessage> {
        vec![
            ChatMessage::system("あなたは編集者です。"),
            ChatMessage::user("hp を 12 にして"),
            ChatMessage::assistant_tool_calls(
                "",
                vec![
                    ToolCall { id: ids.0.into(), name: "read".into(), args: json!({"path": "package.yaml"}) },
                    ToolCall { id: ids.1.into(), name: "sd".into(), args: json!({"pattern": "hp: 10"}) },
                ],
            ),
            ChatMessage::tool_result(ids.0, "read", "1: title: x\n2: hp: 10"),
            ChatMessage::tool_result(ids.1, "sd", "preview: -hp: 10 +hp: 12"),
            ChatMessage::assistant("直しました。"),
        ]
    }

    fn request(messages: Vec<ChatMessage>) -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            messages,
            tools: tools(),
            tool_choice: ToolChoice::Auto,
            temperature: None,
            max_tokens: 1000,
            effort: None,
        }
    }

    fn to_value<T: serde::Serialize>(t: &T) -> Value {
        serde_json::to_value(t).unwrap()
    }

    #[test]
    fn openai_compat_emits_tool_calls_and_tool_role() {
        let v = to_value(&openai_compat::encode(&request(roundtrip_messages(("c1", "c2"))), ToolMode::Forced));
        let msgs = v["messages"].as_array().unwrap();
        // 従来の発話には tool 欄が出ない。
        assert_eq!(msgs[1], json!({"role": "user", "content": "hp を 12 にして"}));
        // ツールを呼んだ assistant: 本文は空なので省き、tool_calls の arguments は JSON 文字列。
        let a = &msgs[2];
        assert_eq!(a["role"], "assistant");
        assert!(a.get("content").is_none(), "空本文は省く: {a}");
        assert_eq!(a["tool_calls"][0]["id"], "c1");
        assert_eq!(a["tool_calls"][0]["type"], "function");
        assert_eq!(a["tool_calls"][0]["function"]["name"], "read");
        assert_eq!(a["tool_calls"][0]["function"]["arguments"], "{\"path\":\"package.yaml\"}");
        assert_eq!(a["tool_calls"][1]["id"], "c2");
        // 結果: role tool + tool_call_id。
        assert_eq!(msgs[3], json!({"role": "tool", "content": "1: title: x\n2: hp: 10", "tool_call_id": "c1"}));
        assert_eq!(msgs[4]["tool_call_id"], "c2");
        assert_eq!(msgs[5], json!({"role": "assistant", "content": "直しました。"}));
        // Auto では単一ツール強制の schema 指示を末尾に積まない (tools が 2 本のときは誤り)。
        assert_eq!(msgs.len(), 6);
        assert_eq!(v["tools"].as_array().unwrap().len(), 2);
        // 互換経路 Auto モードでは tool_choice "auto"。
        let auto = to_value(&openai_compat::encode(&request(roundtrip_messages(("c1", "c2"))), ToolMode::Auto));
        assert_eq!(auto["tool_choice"], "auto");
        assert_eq!(auto["messages"].as_array().unwrap().len(), 6);
    }

    #[test]
    fn anthropic_emits_tool_use_and_tool_result_blocks() {
        let v = to_value(&anthropic::encode(&request(roundtrip_messages(("tu_1", "tu_2")))));
        let msgs = v["messages"].as_array().unwrap();
        // 従来の発話は素の文字列 content。
        assert_eq!(msgs[0], json!({"role": "user", "content": "hp を 12 にして"}));
        // assistant: tool_use ブロック 2 本 (空本文の text ブロックは出さない = 400 回避)。
        let a = &msgs[1];
        assert_eq!(a["role"], "assistant");
        let blocks = a["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        assert_eq!(blocks[0], json!({"type": "tool_use", "id": "tu_1", "name": "read", "input": {"path": "package.yaml"}}));
        // 結果は **user** ロールの tool_result ブロック。
        assert_eq!(
            msgs[2],
            json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "tu_1", "content": "1: title: x\n2: hp: 10"}]})
        );
        assert_eq!(msgs[3]["content"][0]["tool_use_id"], "tu_2");
        assert_eq!(msgs[4], json!({"role": "assistant", "content": "直しました。"}));
        // Auto は {type: auto}。tools は 2 本とも載る。
        assert_eq!(v["tool_choice"], json!({"type": "auto"}));
        assert_eq!(v["tools"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn gemini_emits_function_call_and_response_without_ids() {
        // id は Kataribe が合成したものなので送らない — 空 id でも functionResponse は名前で立つ。
        let v = to_value(&gemini::encode(&request(roundtrip_messages(("", "")))));
        let contents = v["contents"].as_array().unwrap();
        assert_eq!(contents[0], json!({"role": "user", "parts": [{"text": "hp を 12 にして"}]}));
        let a = &contents[1];
        assert_eq!(a["role"], "model");
        let parts = a["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2, "空本文の text part は出さない: {parts:?}");
        assert_eq!(parts[0], json!({"functionCall": {"name": "read", "args": {"path": "package.yaml"}}}));
        assert!(parts[0]["functionCall"].get("id").is_none());
        assert_eq!(
            contents[2],
            json!({"role": "user", "parts": [{"functionResponse": {"name": "read", "response": {"result": "1: title: x\n2: hp: 10"}}}]})
        );
        assert_eq!(contents[3]["parts"][0]["functionResponse"]["name"], "sd");
        assert_eq!(contents[4], json!({"role": "model", "parts": [{"text": "直しました。"}]}));
        assert_eq!(v["toolConfig"]["functionCallingConfig"]["mode"], "AUTO");
        assert_eq!(v["tools"][0]["functionDeclarations"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn responses_emits_function_call_and_output_items() {
        for flavor in [responses::Flavor::OpenAi, responses::Flavor::Meta] {
            let v = to_value(&responses::encode(&request(roundtrip_messages(("fc_1", "fc_2"))), flavor));
            let input = v["input"].as_array().unwrap();
            assert_eq!(input[1], json!({"type": "message", "role": "user", "content": "hp を 12 にして"}));
            // 空本文の assistant は出さず、function_call 2 本が続く。
            assert_eq!(
                input[2],
                json!({"type": "function_call", "call_id": "fc_1", "name": "read", "arguments": "{\"path\":\"package.yaml\"}"})
            );
            assert_eq!(input[3]["call_id"], "fc_2");
            assert_eq!(input[4], json!({"type": "function_call_output", "call_id": "fc_1", "output": "1: title: x\n2: hp: 10"}));
            assert_eq!(input[5]["call_id"], "fc_2");
            assert_eq!(input[6], json!({"type": "message", "role": "assistant", "content": "直しました。"}));
            // Auto: schema 指示 (Meta 方言) も tool_choice も出ない。
            assert_eq!(input.len(), 7, "{flavor:?}: {input:?}");
            assert!(v.get("tool_choice").is_none(), "{flavor:?}");
            assert_eq!(v["tools"].as_array().unwrap().len(), 2);
        }
    }

    /// `EDITOR_LLM_*` はあらすじの別指定と同じ継承則: 未指定フィールドは GM 設定を継ぐ。
    #[test]
    fn editor_profile_inherits_unspecified_fields_from_base() {
        use crate::config::{LlmConfig, Provider};
        let base = LlmConfig::new("https://api.anthropic.com/v1", "k", "claude-x");
        // 何も無ければ None (GM 共用)。
        assert!(LlmConfig::profile_overrides(&base, "EDITOR", None, None, None, None).unwrap().is_none());
        // model だけ → base_url / api_key / provider は継承。
        let m = LlmConfig::profile_overrides(&base, "EDITOR", None, None, Some("claude-y".into()), None)
            .unwrap()
            .unwrap();
        assert_eq!(m.model, "claude-y");
        assert_eq!(m.base_url, base.base_url);
        assert_eq!(m.api_key, "k");
        assert_eq!(m.provider, Provider::Anthropic);
        // base_url だけ → model は継承、provider は実効 url から再判定。
        let u = LlmConfig::profile_overrides(
            &base,
            "EDITOR",
            Some("https://generativelanguage.googleapis.com".into()),
            None,
            None,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(u.model, "claude-x");
        assert_eq!(u.provider, Provider::Gemini);
        // 不正な provider の文言は EDITOR_ の env 名を名指す。
        let err = LlmConfig::profile_overrides(&base, "EDITOR", None, None, Some("m".into()), Some("nope".into()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("EDITOR_LLM_PROVIDER"), "{err}");
    }
}
