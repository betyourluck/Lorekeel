//! TypeSafe Jev — 型付き判断の口 (Cloudflare Workers AI 経由)。
//!
//! Jev は**文章を書かない**。状態と質問を受け取り、確率つきの型付き判断だけを並列で返す。
//! ゆえに canonical な `ChatRequest` には**乗せない** — messages が無く tool-use でもないので、
//! `Provider` の match (chat のための seam) に足すと canonical が汚れる。共有するのは
//! HTTP・[`LlmError`] の一過性判定・指数 backoff・usage の形だけ。
//!
//! v1 は **Noul (真偽の度合い) のみ**。Choice / Score は Jev API に在るが、Lorekeel の
//! 一貫性検査 Phase A が使わないので実装しない — 必要になったら足す。
//!
//! # 実測で決まった使い方 (2026-09-21)
//! - **1 質問 1 論点**。複合質問だと同じ違反で 0.28、分解すると 0.95 まで上がった。
//! - **`criteria` を付ける**。第三者の独立観察 (経費チェック 20 件) でも
//!   「まとめて聞く 71% / 1 つずつ + ルール 100%」と同じ差が出ている。
//! - **`state` は構造化 JSON で渡す**。人間可読の 1 枚テキストにすると照合が効かない。
//! - 質問は**並列・独立**に評価されるので、束ねてもレイテンシはほぼ伸びない。

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::canonical::Usage;
use crate::error::LlmError;

/// Cloudflare Workers AI の既定オリジン。
pub const DEFAULT_BASE_URL: &str = "https://api.cloudflare.com/client/v4";
/// 既定のモデル名 (Workers AI のパートナーモデル識別子)。
pub const DEFAULT_MODEL: &str = "typesafe/jev";

/// 接続設定。
///
/// **キーが無ければ機構ごと存在しない** (opt-in) — `use_tts` 撤去後や画像生成と同型で、
/// 設定していない受領者に 2 つ目の API 契約を強いない。
#[derive(Debug, Clone)]
pub struct JevConfig {
    /// Cloudflare のアカウント ID (32 桁 hex)。
    pub account_id: String,
    pub api_token: String,
    pub base_url: String,
    pub model: String,
    pub timeout: Duration,
    pub max_retries: u32,
}

impl JevConfig {
    /// `JEV_ACCOUNT_ID` / `JEV_API_TOKEN` が**両方**揃ったときだけ `Some`。
    /// 片方だけは設定漏れなので `None` (黙って既定へ落ちない)。
    pub fn from_env() -> Option<Self> {
        let account_id = non_empty("JEV_ACCOUNT_ID")?;
        let api_token = non_empty("JEV_API_TOKEN")?;
        Some(Self {
            account_id,
            api_token,
            base_url: non_empty("JEV_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            model: non_empty("JEV_MODEL").unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            timeout: Duration::from_secs(
                non_empty("JEV_TIMEOUT_SECS")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(20),
            ),
            max_retries: 2,
        })
    }

    /// `POST` 先。`{base}/accounts/{id}/ai/run`。
    pub fn endpoint(&self) -> String {
        format!(
            "{}/accounts/{}/ai/run",
            self.base_url.trim_end_matches('/'),
            self.account_id
        )
    }
}

fn non_empty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Noul の判定基準。**真偽それぞれに一文**を与える (付けると精度が上がる、実測)。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub yes: String,
    #[serde(rename = "false")]
    pub no: String,
}

/// 「この文は真か」を 0.0〜1.0 で答えさせる質問。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NoulQuestion {
    /// 常に `"noul"`。Jev API は質問の型をこの欄で見る。
    #[serde(rename = "type")]
    ty: &'static str,
    pub instructions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub criteria: Option<NoulCriteria>,
}

impl NoulQuestion {
    pub fn new(instructions: impl Into<String>) -> Self {
        Self {
            ty: "noul",
            instructions: instructions.into(),
            criteria: None,
        }
    }

    /// 判定基準を添える。**付けるのが既定の作法** (付けないと精度が落ちる)。
    pub fn with_criteria(mut self, yes: impl Into<String>, no: impl Into<String>) -> Self {
        self.criteria = Some(NoulCriteria {
            yes: yes.into(),
            no: no.into(),
        });
        self
    }
}

/// 送信ボディ。`state` は任意の JSON — 構造のまま渡すと質問から
/// `inventory.player` のようにパス参照できる。
#[derive(Debug, Clone, Serialize)]
pub struct JevRequest<'a> {
    pub model: &'a str,
    pub input: JevInput<'a>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JevInput<'a> {
    pub state: &'a serde_json::Value,
    pub questions: &'a BTreeMap<String, NoulQuestion>,
}

/// 受理した答え。v1 は Noul のみなので `answers` は 0.0〜1.0 の素の値。
#[derive(Debug, Clone, PartialEq)]
pub struct JevResponse {
    /// サーバが名乗ったモデル版 (例 `jev-1.13.0`)。診断用。
    pub model: String,
    pub answers: BTreeMap<String, f64>,
    pub usage: Usage,
}

impl JevResponse {
    /// 質問 id の答え。未回答なら `None` (欠落を 0.0 に潰さない)。
    pub fn get(&self, id: &str) -> Option<f64> {
        self.answers.get(id).copied()
    }
}

// ---- Cloudflare の二重包装を剥がす wire 型 ----

#[derive(Debug, Deserialize)]
struct CfEnvelope {
    #[serde(default)]
    result: Option<CfResult>,
}

#[derive(Debug, Deserialize)]
struct CfResult {
    #[serde(default)]
    result: Option<JevPayload>,
}

#[derive(Debug, Deserialize)]
struct JevPayload {
    #[serde(default)]
    model: String,
    #[serde(default)]
    answers: BTreeMap<String, RawAnswer>,
    #[serde(default)]
    usage: Option<JevUsage>,
}

#[derive(Debug, Deserialize)]
struct RawAnswer {
    #[serde(rename = "type", default)]
    ty: String,
    #[serde(default)]
    noul: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct JevUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

/// 送信ボディを組む (純関数)。
pub fn encode<'a>(
    model: &'a str,
    state: &'a serde_json::Value,
    questions: &'a BTreeMap<String, NoulQuestion>,
) -> JevRequest<'a> {
    JevRequest {
        model,
        input: JevInput { state, questions },
    }
}

/// 応答本文を解く (純関数)。
///
/// **`json()` 直ではなく text を受けて解く** (#34) — 2xx なのに形が違うとき、本文を捨てると
/// 「missing field」だけが残って真因が消える。ここでは raw を [`LlmError::Parse`] に載せる。
///
/// `type` が `noul` でない答えは載せない (v1 は Noul しか問わないので、呼び出し側は
/// [`JevResponse::get`] が `None` を返すことで気づける)。
pub fn decode(raw: &str) -> Result<JevResponse, LlmError> {
    let env: CfEnvelope = serde_json::from_str(raw).map_err(|source| LlmError::Parse {
        source,
        raw: raw.to_string(),
    })?;
    let payload = env
        .result
        .and_then(|r| r.result)
        .ok_or_else(|| LlmError::Api {
            status: 200,
            body: format!("Jev の応答に result が無い: {raw}"),
        })?;

    let answers = payload
        .answers
        .into_iter()
        .filter_map(|(k, a)| {
            if a.ty == "noul" {
                a.noul.map(|v| (k, v))
            } else {
                None
            }
        })
        .collect();
    let usage = payload
        .usage
        .map(|u| Usage {
            prompt: u.input_tokens,
            completion: u.output_tokens,
            cache_read: 0,
            cost_usd: None,
        })
        .unwrap_or_default();
    Ok(JevResponse {
        model: payload.model,
        answers,
        usage,
    })
}

/// Jev クライアント。
pub struct JevClient {
    http: reqwest::Client,
    config: JevConfig,
}

impl JevClient {
    pub fn new(config: JevConfig) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| LlmError::Config(format!("HTTP クライアントを作れない: {e}")))?;
        Ok(Self { http, config })
    }

    /// env が揃っていれば作る。揃っていなければ `None` (機構ごと存在しない)。
    pub fn from_env() -> Option<Self> {
        JevConfig::from_env().and_then(|c| Self::new(c).ok())
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }

    /// 状態と質問を投げ、答えを受け取る。指数 backoff で一過性エラーのみリトライ。
    ///
    /// 402 (クレジット不足) は `is_transient` が false なのでリトライしない — 残高は
    /// 再送で増えないので、待つだけ無駄で課金の芽にもなる。
    pub async fn ask(
        &self,
        state: &serde_json::Value,
        questions: &BTreeMap<String, NoulQuestion>,
    ) -> Result<JevResponse, LlmError> {
        let mut attempt = 0;
        loop {
            match self.ask_once(state, questions).await {
                Ok(r) => return Ok(r),
                Err(e) => {
                    if attempt >= self.config.max_retries || !e.is_transient() {
                        return Err(e);
                    }
                    let secs = 1u64 << attempt;
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn ask_once(
        &self,
        state: &serde_json::Value,
        questions: &BTreeMap<String, NoulQuestion>,
    ) -> Result<JevResponse, LlmError> {
        let req = encode(&self.config.model, state, questions);
        let resp = self
            .http
            .post(self.config.endpoint())
            .bearer_auth(&self.config.api_token)
            .json(&req)
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
        // text→parse (json() 直は使わない、#34)。
        let body = resp.text().await?;
        decode(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q() -> BTreeMap<String, NoulQuestion> {
        let mut m = BTreeMap::new();
        m.insert(
            "absent_speaker".to_string(),
            NoulQuestion::new("`narration` で `present` に無い人物が台詞を話したか。")
                .with_criteria("present に無い人物が話した", "話したのは present の人物だけ"),
        );
        m.insert(
            "no_criteria".to_string(),
            NoulQuestion::new("criteria を省いた質問。"),
        );
        m
    }

    /// 送信ボディが Jev API の形になる。`criteria` は省略時に**欄ごと出ない**
    /// (null を送ると API 側の解釈が変わりうるので明示的に落とす)。
    #[test]
    fn encode_matches_the_documented_shape() {
        let state = serde_json::json!({ "present": ["akari"], "narration": "…" });
        let questions = q();
        let body = serde_json::to_value(encode(DEFAULT_MODEL, &state, &questions)).unwrap();

        assert_eq!(body["model"], "typesafe/jev");
        assert_eq!(body["input"]["state"]["present"][0], "akari");

        let asked = &body["input"]["questions"]["absent_speaker"];
        assert_eq!(asked["type"], "noul");
        assert!(asked["instructions"].as_str().unwrap().contains("present"));
        assert_eq!(asked["criteria"]["true"], "present に無い人物が話した");
        assert_eq!(asked["criteria"]["false"], "話したのは present の人物だけ");

        let bare = &body["input"]["questions"]["no_criteria"];
        assert_eq!(bare["type"], "noul");
        assert!(
            bare.get("criteria").is_none(),
            "criteria 未指定なら欄ごと出ない: {bare}"
        );
    }

    /// Cloudflare の二重包装を剥がし、usage を canonical へ写す。
    /// 本文は 2026-09-21 に実際に受け取った応答の形。
    #[test]
    fn decode_unwraps_cloudflare_envelope() {
        let raw = r#"{
          "result": {
            "state": "Completed",
            "result": {
              "model": "jev-1.13.0",
              "answers": {
                "x5": { "type": "noul", "noul": 0.93 },
                "x6": { "type": "noul", "noul": 0.1 }
              },
              "usage": { "input_tokens": 931, "output_tokens": 128 }
            },
            "gatewayMetadata": { "keySource": "Unified" }
          },
          "success": true, "errors": [], "messages": []
        }"#;
        let got = decode(raw).expect("decode");
        assert_eq!(got.model, "jev-1.13.0");
        assert_eq!(got.get("x5"), Some(0.93));
        assert_eq!(got.get("x6"), Some(0.1));
        assert_eq!(got.get("missing"), None, "未回答は None (0.0 に潰さない)");
        assert_eq!(got.usage.prompt, 931);
        assert_eq!(got.usage.completion, 128);
        assert_eq!(got.usage.cost_usd, None, "Cloudflare は費用を申告しない");
    }

    /// usage が無い応答でも落ちない (計器の欠落で検査を失わせない)。
    #[test]
    fn decode_tolerates_missing_usage() {
        let raw = r#"{"result":{"result":{"model":"jev-1.13.0","answers":{"a":{"type":"noul","noul":0.5}}}},"success":true}"#;
        let got = decode(raw).expect("decode");
        assert_eq!(got.get("a"), Some(0.5));
        assert_eq!(got.usage.prompt, 0);
    }

    /// 非 JSON の本文は raw ごと surface する (#34 — 捨てると真因が消える)。
    #[test]
    fn decode_keeps_raw_on_broken_body() {
        let err = decode("<html>502 Bad Gateway</html>").unwrap_err();
        match err {
            LlmError::Parse { raw, .. } => assert!(raw.contains("502 Bad Gateway")),
            other => panic!("Parse を期待したが {other:?}"),
        }
    }

    /// 200 なのに result が無い形は Api エラーとして本文ごと出す。
    #[test]
    fn decode_reports_missing_result() {
        let err = decode(r#"{"success":true,"errors":[],"result":null}"#).unwrap_err();
        match err {
            LlmError::Api { status, body } => {
                assert_eq!(status, 200);
                assert!(body.contains("result が無い"));
            }
            other => panic!("Api を期待したが {other:?}"),
        }
    }

    /// クレジット不足 (402) は**一過性でない** — 残高は再送で増えないのでリトライしない。
    /// 2026-09-21 に実際に受け取った本文。
    #[test]
    fn insufficient_balance_is_not_transient() {
        let err = LlmError::Api {
            status: 402,
            body: r#"{"errors":[{"message":"Insufficient balance; add money to your gateway or use BYOK","code":2021}],"success":false}"#.to_string(),
        };
        assert!(!err.is_transient(), "402 をリトライしてはいけない");

        // 対照: 429 と 5xx は一過性なのでリトライする。
        assert!(LlmError::Api { status: 429, body: String::new() }.is_transient());
        assert!(LlmError::Api { status: 503, body: String::new() }.is_transient());
    }

    /// env が片方だけなら `None` — 設定漏れを黙って既定へ落とさない。
    /// (env はプロセス共有なのでキー名だけを検査し、値は触らない)
    #[test]
    fn endpoint_is_built_from_account_id() {
        let cfg = JevConfig {
            account_id: "abc123".into(),
            api_token: "t".into(),
            base_url: format!("{DEFAULT_BASE_URL}/"),
            model: DEFAULT_MODEL.into(),
            timeout: Duration::from_secs(20),
            max_retries: 2,
        };
        assert_eq!(
            cfg.endpoint(),
            "https://api.cloudflare.com/client/v4/accounts/abc123/ai/run",
            "base_url の末尾スラッシュは重複させない"
        );
    }
}
