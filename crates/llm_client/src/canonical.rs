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
    /// 会話の末尾まで prompt cache に載せる (2026-11-25、道具の往復で履歴が伸びるループ用)。
    /// ループは毎周、履歴を丸ごと送り直すので、末尾に印を置けば前の周までの全部が次の周で
    /// キャッシュ読みになる。今は Anthropic だけが見る (`anthropic::encode_with` — 他の adapter は
    /// 自動でプレフィックスを延ばすか、静的な部分だけを pin する)。立てるのは
    /// [`crate::LlmClient::chat`] だけ: GM のターンは毎回 user を組み直すので、末尾に印を置くと
    /// 読まれない書き込みに割増を払うだけになり、まとめの周は `tool_choice` が変わって
    /// 会話のキャッシュを読めない上に、その後に読む周が無い。
    pub cache_tail: bool,
}

/// ツール定義。`parameters` は JSON Schema (`emit_delta` は schemars 機械生成の単一真実源、
/// spec 29 の編集道具は手書き)。
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    /// 入力を schema どおりに縛ってよいか (Anthropic の strict tool use)。**schema 側が strict の
    /// 制約を満たすと作者が確かめた道具だけ** true にする (全 object に `additionalProperties: false`・
    /// 再帰なし・数値と文字列の範囲制約なし)。満たさない schema に付けると 400 になるので、型から
    /// 機械生成する `emit_delta` には付けない。送るのは Anthropic 経路の「名指しの強制を断られて
    /// auto へ降格した周」だけ (`anthropic::encode_with`)。他の adapter は見ない。
    pub strict: bool,
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
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct Usage {
    pub prompt: u64,
    pub completion: u64,
    pub cache_read: u64,
    /// プロバイダが申告した費用 (USD)。今は Perplexity (responses の `cost.total_cost`) だけが埋める
    /// (spec 30)。None = 申告なし (見積もりは提示層の仕事)。
    pub cost_usd: Option<f64>,
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
