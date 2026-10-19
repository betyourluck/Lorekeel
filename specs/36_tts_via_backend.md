# spec 36 — 読み上げの OpenAI 互換経路を backend 経由に

**Status**: **Done** (2026-10-19 起草 → 同日ユーザー査読〔矛盾 5 + 要修正 4〕を反映 = rev2 → 同日 Phase 0〜C 実装 → backend の live Green → Tauri 実機 Green〔`none` の揺れは参照音声で解消〕)。残 = D4 の次ターンの出だしの待ちの実測だけ (未決 2・4 は実害が出るまで据え置き)。契約は data_contract `TtsBackendRoute`。実装の記録は末尾。

---

## 動機

2026-07-22 に足した読み上げの OpenAI 互換エンジン (`/v1/audio/speech` を喋る任意のサーバーに繋ぐ口) を、
2026-10-19 に初めて実サーバー (Irodori-TTS-Server、8088) に当てた。結果は failures #114:

1. **口の形は合っている** — アプリと同じ本文 (`model` / `input` / `speed` + `voice: none`) に curl で 200・WAV 3.6 秒。
2. **WebView からは届かない** — サーバーに CORS ミドルウェアが無く、preflight (OPTIONS) が 405・
   `Access-Control-Allow-Origin` なしで fetch が `net::ERR_FAILED`。README にも CORS の設定は無い。
3. **届いても切れる** — ライブラリ (`@aituber-onair/voice`) の `fetchWithTimeout` は 30 秒固定で、
   engine は 3 つ目の引数を渡していない。この機 (GPU なし・fp32) の合成は温まっても 45 秒/文、初回は
   モデル読み込み込みで 439 秒。

2 と 3 はどちらも Kataribe の口 (`tts.ts` の `engineOptions`) では越えられない。CORS はサーバー側、
タイムアウトはライブラリ側に在る。画像生成 (spec 24) が「HTTP は全部 backend」にした理由 (CSP・鍵) が
ここにもそのまま当たる — backend には CORS も CSP も無く、タイムアウトも自前で持てる。

### 位置づけ

- 2026-07-22 の「Irodori 個別対応でなく汎用の口」は不変。経路を backend に移すだけで、口の形
  (OpenAI の `/v1/audio/speech`) は変えない。Irodori 固有の `irodori: {...}` 欄は送らない。
- 他 3 エンジン (ブラウザ内蔵 / VOICEVOX / AivisSpeech) は**触らない**。動いているものを動かさない。
  VOICEVOX / AivisSpeech は Origin を反映する CORS を返すので、「汎用の口が動く前提をたまたま満たす
  サーバーで検証していた」のが #114 の機序。

### スコープ外

- Irodori-TTS 本体の Gradio API (`/gradio_api/call/_run_generation`) への対応 — 7860 はデモ画面で
  サーバーではない。参照音声の upload まで要るので別物。
- `stream_format: sse` (チャンク単位の SSE) — v2。まず 1 文 1 リクエストで成立させる。
- 旧 3 エンジンの backend 移設 — 動いているので動機が無い。
- 鍵欄 (= クラウドの `/v1/audio/speech` を開く) — **v1 の型から外す** (D3)。
- 作者がキャラに声を書く欄 (spec 35 のスコープ外のまま)。

---

## 接地 (2026-10-19 コード実読)

| 事実 | 場所 |
|---|---|
| 合成の HTTP はライブラリの `OpenAiCompatibleEngine.fetchAudio` が WebView から直接 fetch。body = `{model, input, speed}` + `voice` (空なら送らない)。`fetchWithTimeout(url, init)` の 3 つ目 (timeoutMs) を渡さず**既定 30000** | `node_modules/@aituber-onair/voice/dist/esm/engines/OpenAiCompatibleEngine.js` / `engines/internal/utils.js` |
| ライブラリに**注入点が無い** — `VoiceEngineFactory.getEngine` は固定の型、`VoiceEngineAdapter` の `audioPlayer` は内部の `AudioPlayerFactory` で作られ options から差せない。`AudioPlayer` は型だけ export | `services/VoiceEngineAdapter.js` ctor / `index.d.ts` |
| frontend の読み上げは**自前の直列鎖** `chain` + 世代 `generation` + `skipThrough` で、文ごとに `ensureAdapter()` → `a.speakText(sentence)`。声の切替は `updateOptions({speaker})` | `app/src/tts.ts` `speakParts` |
| 中断は `interrupt()` = `adapter.stop()` + **再生中だったらアダプタを捨てる** (#72 の罠)。`stop()` は世代を進め、`skip()` は進めない | `app/src/tts.ts` 545〜 |
| 試聴は `test()` が `speakText` を直に await し、**失敗を投げる** (設定の接地) | `app/src/tts.ts` 531〜 / `SettingsDialog.vue` `testVoice` |
| 設定は `TtsSettings {engine, serverUrl, speaker, rate, pitch, volume, model, scope}` (localStorage `kataribe.tts.*`)。OpenAI 互換は URL が**エンドポイント全体** | `app/src/tts.ts` 19〜 |
| CSP: `connect-src` は localhost のみ・`media-src` は `'self' asset: blob:` (**`data:` は無い** → 再生は Blob URL 一択) | `app/src-tauri/tauri.conf.json` |
| backend の先例: 画像生成は `ImageGenConfig` を command に渡し、HTTP は reqwest・応答は base64 文字列で運ぶ (`tauri::ipc::Response` の生バイト返しは repo に先例なし)。タイムアウトはプロバイダ別既定 + `timeout_secs: Option<u64>` 上書き | `app/src-tauri/src/image_gen.rs` / `lib.rs` `generate_image` |
| 秘密の env は `SECRET_ENV_KEYS` の一覧だけが資格情報ストアへ行く (`IMAGE_API_KEY_*` が先例) | `app/src-tauri/src/secret_store.rs` |
| あらすじの待ち時間 select: UI は `0 = 既定` の数値、command `set_summary_timeout` は 0 を**空の env** に写し、harness 側が未設定を既定へ倒す | `SettingsDialog.vue` / `lib.rs` |
| Irodori-TTS-Server: `voice` 省略は `IRODORI_DEFAULT_VOICE` 未設定なら 400。`none` = Voice Design (参照なし)。**参照なしだと 80 字チャンクごとに声の調子が変わりうる** (README 明記)。`/health` はモデルを読まない。`IRODORI_PRELOAD` 既定 false = 初回リクエストでモデルを読む (実測 439 秒)。任意の bearer `IRODORI_API_KEY` | README (2026-10-19) / `/health` |

---

## 設計

### D1. 経路の分岐は engine 1 つだけ

`engine === "openaiCompatible"` のときだけ合成を backend command へ回す。他 3 エンジンはライブラリのまま。
`tts.ts` に合成の継ぎ目 `Synth` を切り、実装を 2 つ持つ:

```ts
interface Synth {
  /** 1 文を合成して再生し終えるまで待つ。中断されたら reject。 */
  speak(sentence: string, voice: string): Promise<void>;
  /** 再生中の音を止める。進行中の speak は reject させる。 */
  stop(): void;
  isPlaying(): boolean;
  /** interrupt() が「このインスタンスは捨てて作り直せ」と判断する材料 (#72)。 */
  discardOnStop: boolean;
}
```

- `LibrarySynth` = 今の `VoiceEngineAdapter` の包み。`speak` は `updateOptions({speaker})` (声が変わるときだけ) → `speakText`。
  `discardOnStop = true` (#72: 再生中に止めたアダプタは二度と喋らないので捨てる — 現行の `interrupt()` と同じ)。
- `BackendSynth` = `invoke("synthesize_speech")` → base64 → `Uint8Array` → `Blob` → `URL.createObjectURL` →
  `HTMLAudioElement.play()`。`ended` / `error` で resolve / reject、**`stop()` は `pause()` して自分で reject する**
  (#72 の罠 = ライブラリの stop が play の Promise を settle しない、を自分では踏まない形)。Object URL は
  `ended` / `error` / `stop()` のいずれでも revoke する (1 文 1 URL、取りこぼし無し)。`discardOnStop = false` —
  インスタンスが持つ状態は再生中の `Audio` 1 つだけで、`stop()` がそれを畳むので作り直す理由が無い。

鎖 (`chain`) / 世代 (`generation`) / `skipThrough` は `Synth` の外にあり**不変**。`interrupt()` は
`synth.stop()` → `discardOnStop && wasPlaying` のときだけ捨てる、と一般化する (Library では現行と同じ、
Backend では捨てない)。**挙動の分岐は `discardOnStop` の 1 bit に閉じ込める**。

### D2. backend = `tts.rs` (encode / decode 純関数 + HTTP ドライバ)

command `synthesize_speech(config: TtsRouteConfig, text: String) -> Result<SpeechView, TtsRouteError>`。

```rust
pub struct TtsRouteConfig {
    pub endpoint: String,          // エンドポイント全体 (…/v1/audio/speech)
    pub model: String,             // 必須 (空は Shape で弾く = ライブラリと同じ)
    pub voice: String,             // 必須。空は frontend で `none` に正規化済み (D5)。encode は常に送る
    pub speed: f32,                // 0.25〜4.0 に clamp (OpenAI の範囲)
    pub timeout_secs: Option<u64>, // None = 既定 120。frontend が UI の 0 を None に写す (D6)
}
pub struct SpeechView { pub mime: String, pub data_b64: String }
pub enum TtsRouteError {
    Http { status: u16, body_head: String },   // 4xx/5xx。本文先頭 200 字
    Timeout { secs: u64 },                     // 待ち時間切れ。現在値を載せる (OutputTruncated と同じ理由)
    Network { detail: String },                // 接続拒否・DNS・途中切断 (reqwest の非 status エラー全部)
    Shape { detail: String },                  // 応答が audio/* でない・本文空・設定不備 (model 空)
}
```

- **鍵は持たない** (v1)。`api_key` 欄も bearer も無い。将来のクラウド対応は data_contract に名前 `TTS_API_KEY_OPENAI_COMPAT` を
  **予約として書くだけ** (型には足さない = 足すときは additive)。
- 返りは base64 文字列 (画像と同じ運び方・1 文 WAV ≈ 350 KB → 470 KB)。**frontend は data URL にしない** (CSP の
  `media-src` に `data:` が無い) — `atob → Uint8Array → Blob → createObjectURL` に統一する (D1)。コピー 1 回は許容。
  生バイト (`tauri::ipc::Response`) は未決 4 (v2)。
- mime は応答の `content-type` から取り、`audio/` で始まらなければ `Shape`。
- **CORS の種別は無い** — backend の reqwest はブラウザではないので CORS が存在しない。#114 の `net::ERR_FAILED` は
  この経路では**起きない**のが処方そのもので、残る失敗は上の 4 種で尽きる。
- 画像生成 (`image_gen.rs`) と同じ置き場・同じ流儀 (encode / decode は純関数・HTTP は 1 関数)。

### D3. 鍵は v1 の型から外す (査読 1 の反映)

起草時は `api_key: Option` を持ちつつ「v1 では持たない」と書いていた = 型が「鍵あり」の契約を名乗っていた。
rev2 は**型から消す**。`SECRET_ENV_KEYS` にも足さない。クラウドの `/v1/audio/speech` を開くかは 2026-07-22 に
CSP で閉じた判断の再訪なので別 spec (37 以降) で、そのときに `api_key: Option` を additive に足す。

### D4. 中断の契約 (査読 5 の反映)

- `stop()` / `skip()` → `interrupt()` → `synth.stop()`。`BackendSynth` は再生中の `Audio` を `pause()` し URL を revoke し、
  進行中の `speak()` を reject する (鎖の await は必ず settle する = 現行の不変条件を保つ)。
- **進行中の `invoke` は止めない** (Tauri の command に cancel は無い)。`speak()` は `invoke` が解決した後・再生の前に
  `generation` を見て、古ければ結果を捨てる (再生しない・URL も作らない)。
- **サーバー側の直列待ちは世代をまたぐときだけ起きる**: 同じ鎖 (同じ世代) の中は元から 1 文ずつ直列なので余計な待ちは
  無い。`stop()` で捨てた合成がサーバーで走っている間に次のターンの最初の文を投げると、Irodori は
  `MAX_CONCURRENT_SYNTHESIS=1` なので**捨てた 1 文ぶん** (この機で最大 45 秒) 待たされる。待ちは次のターンの
  読み上げの**出だし**に出る (鎖の途中に無音が入るのではない)。v1 はこれを許容し、読み上げの操作列に
  「合成中」の表示を出して無音でないことを示す。drop (`tokio::select!` + 世代 token) は未決 2 (v2)。

### D5. `voice` は必ず送る・空は frontend で `none` に (査読 2 の反映)

- 型は `voice: String` (必須)。`Option` は持たない — 「送らない」と「`none` を送る」を 1 つの None に重ねると
  型が嘘になる (査読 2)。
- 空 → `none` の正規化は **`tts.ts` の責務** (`toRouteConfig(settings)`)。`none` は Irodori の Voice Design の
  語彙で、他の OpenAI 互換サーバー (OpenAI 本家の `alloy` 等・openedai-speech) では 400 になりうる — それは
  **D2 の `Http { status, body_head }` が本文ごと画面に出す**ので、沈黙にはならない。UI の話者 ID の説明に
  「空なら `none` を送る (Irodori の参照なし)。他のサーバーはそのサーバーの voice 名を入れる」を書く。
- **80 字チャンクと声の揺れ (査読「その他」3 の反映)**: Irodori は `voice` が参照つき (voices/ のファイル) なら
  チャンク間で声を保つが、**`none` (参照なし) ではチャンクごとに調子が変わりうる**と README が明記している。
  起草時の「`voice` を送るので揺れない」は `none` には当てはまらない — **訂正**。Kataribe 側で 80 字に再分割しても
  参照が無い限り揺れは消えないので、v1 は再分割しない。安定した声が要るなら voices/ に参照を置いてその名前を
  話者 ID に入れる (作法として UI の説明と package_spec には書かない = プレイヤー側の設定)。Phase D で `none` の
  揺れを 1 ターンぶん耳で確かめ、耐えないなら Irodori 固有欄 `irodori.chunking_enabled=false` の opt-in を v2 で検討
  (汎用の口に vendor 欄を混ぜる判断になるので v1 では入れない)。

### D6. タイムアウト (査読 4 と「その他」4 の反映)

- 既定 **120 秒**は**温まった後の 1 文** (45 秒) 向け。**初回 439 秒は 120 秒では必ず切れる**ので、起草時の
  select だけでは「初回は必ず失敗し、理由が分からない」になっていた。処方 2 つ:
  1. **試聴はウォームアップを兼ねる**: `test()` は設定値と **600 秒の大きい方**で待ち、ボタンの説明に
     「初回はモデルの読み込みで数分かかることがある。サーバー側で `IRODORI_PRELOAD=true` にすれば起動時に読む」を書く。
     (`/health` はモデルを読まないので、ウォームアップは実合成でしか起きない。)
  2. **本番の読み上げの Timeout は文面で理由を言う**: 失敗の表示 (読み上げの操作列のトースト) に
     「N 秒で待ち切れなかった。初回ならサーバーがモデルを読み込み中かもしれない — 設定の試聴で温めるか待ち時間を伸ばす」。
     読み上げの失敗は従来どおりプレイを止めない (その文が飛ぶ) が、**沈黙にはしない**。
- `timeout_secs` の変換規則: UI の select は `0 = 既定` の数値 (localStorage `kataribe.tts.timeoutSecs`、既存の
  `TtsSettings` に欄を足す)。`toRouteConfig` が **`0` を `null` (= Rust の `None`)** に写す。backend は
  `timeout_secs.unwrap_or(120).max(5)`。あらすじの `set_summary_timeout` は「0 を空の env に写す」形で**同じ規則の
  別の表現** (あちらは env が器、こちらは Option)。

### D7. Phase 0 で凍結する中身 (「その他」2 の反映)

Phase 0 は**決めない**。rev2 の D2 (型 3 つと失敗 4 種・base64・mime の取り方) / D5 (`voice` 必須・空 → `none` は frontend) /
D6 (既定 120・`0 → None`・試聴は max(設定, 600)) を data_contract `TtsBackendRoute` に**写す**だけにする。
Phase A は凍結済みの形を実装する。

### D8. 計器 (spec 30) には載せない

ローカル TTS は課金が無く、Irodori の応答にも usage は無い。画像の `ImageLedger` に相当するものは v1 では持たない。

---

## Phase

- **✅0**: data_contract に `TtsBackendRoute` (D7 の中身) + 予約名 `TTS_API_KEY_OPENAI_COMPAT` (未使用) を凍結。
- **✅A (backend)**: `tts.rs` の encode (body = `{model, input, voice, speed}`・`voice` は常に送る・clamp) / decode
  (content-type → mime・`audio/*` でなければ Shape) を純関数で + command + 登録。
  PoC: 純関数 2〜3 本 + **ローカル HTTP サーバから WAV を流して command の入口を通す 1 本** (#109 の教訓 =
  純関数だけでは配線の穴が見えない。400 / 非 audio / タイムアウトの 3 失敗が 4 種のどれに写るかもここで) +
  live 1 本 (ignored、実 Irodori)。
- **✅B (frontend seam)**: `Synth` 2 実装、`speakParts` / `test` / `interrupt` を seam 越しに。`LibrarySynth` は
  ライブラリ adapter への呼び出し列 (speakText / stop / updateOptions の順) を偽装ライブラリで固定している既存テスト
  (spec 35 Phase D) が回帰を見る。`BackendSynth` は `invoke` を偽装して「1 文 1 invoke・`voice` は空なら `none`・
  stop で reject・古い世代の結果は再生しない・URL は必ず revoke」を mount で固定。
- **✅C (設定と表示)**: サウンドタブに待ち時間の select (60 / 120 / 180 / 300 / 600、0 = 既定) + 試聴は max(設定, 600) +
  失敗文言 (Http / Timeout / Network / Shape を区別、Timeout は初回の読み込みの可能性を言う) + 話者 ID の説明 +
  読み上げの操作列に「合成中」。
- **✅D (実測)**: backend の live と Tauri 実機 (鳴る・`none` で揺れる・参照音声で安定) が Green (下の記録)。未測 = stop 直後の次ターンの出だしの待ち。当初の計画 = Irodori-TTS-Server (CPU) で 1 ターンぶん読み切る / 試聴の初回 (モデル読み込み) が 600 秒の中で通る /
  失敗 4 種が区別して出る / `stop()` 直後の次ターンの出だしの待ち (D4) / `none` のチャンク間の声の揺れ (D5)。
  GPU 機は未測定のまま申告。

---

## 未決

1. ~~鍵欄~~ → **v1 の型から外すと決めた** (D3)。クラウド対応は別 spec。
2. backend の進行中リクエストの drop (D4)。v1 は許容 + 「合成中」表示。
3. ~~文単位 / 段単位~~ → 文単位 (D5)。`none` の揺れは Irodori の性質で、Kataribe の分割では消えない。
   `irodori.chunking_enabled=false` の opt-in は v2 の検討事項。
4. base64 か生バイト (`tauri::ipc::Response`) か。v1 は base64 (先例に揃える・1 文 470 KB で測って重ければ v2)。

## 査読の反映記録 (2026-10-19 rev2)

| 指摘 | 受け方 |
|---|---|
| 1. `api_key` が D2 / D3 / Phase 0 で三つ巴 | **受けた**。型から消し、名前だけ data_contract に予約 (D3) |
| 2. `voice: Option` と「必ず送る」の矛盾 | **受けた**。`voice: String` 必須、空 → `none` は `tts.ts` (D5)。`none` が Irodori の語彙である点を追記 |
| 3. 転送形式が D1 / D2 / CSP で不一致 | **受けた**。base64 → decode → Blob → ObjectURL に統一、revoke の契機を明記、画像の data URL の引用を撤回 (D1 / D2) |
| 4. 120 秒が初回 439 秒を救えない | **受けた**。試聴 = ウォームアップ (max(設定, 600)) + Timeout の文面で初回の可能性を言う + `IRODORI_PRELOAD` の案内 (D6) |
| 5. `stop()` と直列待ちの関係 | **受けた**。`discardOnStop` で分岐を 1 bit に閉じ、待ちは「世代をまたぐときの次ターンの出だしだけ」と限定、「合成中」表示 (D1 / D4) |
| 他 1. `Cors / Network` が無い | **半分**。backend の reqwest に CORS は存在しない (ブラウザの機構) ので `Cors` は作らない。`Connect` は `Network { detail }` に広げ、DNS・途中切断も拾う (D2) |
| 他 2. Phase 0 の凍結と Phase A の決定の順序逆転 | **受けた**。決める場所を D2 / D5 / D6 に寄せ、Phase 0 は写すだけ (D7) |
| 他 3. 80 字チャンクで声が保たれる保証が無い | **受けた・訂正**。`none` は参照なしなので README どおり揺れうる。再分割では消えないので v1 は手を入れず Phase D で測る (D5) |
| 他 4. `0 → None` の変換が書かれていない | **受けた**。`toRouteConfig` が 0 を null に、backend は `unwrap_or(120).max(5)` (D6) |

## 接地の限界 (rev2 時点)

- CORS で止まることは **dev サーバ (localhost:1420) で観測**した。配布ビルドの WebView も同じ Origin
  規則なので同じはずだが未確認。
- 45 秒/文はこの機 (GPU なし) の値。GPU 機では 30 秒以内に収まる可能性があり、そのときは 3 の動機が
  消えて 2 (CORS) だけが残る — どちらにせよ backend 経由で両方消える。
- Irodori 以外の OpenAI 互換 TTS サーバーで CORS がどうか・`none` を受けるかは測っていない (openedai-speech 等)。
- `none` のチャンク間の揺れは README の記述に依っており、耳で確かめていない (Phase D)。

## 実装の記録 (2026-10-19、Phase 0〜C + backend live)

- **backend** `app/src-tauri/src/tts.rs`: `TtsRouteConfig { endpoint, model, voice, speed, timeout_secs }` / `SpeechView { mime, data_b64 }` /
  `TtsRouteError` (serde tag `kind`: http / timeout / network / shape)。`encode` (4 欄・`voice` 常送・speed clamp) と `decode`
  (`audio/*` だけ・パラメータ落とし) は純関数、`validate` が空の model / voice / endpoint を HTTP の前に Shape で弾く
  (空の `voice` は frontend の正規化の契約が破れた印 — 黙って送らない)。`timeout()` は None と **0** を既定 120 に・下限 5。
  `synthesize` は reqwest 1 本 (`is_timeout` → Timeout {現在値}・それ以外は source 連鎖の底を Network)。command `synthesize_speech`
  は `Result<SpeechView, TtsRouteError>` をそのまま返す (frontend が object の `kind` で分岐)。
  PoC 4 本 (encode/validate/timeout 規則 / decode / **ローカル HTTP サーバで入口を通す** = 200 audio・400・200 非 audio・閉じたポート /
  応答しないサーバーの Timeout) + live 1 本 (ignored)。**live Green**: 実 Irodori (CPU) で `audio/wav` 349,484 バイト・**47.6 秒** (温まった状態)。
- **frontend** `app/src/tts.ts`: 継ぎ目 `Synth { speak(sentence, voice), stop, isPlaying, discardOnStop }`。`LibrarySynth` (従来のアダプタの包み・
  声の切替を `updateOptions` で「変わるときだけ」) / `BackendSynth` (invoke → base64 → Blob URL → `HTMLAudioElement`・`ended`/`error`/`stop`
  で revoke・stop は pause して自分で reject・invoke 中の stop は解決後に `stopped` を見て**再生しない**)。`interrupt()` は
  `discardOnStop && wasPlaying` のときだけ捨てる。`toRouteConfig(s, voice, minTimeout)` が空の voice を `none` に、0 を null に。
  `test()` は `ensureSynth(WARMUP_TIMEOUT_SECS=600)` で max(設定, 600)、終わったら `invalidate()` で本番に持ち越さない。
  `onSynthesizing` / `onRouteFailure` の購読 (store が `ttsBusy` とトーストへ橋渡し = `initTtsBridge`、App.vue の onMounted から 1 回)。
  **起草に無かった小さな追加**: `BackendSynth` は設定の音量を `audio.volume` に当てる (ライブラリ経路では効かなかった。高さは API に無いので不変)。
  既存の 3 エンジンの mount テスト (spec 35 Phase D の呼び出し列) は**無改修で緑** = ライブラリ経路の挙動不変。
  mount +4 (`tts.backend.mount.test.ts`: 1 文 1 invoke・none・voice 欄・アダプタ不生成・revoke / stop 中の結果を捨てる / 試聴 600・本番は設定 /
  失敗 4 種と購読)。Red は `|| "none"` と `if (this.stopped) throw` を外して確認。
- **設定・表示**: サウンドタブに待ち時間の select (0 = 既定 / 60 / 180 / 300 / 600・OpenAI 互換のときだけ)・話者 ID に placeholder `none` と注記
  (参照なしは声が揺れうる)・`openaiNote` から「未検証」を外し backend 経由を明記・試聴の失敗は `describeError(e, t)` で 4 種の文面
  (timeout は秒数と「初回はモデル読み込み中かも・試聴は 600 秒待つ」)。操作列に「合成中」(spinner、`ttsBusy`)。store のトースト
  `store.ttsRouteFailed`。i18n は ja/en。mount +4 (`SettingsDialog.tts` 2 / `TtsControls` 2)。Red は `v-if` と `describeError` を外して確認。
- **数**: app backend 80 → 84 (ignored 12 → 13)・frontend 214 → 222・workspace 491 不変・clippy clean・`npm run build` 成功。
- **✅Tauri 実機 (2026-10-19 ユーザー実測、n=1)**: backend 経由で Irodori の声が鳴った = #114 の 2 つの壁 (CORS・30 秒) は消えた。
  **`none` では声が変わる** (ユーザー「声は変わるね」) — README の予告どおりで、Kataribe は 1 文 1 リクエストなので
  チャンク間だけでなく**文の間でも**独立に合成される。D5 の判断 (v1 は手を入れない) のとおり、処方はコードではなく
  **参照音声**: サーバーの `voices/` に wav (flac / mp3 / ogg 等も可) を置くと**ファイル名の stem がそのまま voice ID**になり
  (`voices/alice.wav` → 話者 ID `alice`)、参照つきなら声が保たれる。キャラ別の声も同じ (登場人物ごとに別の stem)。
  `irodori.seed` (固定乱数) は vendor 欄のうえ、文が違えば結果も違うので声の固定にはならない。
- **✅参照音声で安定 (2026-10-19 ユーザー実測、n=1、「うまくいきました」)**: `voices/` に参照を置き stem を話者 ID に入れる
  手順で声が保たれた。`none` の揺れは Irodori の性質で、処方はサーバー側の参照音声 = v1 の判断 (D5) どおり。
- **未確認のまま**: stop 直後の次ターンの出だしの待ち (D4)。
- **作業中の罠 (自分の)**: bash の二重引用符の中で python -c にドキュメントコメントを渡したら、コメント内のバッククォート
  (`cargo test …`) をコマンド置換され tts.rs に `test result: ok…` が混入した。スクリプトはファイルに置いて `python -I` で走らせる。
