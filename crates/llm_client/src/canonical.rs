//! プロバイダ中立の canonical モデル (spec 12 Phase A)。
//!
//! Driver 相当 ([`crate::LlmClient::generate`] / [`crate::LlmClient::generate_structured`]) は
//! この型だけを組み、各 adapter (openai_compat / anthropic) が encode/decode 純関数で
//! wire と相互変換する。**Driver は wire 形を一切見ない** (写経元 §4 の adapter 契約)。
//! 翻訳マトリクスの正本は specs/12_unified_tool_layer.md §6 / data_contract `UnifiedToolLayer` 節。

use serde_json::Value;

use crate::config::Effort;
use crate::wire::ChatMessage;

/// プロバイダ中立のリクエスト。
#[derive(Debug, Clone)]
pub(crate) struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolSpec>,
    pub tool_choice: ToolChoice,
    /// 明示設定時のみ送る (None なら provider 既定。新しめモデルは送ると 400)。
    pub temperature: Option<f32>,
    pub max_tokens: u32,
    /// 推論の深さ (spec 12 Phase B)。**None なら送らない** (opt-in)。方言への写像は adapter:
    /// Claude = `thinking: adaptive` + `output_config.effort` / Grok (Phase D) = `reasoning_effort`。
    pub effort: Option<Effort>,
}

/// ツール定義。`parameters` は JSON Schema (`emit_delta` は schemars 機械生成の単一真実源、
/// spec 29 の編集道具は手書き)。
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// ツール選択。Kataribe v1 は `None` (generate) と `Specific` (emit_delta 強制) のみ使う。
/// Auto/Required は canonical の語彙として保持 (将来の Driver/Registry 拡張で型を変えないため)。
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ToolChoice {
    None,
    Auto,
    Required,
    Specific(String),
}

/// 応答終了理由。`Length` は empty-response 防御 (spec 12 Phase D) の判定材料 —
/// 推論モデルが budget を思考に使い切った空応答の一次シグナル。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    Stop,
    ToolUse,
    Length,
    Other,
}

/// プロバイダ中立の usage。`cache_read` / `prompt` は CacheStat (GUI キャッシュ健全性警告
/// #44/#45 + spec 14 の hit rate 曲線) の一次ソースで、adapter が各 wire の該当フィールド
/// から正規化する。
#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub prompt: u64,
    pub completion: u64,
    pub cache_read: u64,
}

/// ツール呼び出し。定義は wire 側 ([`crate::wire::ToolCall`]、pub) — spec 29 で呼び出し側
/// (編集ループ) が履歴を組むので canonical の外へ出した。`args` は **必ず JSON オブジェクト** (D2)。
pub(crate) use crate::wire::ToolCall;

/// プロバイダ中立の応答。
#[derive(Debug, Clone)]
pub(crate) struct ChatResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    #[allow(dead_code)]
    pub finish: Finish,
    pub usage: Usage,
}
