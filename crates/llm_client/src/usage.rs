//! spec 30 (2026-09-13): 利用量の計器。**足し算とその置き場**だけを持つ — 価格表は持たない
//! (見積もりは提示層がユーザーの書いた単価で行う。既定価格を焼くと「間違った金額は無いより悪い」)。
//!
//! - [`UsageLedger`] = client インスタンス単位の累計 (プロセス内揮発。セーブには入れない)。
//!   client は役割ごとに別インスタンス (GM / summary / editor / image_prompt) なので、役割別の
//!   集計はここを読むだけで割れる。
//! - [`UsageEvent`] = 1 リクエスト 1 イベント。app が sink (jsonl 追記・spec 30 Phase C) を配り、
//!   llm_client は [`crate::LlmClient`] の記録点から `Llm` を、image_gen (Phase B) が `Image` を流す。
//!   **プロンプト本文は型として載らない** (ログに秘密が混ざる経路を作らない)。
//! - `Search` は v1 では emit しない (予約)。Perplexity の申告費用はリクエスト全体の値なので
//!   `Llm.cost_usd` に載せる — 同じリクエストから 2 イベントを出すと jsonl の合計が二重になる。

use std::sync::Arc;

use serde::Serialize;

use crate::canonical::Usage;

/// client 単位の累計。`prompt_tokens` は総入力 (cache_read を含む = spec 14 D5 の分母と同じ正規化)。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct UsageLedger {
    pub requests: u64,
    pub prompt_tokens: u64,
    pub cache_read_tokens: u64,
    /// 出力トークン。2026-09-13 まで**どこにも足されていなかった** (CacheStat は入力側だけ)。
    pub completion_tokens: u64,
    /// プロバイダが申告した費用 (USD) の合計。申告するのは今は Perplexity (responses) だけで、
    /// 申告の無いリクエストは 0 として足す。見積もり (単価 × トークン) はここに混ぜない。
    pub reported_cost_usd: f64,
    /// 費用を申告したリクエスト数。0 なら `reported_cost_usd` は意味を持たない。
    pub reported_cost_requests: u64,
}

impl UsageLedger {
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
}

/// 1 リクエスト 1 イベント。jsonl は `{"kind": "llm" | "image" | "search", ...}` の形 (Phase D の
/// 集計がこの形に依存する = PoC で固定)。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UsageEvent {
    Llm {
        /// `gm` / `summary` / `editor` / `image_prompt` (app が [`crate::LlmClient::with_role`] で付ける)。
        role: String,
        model_id: String,
        usage: Usage,
        /// プロバイダ申告 (Perplexity)。無ければ None — 見積もりは提示層。
        cost_usd: Option<f64>,
    },
    Image {
        role: String,
        provider: String,
        count: u32,
        usage_tokens: Option<u64>,
        elapsed_sec: Option<f64>,
    },
    /// 予約 (v1 では emit しない)。
    Search { provider: String, cost_usd: f64 },
}

/// app が配る受け口。llm_client は呼ぶだけで、書く先 (jsonl) を知らない。
pub type UsageSink = Arc<dyn Fn(&UsageEvent) + Send + Sync>;
