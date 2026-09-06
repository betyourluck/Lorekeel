//! spec 29 Phase A の**バイト同一性 golden**。
//!
//! tool の往復 (assistant の `tool_calls` / `Role::Tool` の結果返送) を canonical と 4 adapter に
//! 足すにあたり、**tool 欄を持たない従来のメッセージ列の encode 結果が 1 バイトも変わらない**
//! ことを固定する。安定プレフィックスのバイト列がキャッシュの鍵 (spec 13/14) なので、
//! `null` が 1 つ増えるだけでキャッシュが割れる (= `skip_serializing_if` の忘れを検出する)。
//!
//! フィクスチャは `tests/golden/*.json`。**改修前のコードで採取した** (2026-09-06)。
//! 更新は `UPDATE_GOLDEN=1 cargo test -p llm_client golden` — 意図して wire を変えるときだけ。

#[cfg(test)]
mod tests {
    use crate::canonical::{ChatRequest, ToolChoice, ToolSpec};
    use crate::config::ToolMode;
    use crate::wire::ChatMessage;
    use crate::{anthropic, gemini, openai_compat, responses};

    fn messages() -> Vec<ChatMessage> {
        vec![
            ChatMessage::system("あなたは GM です。"),
            ChatMessage::system("# あらすじ\n第一章。"),
            ChatMessage::user("プレイヤー: 扉を調べる"),
            ChatMessage::assistant("扉は固く閉ざされている。"),
            ChatMessage::user("プレイヤー: 鍵を使う"),
        ]
    }

    fn request(with_tool: bool) -> ChatRequest {
        let tools = if with_tool {
            vec![ToolSpec {
                name: "emit_delta".into(),
                description: "ターンの提出".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { "narration": { "type": "string" } },
                    "required": ["narration"]
                }),
            }]
        } else {
            Vec::new()
        };
        ChatRequest {
            model: "test-model".into(),
            messages: messages(),
            tool_choice: if with_tool { ToolChoice::Specific("emit_delta".into()) } else { ToolChoice::None },
            tools,
            temperature: None,
            max_tokens: 4096,
            effort: None,
        }
    }

    fn check(name: &str, actual: &str) {
        let path = format!("{}/tests/golden/{name}.json", env!("CARGO_MANIFEST_DIR"));
        if std::env::var("UPDATE_GOLDEN").is_ok() {
            std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
            std::fs::write(&path, actual).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("golden {path} が読めない: {e}"));
        assert_eq!(actual, expected, "{name}: wire のバイト列が golden と違う");
    }

    #[test]
    fn golden_openai_compat_forced_and_plain() {
        let forced = openai_compat::encode(&request(true), ToolMode::Forced);
        check("openai_forced", &serde_json::to_string(&forced).unwrap());
        let auto = openai_compat::encode(&request(true), ToolMode::Auto);
        check("openai_auto", &serde_json::to_string(&auto).unwrap());
        let plain = openai_compat::encode(&request(false), ToolMode::Forced);
        check("openai_plain", &serde_json::to_string(&plain).unwrap());
    }

    #[test]
    fn golden_anthropic() {
        check("anthropic_forced", &serde_json::to_string(&anthropic::encode(&request(true))).unwrap());
        check("anthropic_plain", &serde_json::to_string(&anthropic::encode(&request(false))).unwrap());
    }

    #[test]
    fn golden_gemini() {
        check("gemini_forced", &serde_json::to_string(&gemini::encode(&request(true))).unwrap());
        check("gemini_plain", &serde_json::to_string(&gemini::encode(&request(false))).unwrap());
    }

    #[test]
    fn golden_responses() {
        let r = request(true);
        check("responses_openai", &serde_json::to_string(&responses::encode(&r, responses::Flavor::OpenAi)).unwrap());
        check("responses_meta", &serde_json::to_string(&responses::encode(&r, responses::Flavor::Meta)).unwrap());
        let p = request(false);
        check("responses_plain", &serde_json::to_string(&responses::encode(&p, responses::Flavor::OpenAi)).unwrap());
    }
}
