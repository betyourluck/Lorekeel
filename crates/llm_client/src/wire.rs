//! OpenAI 互換 chat/completions のワイヤ型 (request / response)。
//!
//! ここは **LLM との境界の唯一の真実**。壊れるのはこの ser/de なので、PoC テストで固める。
//! tool-use 強制で構造化出力 (`emit_delta` 関数の arguments) を取り出すのが主経路。

use serde::{Deserialize, Serialize};

/// メッセージ役割。`tool` ロールは将来のツール結果返却用 (現状未使用)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// 送信メッセージ。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: Role::System, content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: content.into() }
    }
}

// --- リクエスト ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    /// 明示設定時のみ送る。新しめのモデル (例: claude-opus-4-8) は temperature を
    /// 非対応にしており、送ると 400 を返す。未設定 (None) なら provider 既定に委ねる。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// 最大出力トークン数 (旧欄)。**モデルにより欄名が割れる** — gpt-5 系と o 系は思考
    /// トークン込みで上限を管理する方式へ移り、この欄を 400 `unsupported_parameter` で拒否して
    /// `max_completion_tokens` を要求する。逆に互換サーバ (llama.cpp / vLLM / gpt-oss 系) には
    /// 新欄を知らないものがあるので全面置換もできない → 常に**どちらか片方だけ**を送る
    /// (選ぶのは [`crate::openai_compat::uses_max_completion_tokens`])。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// 推論系モデル用の出力上限 (思考トークン込み)。[`Self::max_tokens`] と排他。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// xAI Grok の推論制御 (spec 12 Phase D)。**対象モデル (grok-4.3/4.5) には既定で送る**
    /// (opt-out) — 未送出だと xAI 側の既定 (4.3=low 常時思考 / 4.5=high) が適用され、
    /// 思考が max_tokens を食い潰して空デルタ/タイムアウトになる (grok-4.3 実測の真因仮説)。
    /// 他モデル/他サーバには送らない (None = キーごと省略)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<&'static str>,
}

/// 関数ツール定義。`parameters` は schemars 生成の JSON Schema (gm_core が単一真実源)。
#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub kind: ToolKind,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Function,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// ツール選択の指定。**サーバによって受け付ける形が違う** — Meta (api.llama.com) は
/// `only "auto" is supported for tool_choice. "none", "required", and named function choices
/// are not currently supported` と名指しで 400 を返す (2026-08-08 実機)。
/// どちらの形を送るかは [`crate::config::ToolMode`] が決める。
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ToolChoice {
    /// 素の文字列 (`"auto"`)。Meta が唯一受け付ける形。
    Mode(&'static str),
    /// `{"type":"function","function":{"name":...}}` で名前指定の強制。
    Function {
        #[serde(rename = "type")]
        kind: ToolKind,
        function: ToolChoiceFunction,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolChoiceFunction {
    pub name: String,
}

impl ToolChoice {
    /// 名前指定で強制する (`ToolMode::Forced`)。
    pub fn force(name: impl Into<String>) -> Self {
        Self::Function {
            kind: ToolKind::Function,
            function: ToolChoiceFunction { name: name.into() },
        }
    }

    /// モデルの判断に委ねる (`ToolMode::Auto`)。**キーを省かず明示する** —
    /// 省略時の既定は仕様上 `"auto"` だが、送っていない値でサーバ既定に決めさせると
    /// 何が効いているか計器から消える (failures #77 の「黙っていることは値を決めて
    /// いないことではない」)。`LLM_DEBUG` の送信ボディに意図が残る。
    pub fn auto() -> Self {
        Self::Mode("auto")
    }
}

// --- レスポンス ---------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    #[serde(default)]
    pub choices: Vec<Choice>,
    /// 使用量。キャッシュ計測 (`prompt_tokens_details.cached_tokens`) の一次ソース (#45)。
    /// OpenAI / xAI / Gemini 互換が返す。無い・形が違うサーバでも壊れない (default/Option)。
    #[serde(default)]
    pub usage: Option<ChatUsage>,
}

/// OpenAI 互換の usage。`prompt_tokens_details.cached_tokens` > 0 = プレフィックスが
/// キャッシュから読まれた (xAI 84% 引き / OpenAI 50% 引きの対象)。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    /// 応答メッセージ。**意図して必須のまま**にしてある (`serde(default)` を付けない)。
    ///
    /// 応答側を緩く取るのが本モジュールの一般則だが、ここだけは逆側の実測が勝つ (#34):
    /// Gemini が 200 で `message` 無しを返した時、**欠落を許すと `content_filter` という
    /// 真因が「空の応答」に潰れる**。必須のままなら Parse エラーが raw ごと本文を surface し、
    /// 何が起きたか読める。緩さが安全なのは**情報を捨てないとき**だけで、
    /// ここでは緩さそのものが情報を捨てる。
    ///
    /// 「`delta` を返すサーバ」は `stream: true` を送ったときの形で、Kataribe は
    /// ストリーミングを使わないので該当しない (spec 12 Phase F は未着手)。
    pub message: ResponseMessage,
    /// 終了理由 (`stop`/`tool_calls`/`length`/...)。canonical `Finish` の材料
    /// (empty-response 防御 spec 12 Phase D)。返さないサーバでも壊れない。
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponseMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallResponse>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolCallResponse {
    pub function: FunctionCallResponse,
    /// 呼び出し ID。canonical `ToolCall.id` に運ぶ (返さないサーバは空扱い)。
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FunctionCallResponse {
    /// **JSON 文字列** (オブジェクトではない)。canonical への decode 境界で 1 回だけ parse する
    /// (写経元 D2)。
    #[serde(default)]
    pub arguments: String,
    /// 関数名。単一ツール強制 (emit_delta) では分岐に使わないが canonical へ運ぶ。
    #[serde(default)]
    pub name: Option<String>,
}

