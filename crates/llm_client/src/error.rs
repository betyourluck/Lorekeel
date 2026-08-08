//! クライアントが返す失敗の型。`Parse` は **raw を保持する** ──
//! GM ターンループが却下理由と一緒に LLM へ戻して再生成させるため (self_repair 同型)。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    /// 設定不備 (環境変数欠落など)。ネットワーク前に弾く。
    #[error("設定エラー: {0}")]
    Config(String),

    /// HTTP 層の失敗 (接続不能・タイムアウト・TLS など)。リトライ対象。
    /// `detail` は **source 連鎖を平坦化**した文面 (reqwest の "error sending request for url" は
    /// 真因をラップして隠すので、"…: operation timed out" 等の根本原因まで surface する)。
    #[error("HTTP エラー: {detail}")]
    Http {
        #[source]
        source: reqwest::Error,
        detail: String,
    },

    /// API がエラーステータスを返した。`status` でリトライ可否を判断する。
    #[error("API エラー (status={status}): {body}")]
    Api { status: u16, body: String },

    /// 応答が空 (`generate` の text 空など、理由が length 以外)。再抽選で回復しうるので一過性。
    #[error("LLM が空の応答を返した")]
    EmptyResponse,

    /// **出力上限に達して、narration も ops も成立しなかった** (finish=length)。
    ///
    /// 原因は 2 通りが同じワイヤ形になる — (a) 推論モデルが出力予算を思考に使い切った
    /// (b) 生成物が上限で切れた。**どちらも同じ入力なら同じ所で切れる**ので**非一過性**
    /// (2026-08-08 に `EmptyResponse` から分離。畳んでいた頃はリトライがバックオフと課金だけを
    /// 増やし、画面には「空の応答」としか出ず受領者が上限に辿り着けなかった)。
    /// `limit` を載せるのは、次の一手が「`LLM_MAX_TOKENS` をいくつまで上げるか」だから。
    #[error(
        "出力上限に達し応答が途中で切れた ({limit} トークン) — LLM_MAX_TOKENS を上げてください\
         (推論モデルは思考+本文の合算上限なので 16000 以上が目安)"
    )]
    OutputTruncated { limit: u32 },

    /// プロバイダが応答をブロックした (Gemini は安全フィルタ/規約でも **200 + 空応答**で
    /// 返すため、理由を捨てると一律「空の応答」になり診断不能 — あらすじ要約の恒久失敗の真因
    /// が見えなかった)。理由 (SAFETY/RECITATION/PROHIBITED_CONTENT 等) を surface する。
    /// 同じ内容の再送では回復しないので非一過性。
    #[error("プロバイダが応答をブロックした (理由: {reason}) — 内容が安全フィルタ/利用規約に触れた可能性")]
    Blocked { reason: String },

    /// tool-use を強制したのに tool_calls もフェンス JSON も得られなかった。
    #[error("構造化出力が得られなかった (tool_call 不在かつ content も JSON でない)")]
    NoStructuredOutput,

    /// JSON のパースに失敗。**`raw` を保持** して再生成プロンプトに添えられるようにする。
    #[error("構造化出力のパース失敗: {source}\n--- raw ---\n{raw}")]
    Parse {
        source: serde_json::Error,
        raw: String,
    },
}

/// reqwest エラーから [`LlmError::Http`] を作る。source 連鎖を平坦化して `detail` に詰める
/// ("error sending request for url (…)" の下に隠れた timeout/connect/TLS の真因まで見せる)。
impl From<reqwest::Error> for LlmError {
    fn from(source: reqwest::Error) -> Self {
        let mut detail = source.to_string();
        let mut cause: Option<&dyn std::error::Error> = std::error::Error::source(&source);
        while let Some(inner) = cause {
            detail.push_str(": ");
            detail.push_str(&inner.to_string());
            cause = inner.source();
        }
        LlmError::Http { source, detail }
    }
}

impl LlmError {
    /// 一過性 (リトライで回復しうる) か。HTTP 障害と 5xx / 429、および理由不明の空応答
    /// (再抽選で回復しうる) を対象とする。**`OutputTruncated` は含めない** — 上限は入力に
    /// 対して決定的で、再送すれば同じ所で切れる。
    pub fn is_transient(&self) -> bool {
        match self {
            LlmError::Http { source, .. } => {
                source.is_timeout() || source.is_connect() || source.is_request()
            }
            LlmError::Api { status, .. } => *status == 429 || (500..600).contains(status),
            LlmError::EmptyResponse => true,
            _ => false,
        }
    }
}
