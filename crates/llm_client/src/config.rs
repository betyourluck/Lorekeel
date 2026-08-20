//! 環境変数からの設定ロード。LocalAI `config.py::LLMSettings` の Rust 版。
//!
//! クラウド LLM キーは `.env` (gitignore 済) に置く。`.env.example` を雛形にする。

use std::time::Duration;

use crate::error::LlmError;

/// LLM の話し方 (ワイヤプロトコル)。
///
/// OpenAI 互換層は **prompt caching 非対応** (公式 docs 明記) なので、Anthropic へは
/// ネイティブ Messages API を使う — 毎ターンのフルプロンプト再送で安定プレフィックス
/// (schema + GM_SYSTEM + scenario_brief) がキャッシュ読取 (0.1×) になる (#44)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// `POST {base_url}/chat/completions` + Bearer (OpenAI / Grok / さくら / ローカル互換サーバ)。
    OpenAiCompat,
    /// `POST {base_url}/messages` + x-api-key + anthropic-version (キャッシュの効く経路)。
    /// tool-use を常に使う (`use_tools=false` は無視 — Anthropic は tool_choice を確実に尊重する)。
    Anthropic,
    /// `POST {base}/v1beta/models/{model}:generateContent` + x-goog-api-key (spec 12 Phase C)。
    /// tool-use を常に使う (tool_choice = functionCallingConfig を確実に尊重する)。
    Gemini,
    /// `POST {base_url}/responses` + Bearer — OpenAI Responses 形のワイヤ (2026-08-20)。
    /// 第一の利用者は **Perplexity Agent API** (`/v1/agent` の互換別名 `/v1/responses`):
    /// Perplexity には `/chat/completions` の口が**無く** (404・本文なし)、互換の口
    /// `/router/v1/chat/completions` は有料クレジットが要る。鍵だけで通るのがこの経路。
    /// tool-use を常に使う (`tool_choice: "required"` が効く。名指しは黙殺されるが
    /// 提示ツールが 1 本なので等価)。
    Responses,
}

impl Provider {
    /// base_url からの自動判定。`LLM_PROVIDER` 未設定時の既定 —
    /// 配布受領者がキーを入れただけで正しいワイヤを話すように。
    ///
    /// **Gemini の罠 (spec 12 rev4)**: OpenAI 互換エンドポイント (`.../v1beta/openai/`) の
    /// base_url にも generativelanguage.googleapis.com が含まれる — 既存の互換利用者を
    /// 壊さないよう `/openai` を含む URL は互換のまま。判定不能なプロキシホストは
    /// OpenAiCompat に落ちる (安全側。明示 `LLM_PROVIDER` を .env.example で誘導)。
    ///
    /// **Perplexity (2026-08-20)**: `api.perplexity.ai` は Responses ワイヤ。ただし
    /// `/router/` を含む base_url (Router API = chat/completions 互換) は互換のまま —
    /// Gemini の `/openai` 例外と同じ形で、互換の口を選んだ利用者を壊さない。
    pub fn detect(base_url: &str) -> Self {
        if base_url.contains("api.anthropic.com") {
            Provider::Anthropic
        } else if base_url.contains("generativelanguage.googleapis.com")
            && !base_url.contains("/openai")
        {
            Provider::Gemini
        } else if base_url.contains("api.perplexity.ai") && !base_url.contains("/router/") {
            Provider::Responses
        } else {
            Provider::OpenAiCompat
        }
    }

    /// `LLM_PROVIDER` / `SUMMARY_LLM_PROVIDER` の値をパースする (純粋・両経路で共用)。
    pub(crate) fn parse_env(raw: &str, var: &str) -> Result<Self, LlmError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "anthropic" | "native" => Ok(Provider::Anthropic),
            "openai" | "openai_compat" | "compat" => Ok(Provider::OpenAiCompat),
            "gemini" | "google" => Ok(Provider::Gemini),
            "responses" | "openai_responses" | "perplexity" => Ok(Provider::Responses),
            other => Err(LlmError::Config(format!(
                "環境変数 {var} の値 '{other}' を解釈できません (anthropic / openai / gemini / responses)"
            ))),
        }
    }
}

/// ツール呼び出しの強さ。**能力の一軸** (`Forced` > `Auto` > `Off`) —
/// OpenAI 互換を名乗るサーバは `tool_choice` の実装範囲で三通りに割れる:
///
/// | 実態 | 例 | この enum |
/// |---|---|---|
/// | 名前指定の強制が通る | OpenAI / Grok / さくら以外の大半 | [`Forced`](Self::Forced) |
/// | tools は通るが `tool_choice` は `"auto"` のみ | **Meta (api.llama.com)** | [`Auto`](Self::Auto) |
/// | tools 自体を送れない | さくら AI Engine / ローカル互換 (#29) | [`Off`](Self::Off) |
///
/// 従来の `use_tools: bool` は下二つを同一視しており、Meta のような中間のサーバを
/// no-tools へ**過剰に降格**させていた (実測 narration 量 tool-use = no-tools の 1.60× ゆえ
/// 品質のダウングレード)。**`tool_choice` の実装範囲はワイヤ層の恒常的な仕事**で、
/// `max_tokens` の欄名や `reasoning_effort` の送り分けと同じ形の三例目 (failures #76)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMode {
    /// tools を送らない。schema を載せた JSON 指示を system に積み content から拾う (#29)。
    Off,
    /// tools は送るが `tool_choice` は素の `"auto"`。**強制が接地に降格する** —
    /// 提示するツールが `emit_delta` 1 本だけであること・GM_SYSTEM の提出行が mode 非依存で
    /// あること・`parse::extract` のフェンス JSON フォールバックの三枚が受け皿になる。
    Auto,
    /// `{"type":"function","function":{"name":...}}` で名前指定の強制 (既定)。
    Forced,
}

impl ToolMode {
    /// base_url からの自動判定 ([`Provider::detect`] と同じ思想 = 受領者ゼロ設定)。
    ///
    /// Meta の Llama API は `tool_choice` が `"auto"` 一値しか通らない
    /// (`only "auto" is supported for tool_choice` — 2026-08-08 実機 400)。
    /// **モデル名でなくホストで判定する** — 拒否はサーバ側の実装範囲であって
    /// モデルの性質ではなく、同じ Meta モデルでも OpenRouter 等の中継越しなら
    /// 名前指定が通る (モデル名判定は偽陽性を作る)。
    pub fn detect(base_url: &str) -> Self {
        if base_url.contains("api.llama.com") {
            ToolMode::Auto
        } else {
            ToolMode::Forced
        }
    }

    /// 400 を受けたときの一段の降格先。`Forced → Auto → Off` で、`Off` が底。
    /// 底まで落ちれば tools を送らないので `tool_choice` 起因の 400 は構造的に起きない。
    pub(crate) fn downgrade(self) -> Option<Self> {
        match self {
            ToolMode::Forced => Some(ToolMode::Auto),
            ToolMode::Auto => Some(ToolMode::Off),
            ToolMode::Off => None,
        }
    }

    /// `LLM_TOOL_MODE` の値をパースする (純粋)。
    ///
    /// 値語彙は `forced` / `auto` / `off`。**`off` を `none` と呼ばない**のは、
    /// ワイヤの `tool_choice: "none"` (ツールを見せるが使わせない) と意味が違うため —
    /// こちらは tools ごと送らない。互換のため `none` も受けるが正は `off`。
    fn parse_env(raw: &str, var: &str) -> Result<Self, LlmError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "forced" | "force" | "named" => Ok(ToolMode::Forced),
            "auto" => Ok(ToolMode::Auto),
            "off" | "none" | "false" | "0" => Ok(ToolMode::Off),
            other => Err(LlmError::Config(format!(
                "環境変数 {var} の値 '{other}' を解釈できません (forced / auto / off)"
            ))),
        }
    }
}

/// 推論の深さ (spec 12 Phase B)。Claude は `thinking: adaptive` + `output_config.effort`、
/// Grok (Phase D) は `reasoning_effort` へ写す — canonical の語彙は一つ、方言は adapter が持つ。
///
/// **未設定 (None) なら何も送らない** = 現行動作 (opt-in)。値語彙は Claude の 5 段階を正とし、
/// 対応しないプロバイダへの丸めは各 adapter の責務。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    /// `LLM_EFFORT` の値をパースする (純粋・テスト可)。不正値は None でなく Err —
    /// 黙って無視すると「効いているつもり」の静かな漏出になる (#44 の教訓)。
    pub(crate) fn parse(raw: &str) -> Result<Self, LlmError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "low" => Ok(Effort::Low),
            "medium" => Ok(Effort::Medium),
            "high" => Ok(Effort::High),
            "xhigh" | "x-high" | "x_high" => Ok(Effort::XHigh),
            "max" => Ok(Effort::Max),
            other => Err(LlmError::Config(format!(
                "環境変数 LLM_EFFORT の値 '{other}' を解釈できません (low / medium / high / xhigh / max)"
            ))),
        }
    }

    /// wire に載せる文字列 (Claude `output_config.effort` / Grok `reasoning_effort` 共通の語彙)。
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::XHigh => "xhigh",
            Effort::Max => "max",
        }
    }
}

/// LLM 接続設定。`base_url` + `api_key` 抽象でベンダーロックインを避ける。
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// 例: `https://api.openai.com/v1` / `https://api.anthropic.com/v1` / ローカル互換サーバ。
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// 明示設定時のみ送る (`None` なら provider 既定)。新しめのモデルは temperature 非対応。
    pub temperature: Option<f32>,
    pub max_tokens: u32,
    pub request_timeout: Duration,
    /// chat 呼び出しの最大試行回数 (指数 backoff)。tenacity `stop_after_attempt` 同型。
    pub max_retries: u32,
    /// ツール呼び出しの強さ (三値)。判定は優先順に `LLM_TOOL_MODE` 明示 →
    /// `LLM_USE_TOOLS=false` (→`Off`) → base_url からの自動判定 ([`ToolMode::detect`])。
    /// 実行時に `tool_choice` 起因の 400 を受けたら [`crate::LlmClient`] が一段降格して
    /// latch する (未知ホストの受け皿)。
    /// **`Provider::Anthropic` / `Provider::Gemini` では無視** (ネイティブ経路は常に tool-use)。
    pub tool_mode: ToolMode,
    /// ワイヤプロトコル。`LLM_PROVIDER` (anthropic|openai) 明示、未設定なら base_url から自動判定。
    pub provider: Provider,
    /// 推論の深さ (`LLM_EFFORT`、spec 12 Phase B)。**None なら送らない** (opt-in・現行動作)。
    /// Claude: `thinking: adaptive` + `output_config.effort` / Grok (Phase D): `reasoning_effort`。
    pub effort: Option<Effort>,
    /// spec 13: Gemini 明示キャッシュ (cachedContent) を使うか (既定 true、`LLM_GEMINI_CACHE=0` で off)。
    /// 暗黙キャッシュの ~8000 トークン崖 (failures #54) を静的プレフィックスの明示 pin で迂回する。
    /// **Gemini 以外の provider では無視** (Anthropic は cache_control で明示済 #44)。
    pub gemini_cache_enabled: bool,
    /// cachedContent の TTL 秒 (`LLM_GEMINI_CACHE_TTL`、既定 900=15分)。失効は透過再試行。
    pub gemini_cache_ttl_secs: u64,
    /// サイズゲート: 静的プレフィックスの文字数がこれ未満なら cache を作らない
    /// (`LLM_GEMINI_CACHE_MIN_CHARS`、既定 4000)。明示キャッシュには最小トークン (モデル依存) が
    /// あり小さいプレフィックスは create が 400 になる — 無駄な create を避ける最適化 (下回っても
    /// 正しさは fallback が守る)。
    pub gemini_cache_min_chars: usize,
}

impl LlmConfig {
    /// 環境変数から構築する。`LLM_BASE_URL` / `LLM_API_KEY` / `LLM_MODEL` は必須でないが、
    /// `api_key` が空の場合のみ `Config` エラーにする (キー無しでは API を叩けないため)。
    ///
    /// プロセス env を読むだけ (副作用なし・テスト可能)。`.env` の読み込みは呼び出し側
    /// (アプリ入口) の責務 — bin で `dotenvy::dotenv().ok()` を先に呼ぶ。
    ///
    /// 既定値:
    /// - `LLM_BASE_URL` / `LLM_MODEL` = **既定なし・未設定は Config エラー** (2026-08-20 ユーザーFB:
    ///   旧既定 gpt-4o-mini が設定画面に出て「無料で使える」と誤解された。黙って特定モデルを
    ///   選ぶ利益は無く、誤解と意図しない課金の芽だけがある)
    /// - `LLM_TEMPERATURE` = **未設定なら送らない** (provider 既定に委ねる)
    /// - `LLM_MAX_TOKENS` = `4096`
    /// - `LLM_REQUEST_TIMEOUT_SECS` = `120`, `LLM_MAX_RETRIES` = `3`
    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = env_opt("LLM_API_KEY").unwrap_or_default();
        if api_key.trim().is_empty() {
            return Err(LlmError::Config(
                "LLM_API_KEY が未設定です (.env に設定してください)".into(),
            ));
        }

        // temperature は明示設定時のみ Some。新しめのモデルは送ると 400 になるため。
        let temperature = match env_opt("LLM_TEMPERATURE") {
            None => None,
            Some(raw) => Some(raw.parse::<f32>().map_err(|_| {
                LlmError::Config(format!("環境変数 LLM_TEMPERATURE の値 '{raw}' を解釈できません"))
            })?),
        };

        let base_url = env_opt("LLM_BASE_URL").ok_or_else(|| {
            LlmError::Config(
                "LLM_BASE_URL が未設定です (設定 → AIモデル で接続先を選んでください)".into(),
            )
        })?;
        // 明示 (LLM_PROVIDER) > 自動判定 (base_url)。誤値は起動時に弾く (ネットワーク前)。
        let provider = match env_opt("LLM_PROVIDER") {
            None => Provider::detect(&base_url),
            Some(raw) => Provider::parse_env(&raw, "LLM_PROVIDER")?,
        };

        let effort = match env_opt("LLM_EFFORT") {
            None => None,
            Some(raw) => Some(Effort::parse(&raw)?),
        };

        // 明示 (LLM_TOOL_MODE) > 旧キー (LLM_USE_TOOLS=false → Off) > 自動判定 (base_url)。
        let tool_mode = resolve_tool_mode(
            env_opt("LLM_TOOL_MODE"),
            env_opt("LLM_USE_TOOLS"),
            &base_url,
        )?;

        let config = Self {
            base_url,
            api_key,
            model: env_opt("LLM_MODEL").ok_or_else(|| {
                LlmError::Config(
                    "LLM_MODEL が未設定です (設定 → AIモデル でモデル名を入れてください)".into(),
                )
            })?,
            temperature,
            max_tokens: env_parse("LLM_MAX_TOKENS", 4096)?,
            request_timeout: Duration::from_secs(env_parse("LLM_REQUEST_TIMEOUT_SECS", 120)?),
            max_retries: env_parse("LLM_MAX_RETRIES", 3)?,
            tool_mode,
            provider,
            effort,
            // spec 13: Gemini 明示キャッシュ (既定 on、Gemini 以外では無視)。
            gemini_cache_enabled: env_opt("LLM_GEMINI_CACHE")
                .map(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "false" | "0" | "no" | "off"))
                .unwrap_or(true),
            gemini_cache_ttl_secs: env_parse("LLM_GEMINI_CACHE_TTL", 900u64)?,
            gemini_cache_min_chars: env_parse("LLM_GEMINI_CACHE_MIN_CHARS", 4000usize)?,
        };
        // 非 fatal の設定警告 (headroom / temperature 併用) を起動時に 1 回 surface する。
        for w in config.warnings() {
            eprintln!("[LLM_CONFIG] 警告: {w}");
        }
        Ok(config)
    }

    /// 非 fatal の設定警告 (純粋・テスト可)。プレイを止めないが「効いているつもり」を防ぐ:
    /// - effort 設定時の max_tokens 不足 — Claude 系の max_tokens は **thinking+output の合算上限**
    ///   (combined)。既定 4096 のままだと思考が本文を食い潰し空応答/切断の芽 (spec 12 rev4、
    ///   claude-api リファレンス接地: effort 時 ≥16000 推奨・xhigh/max は 64000 目安)。
    /// - effort + temperature の併用 — effort が効く世代 (Opus 4.7/4.8・Sonnet 5) は
    ///   temperature 自体をモデルレベルで 400 にする。送る前に気づけるように。
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.effort.is_some() {
            if self.max_tokens < 16000 {
                out.push(format!(
                    "LLM_EFFORT 設定時は LLM_MAX_TOKENS を 16000 以上に推奨 (現在 {} — Claude 系は思考+本文の合算上限のため、思考が本文を食い潰す恐れ。xhigh/max は 64000 目安)",
                    self.max_tokens
                ));
            }
            if self.temperature.is_some() {
                out.push(
                    "LLM_EFFORT と LLM_TEMPERATURE の併用 — effort が効くモデル (Opus 4.7/4.8・Sonnet 5) は temperature を受け付けず 400 を返します (LLM_TEMPERATURE の削除を推奨)"
                        .into(),
                );
            }
        }
        out
    }

    /// あらすじ要約用の設定 (spec 10)。`SUMMARY_LLM_MODEL` か `SUMMARY_LLM_BASE_URL` が
    /// 設定されていれば `Some` — 未指定フィールドは GM (本体) 設定から継承する
    /// (「安いモデルだけ差し替える」が SUMMARY_LLM_MODEL 1 行で書ける)。
    /// どちらも無ければ `None` (呼び出し側は GM の client を共用 = 受領者ゼロ設定)。
    pub fn summary_from_env(base: &LlmConfig) -> Result<Option<Self>, LlmError> {
        Self::summary_overrides(
            base,
            env_opt("SUMMARY_LLM_BASE_URL"),
            env_opt("SUMMARY_LLM_API_KEY"),
            env_opt("SUMMARY_LLM_MODEL"),
            env_opt("SUMMARY_LLM_PROVIDER"),
        )
    }

    /// [`Self::summary_from_env`] の純粋ロジック (env 非依存・テスト可)。
    /// provider は明示 > 実効 base_url からの自動判定 (本体の provider を継がない —
    /// base_url が変われば話すべきプロトコルも変わる)。
    pub fn summary_overrides(
        base: &LlmConfig,
        base_url: Option<String>,
        api_key: Option<String>,
        model: Option<String>,
        provider: Option<String>,
    ) -> Result<Option<Self>, LlmError> {
        if base_url.is_none() && model.is_none() {
            return Ok(None);
        }
        let effective_url = base_url.unwrap_or_else(|| base.base_url.clone());
        let provider = match provider {
            None => Provider::detect(&effective_url),
            Some(raw) => Provider::parse_env(&raw, "SUMMARY_LLM_PROVIDER")?,
        };
        // tool_mode が答えるのは「**この接続先**は tool_choice をどこまで受けるか」なので、
        // 実効 url が本体と同じなら答えも同じ = そのまま継ぐ (LLM_TOOL_MODE の明示や
        // LLM_USE_TOOLS=false を黙って捨てない)。url が変われば provider と同じ理由で再判定する。
        let tool_mode = if effective_url == base.base_url {
            base.tool_mode
        } else {
            ToolMode::detect(&effective_url)
        };
        Ok(Some(Self {
            base_url: effective_url,
            api_key: api_key.unwrap_or_else(|| base.api_key.clone()),
            model: model.unwrap_or_else(|| base.model.clone()),
            temperature: base.temperature,
            max_tokens: base.max_tokens,
            request_timeout: base.request_timeout,
            max_retries: base.max_retries,
            // provider と同じ理由で **実効 base_url から再判定** — url が変われば
            // tool_choice の実装範囲も変わる (要約だけ別プロバイダに投げる構成が壊れない)。
            tool_mode,
            provider,
            // 要約は安い/速い設定が目的 — GM の effort は継がない (深い思考は要約に不要)。
            effort: None,
            // spec 13: cache 設定は本体から継承 (要約は小プレフィックスゆえサイズゲートが自然に Bypass)。
            gemini_cache_enabled: base.gemini_cache_enabled,
            gemini_cache_ttl_secs: base.gemini_cache_ttl_secs,
            gemini_cache_min_chars: base.gemini_cache_min_chars,
        }))
    }

    /// テスト・明示構築用。`base_url` と `api_key` を与えて残りは既定値 (provider は自動判定)。
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>) -> Self {
        let base_url = base_url.into();
        let provider = Provider::detect(&base_url);
        let tool_mode = ToolMode::detect(&base_url);
        Self {
            base_url,
            api_key: api_key.into(),
            model: model.into(),
            temperature: None,
            max_tokens: 4096,
            request_timeout: Duration::from_secs(120),
            max_retries: 3,
            tool_mode,
            provider,
            effort: None,
            gemini_cache_enabled: true,
            gemini_cache_ttl_secs: 900,
            gemini_cache_min_chars: 4000,
        }
    }

    /// `{base_url}/chat/completions` を組み立てる (末尾スラッシュを正規化)。
    pub fn chat_endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    /// `{base_url}/messages` (Anthropic ネイティブ) を組み立てる (末尾スラッシュを正規化)。
    pub fn messages_endpoint(&self) -> String {
        format!("{}/messages", self.base_url.trim_end_matches('/'))
    }

    /// `{base_url}/responses` (OpenAI Responses 形。Perplexity は `/v1/responses`) を組み立てる。
    /// base_url がホスト直 (`https://api.perplexity.ai`) なら `/v1` を補う — Gemini の
    /// `/v1beta` 補完と同じ受領者ゼロ設定。
    pub fn responses_endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{base}/responses")
        } else {
            format!("{base}/v1/responses")
        }
    }

    /// Gemini ネイティブ `generateContent` エンドポイントを組み立てる (spec 12 Phase C)。
    /// base_url はホスト直 (`https://generativelanguage.googleapis.com`) と `/v1beta` 込みの
    /// 両方を受ける — 受領者ゼロ設定 (ホストだけ書けば動く) と明示派の両対応。
    pub fn gemini_endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/v1beta") {
            format!("{base}/models/{}:generateContent", self.model)
        } else {
            format!("{base}/v1beta/models/{}:generateContent", self.model)
        }
    }

    /// Gemini 明示キャッシュ作成エンドポイント `{base}/v1beta/cachedContents` (spec 13)。
    /// [`Self::gemini_endpoint`] と同じく base_url はホスト直 / `/v1beta` 込みの両方を受ける。
    pub fn cachedcontents_endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/v1beta") {
            format!("{base}/cachedContents")
        } else {
            format!("{base}/v1beta/cachedContents")
        }
    }
}

/// [`ToolMode`] の決定 (純粋・テスト可)。優先順は **明示 > 旧キー > 自動判定**:
///
/// 1. `LLM_TOOL_MODE` — 誤値は起動時に弾く (ネットワーク前)
/// 2. `LLM_USE_TOOLS` — **false 系だけを見る**。旧キーの `true` は「tools を使う」の意味しか
///    持たず、名前指定が通るかどうかは言っていないので、自動判定に委ねる (既存 .env が
///    `LLM_USE_TOOLS=true` のまま Meta を指しても正しく `Auto` に落ちる)
/// 3. base_url からの自動判定 ([`ToolMode::detect`])
pub(crate) fn resolve_tool_mode(
    tool_mode: Option<String>,
    use_tools: Option<String>,
    base_url: &str,
) -> Result<ToolMode, LlmError> {
    if let Some(raw) = tool_mode {
        return ToolMode::parse_env(&raw, "LLM_TOOL_MODE");
    }
    let disabled = use_tools.is_some_and(|v| {
        matches!(v.trim().to_ascii_lowercase().as_str(), "false" | "0" | "no" | "off")
    });
    Ok(if disabled { ToolMode::Off } else { ToolMode::detect(base_url) })
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn env_parse<T>(key: &str, default: T) -> Result<T, LlmError>
where
    T: std::str::FromStr,
{
    match env_opt(key) {
        None => Ok(default),
        Some(raw) => raw
            .parse::<T>()
            .map_err(|_| LlmError::Config(format!("環境変数 {key} の値 '{raw}' を解釈できません"))),
    }
}
