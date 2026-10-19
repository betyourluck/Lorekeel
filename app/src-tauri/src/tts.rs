//! 読み上げの OpenAI 互換経路 (spec 36)。`/v1/audio/speech` を喋る任意のサーバーに **backend から**
//! 合成を頼む。
//!
//! 2026-10-19 に初めて実サーバー (Irodori-TTS-Server) に当てたら、口の形は合っているのに WebView
//! からは届かなかった (failures #114): サーバーに CORS が無く preflight が 405、届いてもライブラリの
//! fetch は 30 秒固定で CPU の合成 (45 秒/文) は毎回切れる。どちらも Kataribe の口では越えられない
//! (CORS はサーバー側・タイムアウトはライブラリ側) ので、画像生成 (spec 24) と同じく HTTP を backend
//! へ移す — backend の reqwest に CORS は存在せず、タイムアウトも自前で持てる。
//!
//! 層分け (image_gen と同じ): `encode` / `decode` は純関数、HTTP は [`synthesize`] 1 本。
//! 鍵は持たない (v1、spec 36 D3)。Irodori 固有の `irodori: {...}` 欄も送らない (汎用の口)。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

/// 既定の待ち時間 (秒)。温まった後の 1 文 (実測 45 秒) 向け。**初回のモデル読み込み (実測 439 秒)
/// は救えない** — 試聴がウォームアップを兼ねる (frontend が max(設定, 600) で待つ、spec 36 D6)。
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;
/// 待ち時間の下限。これより短い指定は意味を持たない (合成は秒単位)。
pub const MIN_TIMEOUT_SECS: u64 = 5;

/// frontend から渡る設定 (契約 `TtsBackendRoute.config`)。鍵は無い。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsRouteConfig {
    /// エンドポイント全体 (`…/v1/audio/speech`)。base URL ではない (ライブラリの口と同じ)。
    pub endpoint: String,
    /// 必須。空は [`TtsRouteError::Shape`] (ライブラリも空なら例外)。
    pub model: String,
    /// 必須・**常に送る**。空 → `none` の正規化は frontend (`toRouteConfig`) の責務。
    /// `Option` にしない — 「送らない」と「`none` を送る」を 1 つの None に重ねると型が嘘になる。
    pub voice: String,
    /// 速度。OpenAI の範囲 0.25〜4.0 に clamp する。
    pub speed: f32,
    /// 待ち時間 (秒)。`None` と `0` は既定 [`DEFAULT_TIMEOUT_SECS`] (frontend は UI の 0 を null に
    /// 写すが、写し忘れても 0 を 5 秒に潰さない)。
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

impl TtsRouteConfig {
    /// 実効の待ち時間 (契約 `TtsBackendRoute.timeout`)。
    pub fn timeout(&self) -> Duration {
        let secs = self.timeout_secs.filter(|s| *s > 0).unwrap_or(DEFAULT_TIMEOUT_SECS);
        Duration::from_secs(secs.max(MIN_TIMEOUT_SECS))
    }

    pub fn clamped_speed(&self) -> f32 {
        if self.speed.is_finite() {
            self.speed.clamp(0.25, 4.0)
        } else {
            1.0
        }
    }
}

/// 合成の結果 (契約 `TtsBackendRoute.view`)。音声は base64 文字列で運ぶ (画像と同じ運び方)。
/// frontend は **data URL にしない** (CSP の `media-src` に `data:` が無い) — decode → Blob → Object URL。
#[derive(Debug, Clone, Serialize)]
pub struct SpeechView {
    pub mime: String,
    pub data_b64: String,
}

/// 失敗の 4 種 (契約 `TtsBackendRoute.errors`)。frontend はこれを区別して文面に出す —
/// ライブラリ経由では 404 も CORS も timeout も「失敗」の一語に潰れていた。
/// **CORS の種別は無い**: backend の reqwest はブラウザではないので CORS が存在しない。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TtsRouteError {
    /// 4xx / 5xx。`body_head` は本文の先頭 200 字 (Irodori は `{"error":{"message":…}}` で理由を言う)。
    Http { status: u16, body_head: String },
    /// 待ち時間切れ。現在値を載せる (次の一手が「待ち時間をいくつにするか」なので)。
    Timeout { secs: u64 },
    /// 接続拒否・DNS・途中切断 (reqwest の status を持たないエラー全部)。
    Network { detail: String },
    /// 応答が `audio/*` でない・本文が空・設定不備 (model 空・endpoint 空)。
    Shape { detail: String },
}

impl std::fmt::Display for TtsRouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TtsRouteError::Http { status, body_head } => write!(f, "HTTP {status}: {body_head}"),
            TtsRouteError::Timeout { secs } => write!(f, "{secs} 秒で応答がありませんでした"),
            TtsRouteError::Network { detail } => write!(f, "接続できません: {detail}"),
            TtsRouteError::Shape { detail } => write!(f, "応答の形が合いません: {detail}"),
        }
    }
}

impl std::error::Error for TtsRouteError {}

/// 送る本文 (純関数)。OpenAI の `/v1/audio/speech` の欄だけ: `model` / `input` / `voice` / `speed`。
/// `voice` は**常に**送る (省くと Irodori は `IRODORI_DEFAULT_VOICE` 未設定で 400 = #114)。
/// `response_format` は送らない (サーバー既定 = Irodori は wav。mime は応答の content-type から取る)。
pub fn encode(cfg: &TtsRouteConfig, text: &str) -> Value {
    json!({
        "model": cfg.model,
        "input": text,
        "voice": cfg.voice,
        "speed": cfg.clamped_speed(),
    })
}

/// 2xx 応答を音声に (純関数)。`content_type` が `audio/` で始まらなければ Shape (JSON のエラー本文や
/// HTML を音として再生しない)。`audio/wav; codecs=1` のようなパラメータは落とす。
pub fn decode(content_type: Option<&str>, bytes: Vec<u8>) -> Result<(String, Vec<u8>), TtsRouteError> {
    let mime = content_type
        .map(|c| c.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
        .unwrap_or_default();
    if !mime.starts_with("audio/") {
        let head: String = String::from_utf8_lossy(&bytes).chars().take(200).collect();
        return Err(TtsRouteError::Shape {
            detail: format!("content-type が audio/* ではありません ({}): {head}", if mime.is_empty() { "なし" } else { &mime }),
        });
    }
    if bytes.is_empty() {
        return Err(TtsRouteError::Shape { detail: "本文が空です".into() });
    }
    Ok((mime, bytes))
}

/// 設定の検め (純関数)。HTTP を出す前に弾く。
pub fn validate(cfg: &TtsRouteConfig) -> Result<(), TtsRouteError> {
    if cfg.endpoint.trim().is_empty() {
        return Err(TtsRouteError::Shape { detail: "エンドポイント URL が空です".into() });
    }
    if cfg.model.trim().is_empty() {
        return Err(TtsRouteError::Shape { detail: "モデル名が空です".into() });
    }
    if cfg.voice.trim().is_empty() {
        // frontend が `none` に正規化する契約 — ここに来たら frontend の穴。黙って送らない。
        return Err(TtsRouteError::Shape { detail: "話者 ID が空です (frontend が none に正規化する契約)".into() });
    }
    Ok(())
}

fn head_200(body: &str) -> String {
    body.chars().take(200).collect()
}

/// 1 文を合成する (HTTP ドライバ)。待ち時間は [`TtsRouteConfig::timeout`]。
pub async fn synthesize(cfg: &TtsRouteConfig, text: &str) -> Result<SpeechView, TtsRouteError> {
    validate(cfg)?;
    if text.trim().is_empty() {
        return Err(TtsRouteError::Shape { detail: "本文が空です".into() });
    }
    let http = reqwest::Client::builder()
        .timeout(cfg.timeout())
        .build()
        .map_err(|e| TtsRouteError::Network { detail: e.to_string() })?;
    let resp = http
        .post(cfg.endpoint.trim())
        .json(&encode(cfg, text))
        .send()
        .await
        .map_err(|e| map_reqwest(e, cfg))?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = resp.bytes().await.map_err(|e| map_reqwest(e, cfg))?.to_vec();
    if !(200..300).contains(&status) {
        return Err(TtsRouteError::Http { status, body_head: head_200(&String::from_utf8_lossy(&bytes)) });
    }
    let (mime, bytes) = decode(content_type.as_deref(), bytes)?;
    Ok(SpeechView { mime, data_b64: crate::image_gen::base64_encode(&bytes) })
}

fn map_reqwest(e: reqwest::Error, cfg: &TtsRouteConfig) -> TtsRouteError {
    if e.is_timeout() {
        return TtsRouteError::Timeout { secs: cfg.timeout().as_secs() };
    }
    // 真因は source 連鎖の底に居る (「error sending request」の下の connect refused 等)。
    let mut detail = e.to_string();
    let mut cause: Option<&dyn std::error::Error> = std::error::Error::source(&e);
    while let Some(inner) = cause {
        detail = inner.to_string();
        cause = inner.source();
    }
    TtsRouteError::Network { detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(endpoint: &str) -> TtsRouteConfig {
        TtsRouteConfig {
            endpoint: endpoint.into(),
            model: "irodori-tts".into(),
            voice: "none".into(),
            speed: 1.0,
            timeout_secs: None,
        }
    }

    /// 【spec 36 D2/D5/D6】送る本文は OpenAI の 4 欄だけ・`voice` は常に送る・speed は clamp・
    /// 待ち時間は None と 0 が既定 120、下限 5。
    #[test]
    fn encode_sends_voice_always_and_clamps_speed_and_timeout_rules() {
        let mut c = cfg("http://x/v1/audio/speech");
        let body = encode(&c, "こんにちは");
        assert_eq!(body, json!({"model": "irodori-tts", "input": "こんにちは", "voice": "none", "speed": 1.0}));
        assert!(body.get("response_format").is_none() && body.get("irodori").is_none(), "vendor 欄は送らない");
        c.speed = 9.0;
        assert_eq!(encode(&c, "a")["speed"], 4.0);
        c.speed = 0.0;
        assert_eq!(encode(&c, "a")["speed"], 0.25);
        c.speed = f32::NAN;
        assert_eq!(encode(&c, "a")["speed"], 1.0);
        assert_eq!(c.timeout(), Duration::from_secs(120), "None は既定");
        c.timeout_secs = Some(0);
        assert_eq!(c.timeout(), Duration::from_secs(120), "0 も既定 (frontend の写し忘れを 5 秒に潰さない)");
        c.timeout_secs = Some(2);
        assert_eq!(c.timeout(), Duration::from_secs(5), "下限");
        c.timeout_secs = Some(600);
        assert_eq!(c.timeout(), Duration::from_secs(600));
        // 空の voice は frontend の穴 — 送らずに Shape。
        c.voice = "".into();
        assert!(matches!(validate(&c), Err(TtsRouteError::Shape { .. })));
        c.voice = "none".into();
        c.model = " ".into();
        assert!(matches!(validate(&c), Err(TtsRouteError::Shape { .. })));
    }

    /// 【spec 36 D2】decode は `audio/*` だけを通す。パラメータ付きの content-type は mime だけに。
    #[test]
    fn decode_accepts_only_audio_content_types() {
        let (mime, b) = decode(Some("audio/wav; codecs=1"), vec![1, 2, 3]).unwrap();
        assert_eq!((mime.as_str(), b.len()), ("audio/wav", 3));
        assert!(matches!(decode(Some("application/json"), b"{\"detail\":\"x\"}".to_vec()), Err(TtsRouteError::Shape { .. })));
        assert!(matches!(decode(None, vec![1]), Err(TtsRouteError::Shape { .. })));
        assert!(matches!(decode(Some("audio/wav"), vec![]), Err(TtsRouteError::Shape { .. })));
        // 失敗の種別は wire で区別できる (frontend が文面を分ける)。
        let v = serde_json::to_value(TtsRouteError::Timeout { secs: 120 }).unwrap();
        assert_eq!(v, json!({"kind": "timeout", "secs": 120}));
    }

    /// 台本どおりに 1 接続 1 応答を返すローカル HTTP サーバ (llm_client の serve_scripted と同じ形)。
    /// `(status, content_type, body)`。body を None にすると応答せずに接続を保つ (タイムアウト用)。
    fn serve(responses: Vec<(u16, &'static str, Option<Vec<u8>>)>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for (status, ctype, body) in responses {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let head_end = loop {
                    let n = s.read(&mut chunk).unwrap();
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..head_end]).to_ascii_lowercase();
                let len: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .map(|v| v.trim().parse().unwrap())
                    .unwrap_or(0);
                while buf.len() < head_end + len {
                    let n = s.read(&mut chunk).unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                }
                let Some(body) = body else {
                    // 応答しない: クライアントのタイムアウトまで握る。
                    std::thread::sleep(Duration::from_secs(8));
                    return;
                };
                let resp = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                s.write_all(resp.as_bytes()).unwrap();
                s.write_all(&body).unwrap();
            }
        });
        format!("http://{addr}/v1/audio/speech")
    }

    /// 【spec 36 Phase A】command の入口 (`synthesize`) を実 HTTP で通す — 純関数だけでは配線の穴が
    /// 見えない (#109)。200 audio → base64 / 400 → Http に本文 / 200 非 audio → Shape。
    #[tokio::test]
    async fn synthesize_maps_responses_to_view_and_error_kinds() {
        let wav = b"RIFF....WAVEfmt ".to_vec();
        let ep = serve(vec![
            (200, "audio/wav", Some(wav.clone())),
            (400, "application/json", Some(br#"{"error":{"message":"No voice was provided"}}"#.to_vec())),
            (200, "application/json", Some(b"{\"detail\":\"Not Found\"}".to_vec())),
        ]);
        let c = cfg(&ep);
        let v = synthesize(&c, "こんにちは").await.unwrap();
        assert_eq!(v.mime, "audio/wav");
        assert_eq!(crate::image_gen::base64_decode(&v.data_b64).unwrap(), wav);
        match synthesize(&c, "x").await.unwrap_err() {
            TtsRouteError::Http { status: 400, body_head } => assert!(body_head.contains("No voice"), "{body_head}"),
            other => panic!("Http を期待: {other:?}"),
        }
        assert!(matches!(synthesize(&c, "x").await.unwrap_err(), TtsRouteError::Shape { .. }));
        // 閉じたポート → Network (connect refused が底に出る)。
        let mut dead = cfg("http://127.0.0.1:9/v1/audio/speech");
        dead.timeout_secs = Some(5);
        assert!(matches!(synthesize(&dead, "x").await.unwrap_err(), TtsRouteError::Network { .. }));
    }

    /// 【spec 36 D6】応答しないサーバーは Timeout になり、現在の待ち時間 (下限 5 秒) を文面に載せる。
    #[tokio::test]
    async fn synthesize_reports_timeout_with_current_limit() {
        let ep = serve(vec![(200, "audio/wav", None)]);
        let mut c = cfg(&ep);
        c.timeout_secs = Some(1); // 下限 5 秒に切り上がる
        let err = synthesize(&c, "x").await.unwrap_err();
        assert_eq!(err, TtsRouteError::Timeout { secs: 5 });
        assert!(err.to_string().contains('5'));
    }

    /// 【live】実 Irodori-TTS-Server (8088) に 1 文を頼む。鍵不要・サーバーが居るときだけ
    /// (`cargo test --lib tts::tests::live -- --ignored --nocapture`)。初回はモデル読み込みで
    /// 数分かかるので 600 秒で待つ。
    #[tokio::test]
    #[ignore]
    async fn live_irodori_round_trip() {
        let mut c = cfg("http://127.0.0.1:8088/v1/audio/speech");
        c.timeout_secs = Some(600);
        let t0 = std::time::Instant::now();
        let v = synthesize(&c, "こんにちは。語り部の読み上げテストです。").await.unwrap();
        let bytes = crate::image_gen::base64_decode(&v.data_b64).unwrap();
        eprintln!("[live] mime={} bytes={} elapsed={:.1}s", v.mime, bytes.len(), t0.elapsed().as_secs_f64());
        assert_eq!(v.mime, "audio/wav");
        assert!(bytes.starts_with(b"RIFF"), "WAV のはず");
        // voice を空にすると backend は送らず Shape (frontend の正規化の契約)。
        c.voice = String::new();
        assert!(matches!(synthesize(&c, "x").await.unwrap_err(), TtsRouteError::Shape { .. }));
    }
}
