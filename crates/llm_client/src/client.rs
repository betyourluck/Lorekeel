//! HTTP クライアント本体。OpenAI 互換 chat/completions を叩き、指数 backoff でリトライする。
//!
//! `AsyncOpenAI` (Python) の Rust 版。ネットワーク経路は実キーが要るため単体テスト対象外
//! (壊れる ser/de は wire.rs / parse.rs 側で固める)。実 API 通しは「実クラウド通しプレイ」フェーズ。

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Mutex;

use crate::anthropic;
use crate::canonical;
use crate::config::{LlmConfig, Provider, ToolMode};
use crate::error::LlmError;
use crate::gemini;
use crate::openai_compat;
use crate::parse;
use crate::responses;
use crate::wire::{ChatMessage, ChatRequest, ChatResponse};

/// プロンプトキャッシュの健全性の計測値。`cache_read`>0 = 安定プレフィックスがキャッシュから
/// 読まれた (入力コスト減)。GUI が**連続 miss** を検知して「キャッシュ経路が壊れているかも」を
/// 警告する材料になる — #44 (Anthropic 互換層は caching 非対応) / #45 (xAI は sticky ヘッダ必須)
/// の「キャッシュの静かな漏出は usage が一次ソース」を GUI へ引き上げる。
///
/// spec 14 Phase C: hit rate 曲線の観測を追加。**定義は D5 で凍結** —
/// `cached = cache_read` (読取 0.1× のみ。書込 1.25× の cache_creation は「hit」に含めない =
/// 章追加ターンの再 warm を誤計上しない) / `prompt = input_tokens` (総入力)。
/// per-request 履歴は**有界** ([`Self::RECENT_CAP`] のリングバッファ) — 長セッションで
/// 常駐メモリを伸ばさない。ドル建てコストは出さない (provider 価格は変動・stale 化)。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CacheStat {
    /// 直近リクエストの cache read トークン (0 = miss)。
    pub last_cache_read: u64,
    /// 連続で cache read が 0 だった回数 (1 回でもヒットで 0 にリセット)。
    pub consecutive_misses: u32,
    /// 累計リクエスト数。初回は書き込みゆえ miss が正常なので、判定は 2 回目以降を見る。
    pub total_requests: u32,
    /// 累積キャッシュ読取トークン (spec 14)。セッション累積 hit rate の分子。
    pub hit_tokens: u64,
    /// 累積入力トークン (spec 14)。分母 — `hit_tokens / total_tokens` = セッションのキャッシュ率。
    pub total_tokens: u64,
    /// 直近 [`Self::RECENT_CAP`] 件の per-request 記録 (spec 14)。hit rate 曲線の可視化用。
    pub recent: Vec<CachePoint>,
    /// キャッシュ最小プレフィックス (トークン)。**これ未満の prompt は構造的にヒット不可能**
    /// なので miss として数えない (Perplexity 8192 ブロック床 #80 / Anthropic 最小 4096 #44。
    /// 0 = 床なし = 全 miss を数える従来挙動)。値は [`LlmClient::new`] が provider から決める。
    /// DTO には出さない (警告条件は consecutive_misses のままで frontend 無改修)。
    #[serde(skip)]
    pub floor: u64,
}

/// 1 リクエスト分のキャッシュ計測点 (spec 14)。`cached / prompt` = そのリクエストの hit rate。
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CachePoint {
    /// cache read トークン (D5: cache_creation は含めない)。
    pub cached: u64,
    /// 総入力トークン。
    pub prompt: u64,
}

impl CacheStat {
    /// リングバッファの上限。曲線の可視化に足りる小ささ (無制限履歴は持たない — rev2 W4)。
    pub const RECENT_CAP: usize = 32;

    /// 1 リクエスト分の cache read / 総入力を記録する (純粋・テスト可)。
    pub(crate) fn record(&mut self, cache_read: u64, prompt: u64) {
        self.total_requests = self.total_requests.saturating_add(1);
        self.last_cache_read = cache_read;
        if cache_read > 0 {
            self.consecutive_misses = 0;
        } else if prompt >= self.floor {
            self.consecutive_misses = self.consecutive_misses.saturating_add(1);
        }
        // 床未満の miss は数えない — キャッシュ対象外のサイズで鳴る警告は誤報 (累積とリングは正直に積む)。
        self.hit_tokens = self.hit_tokens.saturating_add(cache_read);
        self.total_tokens = self.total_tokens.saturating_add(prompt);
        self.recent.push(CachePoint { cached: cache_read, prompt });
        if self.recent.len() > Self::RECENT_CAP {
            self.recent.remove(0);
        }
    }
}

/// provider ごとのキャッシュ最小プレフィックス (トークン)。**実測/公式で確定した床だけ**を持ち、
/// 未測は 0 (= 従来どおり全 miss を数える)。表を広げるのは観測が出てから — 床を盛りすぎると
/// 真の miss (設定ミス) まで隠す。
/// - Perplexity: 8192 トークンブロック単位でしか cached が返らない (failures #80 実測)
/// - Anthropic: 最小キャッシュ 4096 tokens (opus 系公式 #44。小パッケージの偽警告の既知留意も解消)
pub(crate) fn cache_floor(config: &LlmConfig) -> u64 {
    match config.provider {
        Provider::Responses if config.base_url.contains("api.perplexity.ai") => 8192,
        // Meta はキャッシュが**口を問わず不安定** (2026-08-21 実測 15 リクエスト: 実プレイ形
        // 〔固定プレフィックス+可変 user〕は 0/9、完全同一の再送ですら 2/4。互換口の
        // 「たまに効く」〔2026-08-09〕と同じ側)。受領者が設定で直せる故障ではないので
        // miss を一切数えない = 警告の対象外 (ヒットすれば計測には正直に載る)。
        Provider::Responses if config.base_url.contains("api.meta.ai") => u64::MAX,
        Provider::Anthropic => 4096,
        _ => 0,
    }
}

/// クラウド LLM ナレーター脚。
pub struct LlmClient {
    http: reqwest::Client,
    config: LlmConfig,
    /// セッション識別子。OpenAI 互換経路で `x-grok-conv-id` として送る (#45) —
    /// xAI のキャッシュは**サーバ単位**で、このヘッダが無いとロードバランサで散って
    /// 同一プレフィックスでも miss する (sticky routing)。xAI 以外は未知ヘッダとして無視。
    /// クライアントは app=ゲームセッション毎 / CLI=実行毎に作られるので粒度が会話に一致する。
    conv_id: String,
    /// キャッシュ健全性の計測 (interior mutability — propose は `&self`)。両経路のリクエストで
    /// cache read を記録し、GUI が連続 miss を警告に出す。
    cache_stat: Mutex<CacheStat>,
    /// Gemini の呼び出し id 合成用の単調カウンタ (spec 12 rev4 Must 4)。リクエスト毎に
    /// リセットしない — 却下→再生成の同一ターン内で `call_0` が重複しないため。
    call_seq: std::sync::atomic::AtomicU64,
    /// spec 13: Gemini 明示キャッシュのセッションハンドル。fingerprint が現在の静的プレフィックスと
    /// 一致すれば reuse、違えば作り直す (campaign 遷移等)。失効時はクリアして full request へ透過。
    gemini_cache: Mutex<Option<gemini::CacheHandle>>,
    /// 盤面の判定様式による **追加除外 op** (spec 16)。`check_style: percentile` の盤面は
    /// `["check"]`、additive (既定) は `["check_under"]` — 使わない様式の判定 op を schema から
    /// 落とし、LLM に様式を混ぜさせない (AUTHORED_ONLY_OPS の除外に合算)。セッション内不変
    /// (new_game 時に確定) なので schema = 静的プレフィックス性は保たれる。
    excluded_ops: Vec<String>,
    /// **実効の [`ToolMode`] (セッション内 latch)**。初期値は設定由来 (明示 > 旧キー > base_url
    /// 自動判定) で、`tool_choice` を名指しした 400 を受けたら一段降格して**ここへ覚える** —
    /// ホスト名で判定できない経路 (中継・自前プロキシ) の受け皿。覚えるので余計な往復は
    /// セッションで高々 2 回 (Forced→Auto→Off)。名前指定が通るサーバでは 400 が来ないので
    /// **一度も発火しない**。`gemini_cache` と同じ interior mutability の流儀。
    tool_mode: Mutex<ToolMode>,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .build()?;
        let config_tool_mode = config.tool_mode;
        let floor = cache_floor(&config);
        Ok(Self {
            http,
            config,
            conv_id: new_conv_id(),
            cache_stat: Mutex::new(CacheStat { floor, ..CacheStat::default() }),
            call_seq: std::sync::atomic::AtomicU64::new(0),
            gemini_cache: Mutex::new(None),
            tool_mode: Mutex::new(config_tool_mode),
            // 既定 = additive 盤面 (従来どおり)。percentile 判定 op は隠す。
            excluded_ops: vec!["check_under".to_string()],
        })
    }

    pub fn config(&self) -> &LlmConfig {
        &self.config
    }

    /// 盤面の判定様式による追加除外 op を設定する (spec 16)。new_game でシナリオを読んだ
    /// 呼び出し側 (app/CLI) が `check_style` から決める: percentile → `["check"]` /
    /// additive → `["check_under"]`。セッション開始時に一度だけ呼ぶ (schema の安定性)。
    pub fn set_excluded_ops(&mut self, ops: Vec<String>) {
        self.excluded_ops = ops;
    }

    /// 現在の追加除外 op (schema 構築用)。
    pub(crate) fn excluded_ops(&self) -> &[String] {
        &self.excluded_ops
    }

    /// セッション識別子 (x-grok-conv-id に載せる値)。
    pub fn conv_id(&self) -> &str {
        &self.conv_id
    }

    /// キャッシュ健全性のスナップショット (GUI の警告判定用)。lock 毒化時は既定値。
    pub fn cache_stat(&self) -> CacheStat {
        self.cache_stat.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// 1 リクエスト分の cache read / 総入力を計測に記録する ([`Self::complete`] が canonical
    /// usage から一元で呼ぶ)。`LLM_CACHE_DEBUG=1` なら**機械可読の 1 行** (spec 14 Phase C:
    /// req 連番 + unix 秒 + input + cache_read + per-req ratio + 累積 ratio) を stderr へ —
    /// 長セッションの hit rate 曲線をログから grep で再構成できる (provider 別の生 usage 行と併存)。
    /// `t=` (unix 秒) は Phase D の仮説弁別用: 自動キャッシュの miss が**リクエスト間隔**
    /// (TTL/eviction) と相関するか、間隔非依存 (ルーティング分散) かを人間ペースのプレイで切り分ける
    /// (Grok 実測 2026-07-16: 間隔非依存で TTL 説棄却 = 分散/eviction、failures #57)。
    /// `conv=` は client の系列弁別用: summary 用 client (spec 10) が同じ stderr に
    /// **req 連番を 1 から別カウント**して混ざるため、grep 集計はこのキーで系列を分ける。
    fn record_cache(&self, cache_read: u64, prompt: u64) {
        if let Ok(mut g) = self.cache_stat.lock() {
            g.record(cache_read, prompt);
            if std::env::var("LLM_CACHE_DEBUG").is_ok() {
                let ratio = |c: u64, p: u64| if p > 0 { c as f64 / p as f64 } else { 0.0 };
                let t = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                eprintln!(
                    "[LLM_CACHE_STAT] conv={} req={} t={} input={} cache_read={} ratio={:.3} cum_ratio={:.3}",
                    self.conv_id,
                    g.total_requests,
                    t,
                    prompt,
                    cache_read,
                    ratio(cache_read, prompt),
                    ratio(g.hit_tokens, g.total_tokens),
                );
            }
        }
    }

    /// プレーンなテキスト生成 (`generate`)。ツール無し。
    pub async fn generate(&self, messages: Vec<ChatMessage>) -> Result<String, LlmError> {
        let req = canonical::ChatRequest {
            model: self.config.model.clone(),
            messages,
            tools: Vec::new(),
            tool_choice: canonical::ToolChoice::None,
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            effort: self.config.effort,
        };
        let resp = self.complete(req).await?;
        resp.text
            .filter(|c| !c.trim().is_empty())
            .ok_or(LlmError::EmptyResponse)
    }

    /// 構造化出力 (`generate_json` の Rust 版)。
    ///
    /// 単一ツール `tool_name` を `tool_choice` で強制し、`parameters` schema に沿わせる。
    /// 応答は tool_calls もしくはフェンス JSON から `T` に解決する (抽出は canonical に
    /// 対する単一経路 [`parse::extract`])。
    pub async fn generate_structured<T: DeserializeOwned>(
        &self,
        messages: Vec<ChatMessage>,
        tool_name: &str,
        tool_description: &str,
        parameters: serde_json::Value,
    ) -> Result<T, LlmError> {
        let req = canonical::ChatRequest {
            model: self.config.model.clone(),
            messages,
            tools: vec![canonical::ToolSpec {
                name: tool_name.to_string(),
                description: tool_description.to_string(),
                parameters,
            }],
            tool_choice: canonical::ToolChoice::Specific(tool_name.to_string()),
            // temperature は config 任せ (未設定なら送らない)。
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            effort: self.config.effort,
        };
        let resp = self.complete(req).await?;
        parse::extract::<T>(&resp)
    }

    /// canonical リクエストを 1 回完了させる — **adapter seam の単一入口** (spec 12 Phase A)。
    ///
    /// 経路選択 (provider match) はここだけ。各経路は encode 純関数 → HTTP (リトライ込み) →
    /// decode 純関数で canonical に戻る。キャッシュ計測 ([`CacheStat`]) も canonical usage から
    /// **一元記録**する (成功 1 回 = 記録 1 回。リトライの失敗試行は usage を持たないので
    /// 従来の per-成功記録と同値)。
    async fn complete(
        &self,
        req: canonical::ChatRequest,
    ) -> Result<canonical::ChatResponse, LlmError> {
        let resp = match self.config.provider {
            // Anthropic ネイティブ経路 (#44): 安定プレフィックス末尾の cache_control で
            // schema+system がキャッシュされる。tool_choice を確実に尊重するので常に tool-use
            // (use_tools は無視 = 従来動作)。effort 方言 (Phase B) も encode が持つ。
            Provider::Anthropic => {
                let native = anthropic::encode(&req);
                let raw = self.messages_with_retry(&native).await?;
                anthropic::decode(raw)
            }
            // OpenAI 互換経路: ToolMode 三値 (#29 / Meta) の分岐は encode が担う。
            // decode + 出力上限の検出は試行毎に掛かる。tool_choice 起因の 400 は降格して再送。
            Provider::OpenAiCompat => self.compat_complete(&req).await?,
            // Gemini ネイティブ経路 (Phase C) + 明示キャッシュ (spec 13): 静的プレフィックスを
            // cachedContent に pin し、暗黙キャッシュの ~8000 崖 (failures #54) を迂回する。
            Provider::Gemini => self.gemini_complete(req).await?,
            // OpenAI Responses 形 (2026-08-20): Perplexity Agent API の `/v1/responses`。
            // 常に tool-use (`required` が効く)。ToolMode の降格は持たない — tool_choice で
            // 400 を返す口ではなく (名指しは黙殺)、Off へ落ちる道が無い。
            Provider::Responses => {
                let flavor = responses::flavor_for(&self.config.base_url);
                let wire_req = responses::encode(&req, flavor);
                self.responses_with_retry(&wire_req, req.max_tokens).await?
            }
        };
        self.record_cache(resp.usage.cache_read, resp.usage.prompt);
        Ok(resp)
    }

    /// chat/completions を 1 回叩く (リトライ無し)。
    async fn chat_once(&self, req: &ChatRequest) -> Result<ChatResponse, LlmError> {
        // 診断: LLM_DEBUG が設定されていれば送信ボディと生応答を stderr に出す。
        // tool_choice/schema を受理しつつ応答形が噛み合わないサーバ (Grok 等) の切り分け用。
        let debug = std::env::var("LLM_DEBUG").is_ok();
        if debug {
            eprintln!(
                "[LLM_DEBUG] request -> {}",
                serde_json::to_string(req).unwrap_or_default()
            );
        }
        let resp = self
            .http
            .post(self.config.chat_endpoint())
            .bearer_auth(&self.config.api_key)
            // xAI の sticky routing (#45)。他サーバは未知ヘッダとして無視する。
            .header("x-grok-conv-id", &self.conv_id)
            .json(req)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status: status.as_u16(),
                body,
            });
        }
        // 常に text→parse (json() 直は使わない)。2xx なのに形が合わない応答 (Gemini の
        // content filter / 長さ切れ / quota 系の変形) でデコードに失敗した時、json() は本文を
        // 捨て「missing field `message`」だけが残る — raw を保持して真因を診断可能にする (#34)。
        let body = resp.text().await?;
        if debug {
            eprintln!("[LLM_DEBUG] response <- {body}");
        }
        let decoded = decode_chat_body(body)?;
        // surface (ネイティブ経路の [LLM_CACHE] と同形)。CacheStat への記録は canonical usage
        // から complete() が一元で行う (成功 1 回 = 記録 1 回で従来と同値)。
        if debug || std::env::var("LLM_CACHE_DEBUG").is_ok() {
            if let Some(u) = &decoded.usage {
                let cached = u
                    .prompt_tokens_details
                    .as_ref()
                    .map(|d| d.cached_tokens)
                    .unwrap_or(0);
                eprintln!(
                    "[LLM_CACHE] cached={} prompt={} completion={}",
                    cached, u.prompt_tokens, u.completion_tokens
                );
            }
        }
        Ok(decoded)
    }

    /// 指数 backoff 付きで chat を叩き canonical まで解決する。一過性エラーのみリトライ
    /// (tenacity 同型)。decode と empty-response 防御 (spec 12 Phase D — 推論モデルが
    /// budget を思考に使い切った finish=length の空応答) を**試行の中**に含めることで、
    /// 空応答が思考の再抽選に乗る (Parse エラーは非一過性のまま = 従来どおり即失敗)。
    /// OpenAI 互換経路の完了。**`tool_choice` を名指しした 400 で一段降格して再送する**
    /// (`Forced → Auto → Off`)。
    ///
    /// 動機: `tool_choice` の実装範囲はサーバごとに違い、Meta (api.llama.com) は `"auto"` 以外を
    /// 400 で拒む。ホスト名の自動判定 ([`ToolMode::detect`]) は既知の口しか救えないので、
    /// **未知のホスト (中継・自前プロキシ) は実際の拒否から学ぶ**。降格は
    /// [`Self::tool_mode`] に latch するので、余計な往復はセッションで高々 2 回。
    /// 名前指定が通るサーバでは 400 が来ないため一度も発火しない。
    async fn compat_complete(
        &self,
        req: &canonical::ChatRequest,
    ) -> Result<canonical::ChatResponse, LlmError> {
        loop {
            // lock は await を跨がない (判定して即 drop — spec 13 の gemini_cache と同じ規律)。
            let mode = *self.tool_mode.lock().expect("tool_mode lock");
            let wire_req = openai_compat::encode(req, mode);
            match self.compat_with_retry(&wire_req, req.max_tokens).await {
                Err(LlmError::Api { status: 400, body })
                    if openai_compat::blames_tool_choice(&body) =>
                {
                    let Some(next) = mode.downgrade() else {
                        // 底 (Off) でも tool_choice を名指しされた = こちらの送り分けの問題では
                        // ない。本文をそのまま返して真因を見せる。
                        return Err(LlmError::Api { status: 400, body });
                    };
                    eprintln!(
                        "[LLM_TOOL_MODE] {mode:?} が 400 で拒否されたため {next:?} へ降格します \
                         (このセッションでは以後 {next:?} で送ります): {body}"
                    );
                    *self.tool_mode.lock().expect("tool_mode lock") = next;
                }
                other => return other,
            }
        }
    }

    async fn compat_with_retry(
        &self,
        req: &ChatRequest,
        limit: u32,
    ) -> Result<canonical::ChatResponse, LlmError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let result = match self.chat_once(req).await {
                Ok(raw) => openai_compat::decode(raw)
                    .and_then(|r| openai_compat::reject_empty_reasoning(r, limit)),
                Err(e) => Err(e),
            };
            match result {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    if attempt >= self.config.max_retries || !e.is_transient() {
                        return Err(e);
                    }
                    // 1s, 2s, 4s ... 上限 10s (wait_exponential(min=1, max=10) 同型)。
                    let secs = (1u64 << (attempt - 1)).min(10);
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                }
            }
        }
    }

    // --- Anthropic ネイティブ Messages API (#44) --------------------------------

    /// `POST {base_url}/messages` を 1 回叩く (リトライ無し)。
    /// 認証は Bearer でなく `x-api-key` + `anthropic-version` (ネイティブ API の作法)。
    async fn messages_once(
        &self,
        req: &anthropic::MessagesRequest,
    ) -> Result<anthropic::MessagesResponse, LlmError> {
        let debug = std::env::var("LLM_DEBUG").is_ok();
        if debug {
            eprintln!(
                "[LLM_DEBUG] request -> {}",
                serde_json::to_string(req).unwrap_or_default()
            );
        }
        let resp = self
            .http
            .post(self.config.messages_endpoint())
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", anthropic::ANTHROPIC_VERSION)
            .json(req)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::Api { status: status.as_u16(), body });
        }
        let body = resp.text().await?;
        if debug {
            eprintln!("[LLM_DEBUG] response <- {body}");
        }
        let decoded = decode_messages_body(body)?;
        // surface: LLM_CACHE_DEBUG=1 (または LLM_DEBUG) で stderr に 1 行。CacheStat への
        // 記録は canonical usage から complete() が一元で行う。
        if debug || std::env::var("LLM_CACHE_DEBUG").is_ok() {
            if let Some(u) = &decoded.usage {
                eprintln!(
                    "[LLM_CACHE] cache_read={} cache_write={} input={} output={}",
                    u.cache_read_input_tokens,
                    u.cache_creation_input_tokens,
                    u.input_tokens,
                    u.output_tokens
                );
            }
        }
        Ok(decoded)
    }

    /// 指数 backoff 付きで Messages API を叩く ([`Self::chat_with_retry`] のネイティブ版)。
    async fn messages_with_retry(
        &self,
        req: &anthropic::MessagesRequest,
    ) -> Result<anthropic::MessagesResponse, LlmError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.messages_once(req).await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    if attempt >= self.config.max_retries || !e.is_transient() {
                        return Err(e);
                    }
                    let secs = (1u64 << (attempt - 1)).min(10);
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                }
            }
        }
    }

    // --- OpenAI Responses 形 (Perplexity `/v1/responses`、2026-08-20) ------------------

    /// `POST {base_url}/responses` を 1 回叩く (リトライ無し)。認証は Bearer。
    async fn responses_once(
        &self,
        req: &responses::ResponsesRequest,
    ) -> Result<responses::ResponsesResponse, LlmError> {
        let debug = std::env::var("LLM_DEBUG").is_ok();
        if debug {
            eprintln!(
                "[LLM_DEBUG] request -> {}",
                serde_json::to_string(req).unwrap_or_default()
            );
        }
        let resp = self
            .http
            .post(self.config.responses_endpoint())
            .bearer_auth(&self.config.api_key)
            .json(req)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::Api { status: status.as_u16(), body });
        }
        let body = resp.text().await?;
        if debug {
            eprintln!("[LLM_DEBUG] response <- {body}");
        }
        let decoded = decode_responses_body(body)?;
        if debug || std::env::var("LLM_CACHE_DEBUG").is_ok() {
            if let Some(u) = &decoded.usage {
                let cached = u
                    .input_tokens_details
                    .as_ref()
                    .map(|d| d.cached_tokens.max(d.cache_read_input_tokens))
                    .unwrap_or(0);
                eprintln!(
                    "[LLM_CACHE] cached={} input={} output={}",
                    cached, u.input_tokens, u.output_tokens
                );
            }
        }
        Ok(decoded)
    }

    /// 指数 backoff 付きで Responses を叩き canonical まで解決する ([`Self::compat_with_retry`]
    /// 同型 — decode と出力上限の検出を**試行の中**に含める)。
    async fn responses_with_retry(
        &self,
        req: &responses::ResponsesRequest,
        limit: u32,
    ) -> Result<canonical::ChatResponse, LlmError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let result = match self.responses_once(req).await {
                Ok(raw) => responses::decode(raw)
                    .and_then(|r| openai_compat::reject_empty_reasoning(r, limit)),
                Err(e) => Err(e),
            };
            match result {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    if attempt >= self.config.max_retries || !e.is_transient() {
                        return Err(e);
                    }
                    let secs = (1u64 << (attempt - 1)).min(10);
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                }
            }
        }
    }

    // --- Gemini ネイティブ generateContent (spec 12 Phase C) ------------------------

    /// `POST {base}/v1beta/models/{model}:generateContent` を 1 回叩く (リトライ無し)。
    /// 認証は **`x-goog-api-key` ヘッダ** — キーを URL クエリに載せない (K5。ログ/プロキシ
    /// へのキー露出を避ける。live 確証は Phase E、通らなければ query key へ改訂)。
    async fn gemini_once(
        &self,
        req: &gemini::GenerateContentRequest,
    ) -> Result<gemini::GenerateContentResponse, LlmError> {
        let debug = std::env::var("LLM_DEBUG").is_ok();
        if debug {
            eprintln!(
                "[LLM_DEBUG] request -> {}",
                serde_json::to_string(req).unwrap_or_default()
            );
        }
        let resp = self
            .http
            .post(self.config.gemini_endpoint())
            .header("x-goog-api-key", &self.config.api_key)
            .json(req)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::Api { status: status.as_u16(), body });
        }
        let body = resp.text().await?;
        if debug {
            eprintln!("[LLM_DEBUG] response <- {body}");
        }
        let decoded = decode_gemini_body(body)?;
        // surface (#44/#45 と同形)。Gemini 2.5 系は暗黙キャッシュが自動 —
        // cachedContentTokenCount > 0 = プレフィックスがキャッシュから読まれた。
        if debug || std::env::var("LLM_CACHE_DEBUG").is_ok() {
            if let Some(u) = &decoded.usage_metadata {
                eprintln!(
                    "[LLM_CACHE] cached={} prompt={} completion={}",
                    u.cached_content_token_count, u.prompt_token_count, u.candidates_token_count
                );
            }
        }
        Ok(decoded)
    }

    /// 指数 backoff 付きで generateContent を叩く ([`Self::chat_with_retry`] の Gemini 版)。
    async fn gemini_with_retry(
        &self,
        req: &gemini::GenerateContentRequest,
    ) -> Result<gemini::GenerateContentResponse, LlmError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.gemini_once(req).await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    if attempt >= self.config.max_retries || !e.is_transient() {
                        return Err(e);
                    }
                    let secs = (1u64 << (attempt - 1)).min(10);
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                }
            }
        }
    }

    /// Gemini リクエストを明示キャッシュ (spec 13) 込みで完了させる。fingerprint で cache を
    /// reuse/再作成し、systemInstruction+tools を cachedContent に載せて generateContent は
    /// 可変 contents だけ送る。作成失敗・サイズゲート未満・無効化は full request に fallback
    /// (キャッシュは最適化であって正しさの前提ではない — turn は絶対に落とさない)。
    async fn gemini_complete(
        &self,
        req: canonical::ChatRequest,
    ) -> Result<canonical::ChatResponse, LlmError> {
        let fp = gemini::fingerprint(&req);
        let static_chars = gemini::static_prefix_chars(&req);
        // std Mutex guard は await を跨げない — 判定だけ lock 内で済ませて即 drop。
        let action = {
            let guard = self.gemini_cache.lock();
            let handle = guard.as_ref().ok().and_then(|g| g.as_ref());
            gemini::decide_cache_action(
                self.config.gemini_cache_enabled,
                self.config.gemini_cache_min_chars,
                static_chars,
                fp,
                handle,
            )
        };
        let cache_name = match action {
            gemini::CacheAction::Reuse(name) => Some(name),
            gemini::CacheAction::Bypass => None,
            gemini::CacheAction::Create => match self.gemini_create_cache(&req, fp).await {
                Ok(name) => Some(name),
                Err(e) => {
                    if std::env::var("LLM_CACHE_DEBUG").is_ok() {
                        eprintln!("[LLM_CACHE] cachedContent 作成失敗 → full request にフォールバック: {e}");
                    }
                    None
                }
            },
        };

        let native = gemini::encode_with_cache(&req, cache_name.clone());
        let raw = match self.gemini_with_retry(&native).await {
            Ok(r) => r,
            // cache 参照の失効兆候 → handle をクリアして full request で 1 回だけ再試行 (透過)。
            Err(e) if cache_name.is_some() && gemini::is_cache_miss_error(&e) => {
                if let Ok(mut g) = self.gemini_cache.lock() {
                    *g = None;
                }
                if std::env::var("LLM_CACHE_DEBUG").is_ok() {
                    eprintln!("[LLM_CACHE] cachedContent 失効 → full request で再試行");
                }
                self.gemini_with_retry(&gemini::encode(&req)).await?
            }
            Err(e) => return Err(e),
        };
        // 安全フィルタ等のブロックは 200 + 空応答で返る — 理由を「空の応答」に潰さず surface
        // (非一過性 = 同じ内容の再送では回復しない。あらすじ要約の恒久失敗を診断可能にする)。
        if let Some(reason) = gemini::block_reason(&raw) {
            return Err(LlmError::Blocked { reason });
        }
        let seq = self.call_seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(gemini::decode(raw, seq))
    }

    /// `POST {base}/v1beta/cachedContents` で静的プレフィックスを pin し、handle を保存して
    /// resource name を返す (spec 13 Phase B)。認証は generateContent と同じ x-goog-api-key。
    async fn gemini_create_cache(
        &self,
        req: &canonical::ChatRequest,
        fp: u64,
    ) -> Result<String, LlmError> {
        let create = gemini::build_create_request(req, self.config.gemini_cache_ttl_secs);
        let resp = self
            .http
            .post(self.config.cachedcontents_endpoint())
            .header("x-goog-api-key", &self.config.api_key)
            .json(&create)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::Api { status: status.as_u16(), body });
        }
        let body = resp.text().await?;
        let parsed: gemini::CreateCacheResponse =
            serde_json::from_str(&body).map_err(|source| LlmError::Parse { source, raw: body })?;
        if parsed.name.is_empty() {
            return Err(LlmError::Api { status: 200, body: "cachedContents create: name が空".into() });
        }
        if let Ok(mut g) = self.gemini_cache.lock() {
            *g = Some(gemini::CacheHandle { name: parsed.name.clone(), fingerprint: fp });
        }
        if std::env::var("LLM_CACHE_DEBUG").is_ok() {
            eprintln!("[LLM_CACHE] cachedContent 作成: {}", parsed.name);
        }
        Ok(parsed.name)
    }
}

/// 2xx 応答の本文を [`ChatResponse`] へ。形が合わなければ [`LlmError::Parse`] で
/// **本文 (raw) を保持**する — serde の「missing field」だけでは真因 (content filter /
/// 長さ切れ等のサーバ都合の変形応答) が見えないため (#34)。
pub(crate) fn decode_chat_body(body: String) -> Result<ChatResponse, LlmError> {
    serde_json::from_str::<ChatResponse>(&body)
        .map_err(|source| LlmError::Parse { source, raw: body })
}

/// Messages API 版の [`decode_chat_body`]。同じく **raw を保持** (#34 同型)。
pub(crate) fn decode_messages_body(body: String) -> Result<anthropic::MessagesResponse, LlmError> {
    serde_json::from_str::<anthropic::MessagesResponse>(&body)
        .map_err(|source| LlmError::Parse { source, raw: body })
}

/// Responses 版の [`decode_chat_body`]。同じく **raw を保持** (#34 同型)。
pub(crate) fn decode_responses_body(body: String) -> Result<responses::ResponsesResponse, LlmError> {
    serde_json::from_str::<responses::ResponsesResponse>(&body)
        .map_err(|source| LlmError::Parse { source, raw: body })
}

/// generateContent 版の [`decode_chat_body`]。同じく **raw を保持** (#34 同型)。
pub(crate) fn decode_gemini_body(body: String) -> Result<gemini::GenerateContentResponse, LlmError> {
    serde_json::from_str::<gemini::GenerateContentResponse>(&body)
        .map_err(|source| LlmError::Parse { source, raw: body })
}

/// セッション識別子を作る。プロセス ID + 単調カウンタ + 起動時刻ナノ秒 —
/// 会話を跨いで衝突しなければよい (暗号強度は不要)。uuid 依存を増やさない。
fn new_conv_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("kataribe-{}-{}-{}", std::process::id(), nanos, n)
}

