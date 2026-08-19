//! OpenAI Responses 形 (`POST {base_url}/responses`) のワイヤ型と encode/decode 純関数
//! (2026-08-20)。
//!
//! **なぜ存在するか**: Perplexity には `/chat/completions` の口が**無い**
//! (`https://api.perplexity.ai/v1` を互換経路で叩くと **404・本文なし**。アプリの実観測、
//! Fuseforks 2026-08-19 と同じ)。互換の口は `/router/v1/chat/completions` (Router API) だが
//! **有料クレジットが要る** (403 `restricted_api_key`)。鍵だけで通るのは Agent API
//! (`/v1/agent`) で、その OpenAI Responses 互換別名 `/v1/responses` をこの module が話す。
//!
//! probe で確定した形 (2026-08-20、`perplexity/deepseek-v4-flash-0731`):
//! - `input` は `{type:"message", role, content}` の列。**system ロールが通る**
//! - 関数ツールは **flat** (`{type:"function", name, description, parameters}`) —
//!   互換層の `function` 入れ子ではない。Kataribe の実 schema (oneOf 入り・$ref inline 済) を
//!   そのまま受理し、`ops` を正しい配列で返した
//! - **`tool_choice: "required"` は効く** (3/3 で function_call)。名指し
//!   `{type:"function", name}` は 200 で受理されるが**黙殺**される (message が返った) —
//!   文書の request schema にも `tool_choice` は無い。提示ツールが `emit_delta` 1 本の
//!   Kataribe では `required` が名指しと等価なのでそちらを送る
//! - 応答は `output` の混在列 (`function_call` / `message` / `search_results` …)。
//!   `function_call.arguments` は **JSON 文字列** (decode 境界で 1 回だけ parse = 写経元 D2)
//! - usage は `input_tokens` / `output_tokens` / `input_tokens_details.cached_tokens`
//!   (+ Perplexity 固有の `cost` USD。読むが使わない)
//!
//! 写経元は Fuseforks `crates/fuseforks-core/src/llm/openai_responses.rs` (Spec 34) — ただし
//! Kataribe には web 検索も思考の要約も要らないので、**送るのは Kataribe が使う欄だけ**
//! (`reasoning` は LLM_EFFORT 明示時のみ・`include` は送らない・`store: false` 常送)。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::canonical::{ChatRequest, ChatResponse, Finish, ToolCall, ToolChoice, Usage};
use crate::config::Effort;
use crate::error::LlmError;
use crate::wire::Role;

// --- リクエスト ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ResponsesRequest {
    pub model: String,
    /// 会話の列 (`messages` に相当)。
    pub input: Vec<InputItem>,
    /// 空なら欄ごと省く。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDef>,
    /// `"required"` のときだけ送る (名指しは黙殺されるので送らない)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<&'static str>,
    /// 出力上限 (chat/completions の `max_tokens` に相当。欄名が違う)。
    pub max_output_tokens: u32,
    /// 明示設定時のみ送る (canonical の規律)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// 推論の深さ。`LLM_EFFORT` 明示時のみ `{effort}` を送る (opt-in)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
    /// 接続先に会話を保持させない。**常送** (Perplexity の既定は true)。
    pub store: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct InputItem {
    #[serde(rename = "type")]
    pub kind: &'static str, // 常に "message"
    pub role: &'static str, // system | user | assistant
    pub content: String,
}

/// 関数ツール定義。**flat** — 互換層 (`{type, function:{name,…}}`) と違い `name` 等が
/// トップレベル。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ToolDef {
    #[serde(rename = "type")]
    pub kind: &'static str, // 常に "function"
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Reasoning {
    pub effort: &'static str,
}

// --- レスポンス ---------------------------------------------------------------

/// 応答。`output` は種別の混在列なので**全欄 Option の構造体**で受けて `kind` で分岐する
/// (enum で閉じると未知種別で応答全体の parse が落ちる — Gemini の part と同じ流儀)。
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ResponsesResponse {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub output: Vec<OutputItem>,
    #[serde(default)]
    pub usage: Option<ResponsesUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct OutputItem {
    #[serde(rename = "type")]
    pub kind: String,
    /// `message` のとき。
    #[serde(default)]
    pub content: Option<Vec<ContentPart>>,
    /// `function_call` のとき。
    #[serde(default)]
    pub call_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// `function_call` のとき。**JSON 文字列**。
    #[serde(default)]
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ContentPart {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ResponsesUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub input_tokens_details: Option<InputTokensDetails>,
}

/// キャッシュ計数。実ワイヤは `cached_tokens` (probe 実測)、文書は
/// `cache_read_input_tokens` と書く — **両方受けて**大きい方を採る。
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct InputTokensDetails {
    #[serde(default)]
    pub cached_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

// --- 変換 ---------------------------------------------------------------------

/// canonical → Responses wire (純粋)。
pub(crate) fn encode(req: &ChatRequest) -> ResponsesRequest {
    let input = req
        .messages
        .iter()
        .map(|m| InputItem {
            kind: "message",
            role: match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                // Kataribe はツール結果を返さない (単一ツール強制で 1 往復)。
                // 万一混じっても user に落として壊さない。
                Role::Tool => "user",
            },
            content: m.content.clone(),
        })
        .collect();
    let tools: Vec<ToolDef> = req
        .tools
        .iter()
        .map(|t| ToolDef {
            kind: "function",
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.parameters.clone(),
        })
        .collect();
    // 名指し (`Specific`) は黙殺されるので `required` へ畳む — 提示ツールが 1 本なら等価。
    // Auto/None は送らない (= サーバ既定)。tools が無ければ tool_choice も無し。
    let tool_choice = if tools.is_empty() {
        None
    } else {
        match &req.tool_choice {
            ToolChoice::Specific(_) | ToolChoice::Required => Some("required"),
            ToolChoice::Auto | ToolChoice::None => None,
        }
    };
    ResponsesRequest {
        model: req.model.clone(),
        input,
        tools,
        tool_choice,
        max_output_tokens: req.max_tokens,
        temperature: req.temperature,
        reasoning: req.effort.map(|e| Reasoning {
            effort: match e {
                Effort::Low => "low",
                Effort::Medium => "medium",
                Effort::High => "high",
                Effort::XHigh => "xhigh",
                Effort::Max => "max",
            },
        }),
        store: false,
    }
}

/// Responses wire → canonical (純粋)。
///
/// `function_call.arguments` (JSON 文字列) はここで **1 回だけ** parse して以後は
/// オブジェクトとして運ぶ。壊れた arguments は **raw を保持した** Parse エラー (#34 同型)。
/// 未知種別 (`search_results` / `reasoning` 等) は落として壊さない。
pub(crate) fn decode(resp: ResponsesResponse) -> Result<ChatResponse, LlmError> {
    let mut texts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    for item in resp.output {
        match item.kind.as_str() {
            "message" => {
                for part in item.content.unwrap_or_default() {
                    if part.kind != "output_text" {
                        continue;
                    }
                    if let Some(text) = part.text {
                        if !text.is_empty() {
                            texts.push(text);
                        }
                    }
                }
            }
            "function_call" => {
                let raw = item.arguments.unwrap_or_default();
                let args = serde_json::from_str(&raw)
                    .map_err(|source| LlmError::Parse { source, raw: raw.clone() })?;
                tool_calls.push(ToolCall {
                    id: item.call_id.unwrap_or_default(),
                    name: item.name.unwrap_or_default(),
                    args,
                });
            }
            _ => {}
        }
    }
    let usage = resp
        .usage
        .as_ref()
        .map(|u| {
            let cache_read = u
                .input_tokens_details
                .as_ref()
                .map(|d| d.cached_tokens.max(d.cache_read_input_tokens))
                .unwrap_or(0);
            Usage { prompt: u.input_tokens, completion: u.output_tokens, cache_read }
        })
        .unwrap_or_default();
    let finish = if !tool_calls.is_empty() {
        Finish::ToolUse
    } else {
        match resp.status.as_deref() {
            Some("completed") => Finish::Stop,
            // 打ち切り。空なら openai_compat::reject_empty_reasoning が OutputTruncated へ。
            Some("incomplete") => Finish::Length,
            _ => Finish::Other,
        }
    };
    let text = if texts.is_empty() { None } else { Some(texts.join("\n\n")) };
    Ok(ChatResponse { text, tool_calls, finish, usage })
}
