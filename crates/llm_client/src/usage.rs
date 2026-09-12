//! spec 30 (2026-09-13 rev2): 利用量の計器。**足し算とその置き場**だけを持つ — 価格表は持たない
//! (見積もりは提示層がユーザーの書いた単価で行う。既定価格を焼くと「間違った金額は無いより悪い」)。
//!
//! - [`LlmLedger`] = client インスタンス単位の累計 (プロセス内揮発。セーブには入れない)。
//!   client は役割ごとに別インスタンス (gm / summary / editor / image_prompt) なので、役割別の
//!   集計はここを読むだけで割れる。セッション合計は全 client を [`LlmLedger::absorb`] で足す。
//! - [`ImageLedger`] = 画像生成の累計。`LlmClient` を通らないので置き場は app のセッション状態
//!   (spec 30 Phase B の `UsageState`)。
//! - [`UsageEvent`] = **1 イベント 1 行** (jsonl)。app が sink を配り、llm_client は
//!   [`crate::LlmClient`] の記録点から `Llm` を、image_gen (Phase B) が `Image` を流す。
//!   費用の置き場は [`Usage::cost_usd`] の 1 箇所だけ (rev2 — Llm イベントに二重に持たない)。
//!   **プロンプト本文は型として載らない** (ログに秘密が混ざる経路を作らない)。
//! - `Search` は v1 では emit しない (予約)。Perplexity の申告費用はリクエスト全体の値なので
//!   `Usage.cost_usd` に載る — 同じリクエストから 2 イベントを出すと jsonl の合計が二重になる。
//! - [`LoggedUsageEvent`] = jsonl の 1 行の形 (`t_ms` + flatten)。時刻はミリ秒 (秒だと同一秒の
//!   順序が潰れる)。

use std::sync::Arc;

use serde::Serialize;

use crate::canonical::Usage;

/// client 単位の累計。`prompt_tokens` は総入力 (cache_read を含む = spec 14 D5 の分母と同じ正規化。
/// Anthropic の非キャッシュ分のみの `input_tokens` は adapter が足して canonical へ渡す = #58。
/// ここでは足し直さない)。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct LlmLedger {
    pub requests: u64,
    pub prompt_tokens: u64,
    pub cache_read_tokens: u64,
    /// 出力トークン。2026-09-13 まで**どこにも足されていなかった** (CacheStat は入力側だけ)。
    pub completion_tokens: u64,
    /// プロバイダが申告した費用 (USD) の合計。申告するのは今は Perplexity (responses) だけで、
    /// 申告の無いリクエストは 0 として足す。**加算経路は `Llm` だけ** (`Image` は費用欄を持たず、
    /// `Search` は emit しない)。見積もり (単価 × トークン) はここに混ぜない。
    pub reported_cost_usd: f64,
    /// 費用を申告したリクエスト数。0 なら `reported_cost_usd` は意味を持たない。
    /// `0 < reported < requests` は「部分申告」= 提示層は見積もらず、その旨を表示する (spec 30 C)。
    pub reported_cost_requests: u64,
}

impl LlmLedger {
    pub fn record(&mut self, u: &Usage) {
        self.requests += 1;
        self.prompt_tokens += u.prompt;
        self.cache_read_tokens += u.cache_read;
        self.completion_tokens += u.completion;
        if let Some(c) = u.cost_usd {
            self.reported_cost_usd += c;
            self.reported_cost_requests += 1;
        }
    }

    /// 別 client の累計を足す (セッション合計 = 全 client を absorb)。
    pub fn absorb(&mut self, other: &LlmLedger) {
        self.requests += other.requests;
        self.prompt_tokens += other.prompt_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.completion_tokens += other.completion_tokens;
        self.reported_cost_usd += other.reported_cost_usd;
        self.reported_cost_requests += other.reported_cost_requests;
    }
}

/// 画像生成の累計 (app セッション単位)。トークンは提供者が返すときだけ (ComfyUI は None)。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImageLedger {
    pub requests: u64,
    pub count: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub elapsed_sec: f64,
}

impl ImageLedger {
    pub fn record(
        &mut self,
        count: u32,
        prompt_tokens: Option<u64>,
        completion_tokens: Option<u64>,
        elapsed_sec: Option<f64>,
    ) {
        self.requests += 1;
        self.count += u64::from(count);
        self.prompt_tokens += prompt_tokens.unwrap_or(0);
        self.completion_tokens += completion_tokens.unwrap_or(0);
        self.elapsed_sec += elapsed_sec.unwrap_or(0.0);
    }
}

/// 1 イベント 1 行。jsonl は `{"kind": "llm" | "image" | "search", ...}` の形 (Phase D の
/// 集計がこの形に依存する = PoC で固定)。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UsageEvent {
    Llm {
        /// `gm` / `summary` / `editor` / `image_prompt` (app が [`crate::LlmClient::with_role`] で付ける)。
        role: String,
        model_id: String,
        /// 費用 (申告) は `usage.cost_usd` に載る — ここに二重に持たない (rev2)。
        usage: Usage,
    },
    Image {
        /// 常に `illustration` (画像だけに現れる役割)。
        role: String,
        provider: String,
        count: u32,
        prompt_tokens: Option<u64>,
        completion_tokens: Option<u64>,
        elapsed_sec: Option<f64>,
    },
    /// 予約 (v1 では emit しない)。
    Search { provider: String, cost_usd: f64 },
}

/// jsonl の 1 行。`t_ms` と `kind` が同じ段に並ぶ (flatten)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LoggedUsageEvent {
    /// unix ミリ秒。
    pub t_ms: u64,
    #[serde(flatten)]
    pub event: UsageEvent,
}

/// app が配る受け口。llm_client は呼ぶだけで、書く先 (jsonl) を知らない。
/// **ロックの外で呼ばれる** (sink の中から [`crate::LlmClient::usage_ledger`] を読んでよい)。
pub type UsageSink = Arc<dyn Fn(&UsageEvent) + Send + Sync>;
