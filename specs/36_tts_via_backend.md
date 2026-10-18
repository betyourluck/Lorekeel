# spec 36 — 読み上げの OpenAI 互換経路を backend 経由に

**Status**: Draft (2026-10-19 起草、failures #114 起点)。契約は Phase 0 で data_contract に `TtsBackendRoute` 節を凍結する (起草時点では未記載)。

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
  (OpenAI の `/v1/audio/speech`) は変えない。
- 他 3 エンジン (ブラウザ内蔵 / VOICEVOX / AivisSpeech) は**触らない**。動いているものを動かさない。
  VOICEVOX / AivisSpeech は Origin を反映する CORS を返すので、「汎用の口が動く前提をたまたま満たす
  サーバーで検証していた」のが #114 の機序。

### スコープ外

- Irodori-TTS 本体の Gradio API (`/gradio_api/call/_run_generation`) への対応 — 7860 はデモ画面で
  サーバーではない。参照音声の upload まで要るので別物。
- `stream_format: sse` (チャンク単位の SSE) — v2。まず 1 文 1 リクエストで成立させる。
- 旧 3 エンジンの backend 移設 — 動いているので動機が無い。
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
| CSP: `connect-src` は localhost のみ・`media-src` は `'self' asset: blob:` (**`data:` は無い**) | `app/src-tauri/tauri.conf.json` |
| backend の先例: 画像生成は `ImageGenConfig` を command に渡し、HTTP は reqwest・応答は base64 → frontend で data URL。タイムアウトはプロバイダ別既定 + `timeout_secs` 上書き | `app/src-tauri/src/image_gen.rs` / `lib.rs` `generate_image` |
| 秘密の env は `SECRET_ENV_KEYS` の一覧だけが資格情報ストアへ行く (`IMAGE_API_KEY_*` が先例) | `app/src-tauri/src/secret_store.rs` |
| Irodori-TTS-Server: `voice` 省略は `IRODORI_DEFAULT_VOICE` 未設定なら 400。`none` = Voice Design。任意の bearer `IRODORI_API_KEY`。`IRODORI_PRELOAD=false` 既定で初回にモデルを読む | README (2026-10-19) / `/health` |

---

## 設計

### D1. 経路の分岐は engine 1 つだけ

`engine === "openaiCompatible"` のときだけ合成を backend command へ回す。他 3 エンジンはライブラリのまま。
`tts.ts` に合成の継ぎ目 `Synth` (1 文を合成して再生し終えるまで待つ / 止める / 再生中か) を切り、
実装を 2 つ持つ:

- `LibrarySynth` = 今の `VoiceEngineAdapter` の包み (speakText / stop / isPlaying)。挙動不変。
- `BackendSynth` = `invoke("synthesize_speech")` → bytes → `Blob` → `URL.createObjectURL` → `HTMLAudioElement.play()`。
  再生終了 (`ended` / `error`) で resolve、`stop()` は `pause()` して**自分で reject する** (#72 の罠 =
  ライブラリの stop が play の Promise を settle しない、を自分では踏まない形)。Object URL は再生後に revoke。

鎖 (`chain`) / 世代 / `skipThrough` / 声の切替は `Synth` の外にあり不変。`updateOptions({speaker})` に当たるものは
`BackendSynth` では次のリクエストの `voice` 欄になるだけ。

### D2. backend = `tts.rs` (encode / decode 純関数 + HTTP ドライバ)

- command `synthesize_speech(config: TtsRouteConfig, text: String) -> SpeechView`。
- `TtsRouteConfig { endpoint (全体 URL), model, voice: Option, speed: f32, timeout_secs: Option<u64>, api_key: Option }`。
- `SpeechView { mime, data_b64 }` — 画像と同じく base64 文字列で運ぶ (WAV 1 文 ≈ 350 KB → 470 KB。
  `tauri::ipc::Response` の生バイト返しは repo に先例が無いので v1 は base64、measure が要れば v2)。
- 失敗は構造化: `Http { status, body_head }` / `Timeout { secs }` / `Connect` / `Shape` (audio でない)。
  frontend の試聴はこれをそのまま文面に出す (今は 404 も CORS も timeout も「失敗」の一語)。
- 既定タイムアウト **120 秒** (#114 の 45 秒 + モデル読み込みの余地。あらすじ要約の既定と同じ値)。
  `timeout_secs` は設定サウンドタブの select (60 / 120 / 180 / 300 / 600、0 = 既定) で、あらすじの
  `set_summary_timeout` と同じ「0 = 空 = backend の既定」の流儀。
- CSP は backend に無いので触らない。`media-src blob:` は既にある。

### D3. 鍵は v1 では持たない (未決 1)

Irodori の bearer は任意で既定 unset。backend 経由なら鍵が WebView に載らないので、OpenAI 本家や
クラウドの `/v1/audio/speech` も同じ口で使えるようになる — これは 2026-07-22 に CSP で**閉じた**判断の
再訪になるので、v1 は鍵欄を作らず、欲しくなったら `TTS_API_KEY_OPENAI_COMPAT` を `SECRET_ENV_KEYS` に
足す (1 欄 + 資格情報ストア 1 キー)。

### D4. 中断と backend の進行中リクエスト (未決 2)

`stop()` / `skip()` は frontend の再生を止めるが、backend で走っている reqwest は止めない (Tauri の
command に cancel は無い)。結果は世代で捨てる。サーバー側は `IRODORI_MAX_CONCURRENT_SYNTHESIS=1`
なので、捨てたリクエストが終わるまで次の文は待たされる (CPU なら最大 45 秒)。v1 はこの待ちを許容し、
効くなら v2 で `tokio::select!` + 世代 token で drop する。

### D5. 分割は文単位のまま (未決 3)

`splitSentence` で 1 文 1 リクエスト (他エンジンと同じ)。Irodori 側にも 80 字のチャンク分割があり、
`voice` 未指定だとチャンクごとに声が変わりうる (README) — Kataribe は `voice` を必ず送る形にするので
(話者 ID が空なら `none` を既定で送る = **#114 の 400 を UI の説明なしに踏ませない**)、二重分割は
声の揺れにならない。段まとめ送りは CPU だと 1 段 数分になるので採らない。

### D6. 計器 (spec 30) には載せない

ローカル TTS は課金が無く、Irodori の応答にも usage は無い。画像の `ImageLedger` に相当するものは v1 では持たない。

---

## Phase

- **0**: data_contract に `TtsBackendRoute` (command の形・既定タイムアウト・失敗の種別・鍵なし) を凍結。
- **A (backend)**: `tts.rs` の encode (body の形・`voice` 既定 `none`・bearer は None なら送らない) /
  decode (content-type から mime、audio でなければ Shape) を純関数で + command + 登録。
  PoC: 純関数 2〜3 本 + **ローカル HTTP サーバから WAV を流して command の入口を通す 1 本**
  (#109 の教訓 = 純関数だけでは配線の穴が見えない) + live 1 本 (ignored、実 Irodori)。
- **B (frontend seam)**: `Synth` 2 実装、`speakParts` / `test` / `interrupt` を seam 越しに。他 3 エンジンの
  経路は **mount テストで byte 一致**は取れないので、ライブラリ adapter への呼び出し列 (speakText / stop /
  updateOptions の順) を偽装ライブラリで固定している既存テスト (spec 35 Phase D) が回帰を見る。
  `BackendSynth` は `invoke` を偽装して「1 文 1 invoke・停止で reject・声は欄で渡る」を mount で固定。
- **C (設定)**: サウンドタブに待ち時間の select + 失敗文言 (status / timeout / connect を区別)。
  話者 ID の説明に「Irodori は `none` か voices/ のファイル名」。
- **D (実測)**: Irodori-TTS-Server (CPU) で 1 ターンぶん読み切る / 試聴の失敗 3 種が区別して出る /
  `stop()` 直後の次ターンがサーバーの直列待ちでどれだけ遅れるか (D4 の判断材料)。GPU 機は未測定のまま申告。

---

## 未決

1. 鍵欄を作るか (= クラウド TTS を開くか)。v1 は作らない (D3)。
2. backend の進行中リクエストの drop (D4)。v1 は許容。
3. 文単位 / 段単位 (D5)。v1 は文単位。
4. base64 か生バイト (`tauri::ipc::Response`) か。v1 は base64 (先例に揃える)。

## 接地の限界 (起草時点)

- CORS で止まることは **dev サーバ (localhost:1420) で観測**した。配布ビルドの WebView も同じ Origin
  規則なので同じはずだが未確認。
- 45 秒/文はこの機 (GPU なし) の値。GPU 機では 30 秒以内に収まる可能性があり、そのときは 3 の動機が
  消えて 2 (CORS) だけが残る — どちらにせよ backend 経由で両方消える。
- Irodori 以外の OpenAI 互換 TTS サーバーで CORS がどうかは測っていない (openedai-speech 等)。
