# spec 30: 利用量の計器 — トークンと枚数を役割別に数え、価格はユーザーが持ち込む

**Status**: rev1（2026-09-13 起草。4 点はユーザー裁定で凍結済み → `UsageLedger` を data_contract に凍結 → Phase A 着手）。

## 動機（ユーザーの言葉から）

「コスト測定の計器づくりをやりましょうか？」（2026-09-13）。前段の観察（2026-09-10 next_steps ④）:
ユーザー月 $100 の内訳が割れない。Opus 4.6 の文章が最良だが減りが早く、他は 1〜2 ドル級、
動画素材は Gemini の画像生成のみで一度きりの山 — という体感はあるが、**どの費目がいくらか**を
示す計器がアプリに無い。

コード実読で確定した現状（2026-09-13）:

| 経路 | 取れている | 取れていない |
|---|---|---|
| GM・要約・編集・エピローグ・挿絵プロンプト書き | リクエストごとの `Usage{prompt, completion, cache_read}` | 累計は cache 用（`CacheStat` の hit/total）だけで、**出力トークンはどこにも足していない**。役割別の集計なし |
| Perplexity（responses） | `cost` USD を decode | 読んで捨てている |
| 画像生成（`image_gen.rs`） | なし | usage を 1 バイトも読まない。OpenAI Images と Gemini は返している |
| 表示 | `TurnView.cache`（キャッシュ警告用） | セッション累計・役割別・画像の枚数 |

つまり計器は「ゼロから」ではなく、**足し算とその置き場が無い**。llm_client は役割ごとに別インスタンス
（GM / summary / editor / image_prompt）なので、client 単位で累計を持てば役割別は自動で割れる。

## 決定（2026-09-13 ユーザー裁定、4 点とも推奨どおり）

1. **価格表を持たない。焼き込み既定なし。** 登録モデル（`model_registry`）に任意の
   `pricing: Option { input_per_mtok_usd, cache_read_per_mtok_usd, output_per_mtok_usd }` を足す。
   None なら金額は出さない。見積もりは 3 つ揃った登録モデルだけ。08-20（AI モデル既定の撤去）と
   同型 — 価格は変わるし、**間違った金額は無いより悪い**。
2. **永続化はセッション揮発 + `app_data/logs/usage.jsonl` 追記。** セーブには入れない（セーブ汚染と
   再現性崩れを避ける）。セッション累計 = llm_client インスタンス単位の `UsageLedger`（プロセス内揮発）。
   永続 = 1 リクエスト 1 行の jsonl。**プロンプト本文は書かない**。時刻・role・model_id・Usage・
   cost_estimate（計算できた場合のみ）。ローテは Phase A では考えない（単一ファイル。Phase D の
   月次集計で要れば切る）。
3. **画像は枚数 + 提供者側トークン。** OpenAI Images = レスポンスの `usage` トークン / Gemini =
   `usageMetadata` / ComfyUI = `count + elapsed_sec`、cost は 0 扱い。Perplexity の `cost` もここで
   初めて捨てずに ledger へ。
4. **表示単位はトークン主、金額は併記。** 会話ログには出さない（ノイズ）。タイトルバーのモデル
   バッジ hover = このセッションの `prompt / cache_read / completion / requests`。設定 > AIモデル
   タブ = 役割別累計（client 単位なので自動で割れる）、価格入りモデルは `$` 併記。画像は
   `N 枚 / token / sec`。

### 2026-09-10「コードより先に請求書を割れ」との整合

あの順序は「推測で計器の形を決めるな」の意味。手作業の突合は 1 回きりで残らず、ユーザーの時間を
食う。計器を先に入れ、**ダッシュボードとの突合を Phase D の較正**に置き換えれば、同じ検証が以後の
セッションでも機械的に効く。順序の逆転ではなく検証工程の置換。

### rev1 で私が足した判断（査読で覆せる）

ユーザーの凍結案から 3 点ずらした。理由はいずれも「1 リクエスト = 1 イベント」を守るため。

- **(a) `Usage` に `cost_usd: Option<f64>`（プロバイダ申告）を足す。** Perplexity の `cost` は
  adapter の decode でここに載る。他の adapter は None。見積もり（価格表 × トークン）は提示層で
  計算し、**申告があれば申告を優先**する。
- **(b) `UsageLedger` に `reported_cost_usd: f64` と `reported_cost_requests: u64` を足す**（凍結案の
  4 列 + 2）。「Perplexity の cost を捨てずに ledger へ」の器が 4 列に無かった。申告の無い
  リクエストは 0 として足すので、`reported_cost_requests == 0` なら金額は意味を持たない。
- **(c) `UsageEvent::Search` は v1 では emit しない（予約のみ）。** Perplexity の `cost` はリクエスト
  全体の値で、Llm イベントの `cost_usd` に載る。同じリクエストから Llm と Search の 2 イベントを
  出すと、jsonl を素朴に合計した人が二重に数える。検索専用の呼び出しを作る日のために variant は残す。

## 何を作るか

### A. llm_client — 累計と機械可読行（Phase A）

- `usage.rs`（新規）: `UsageLedger { requests, prompt_tokens, cache_read_tokens, completion_tokens,
  reported_cost_usd, reported_cost_requests }` + `record(&Usage)`。`UsageEvent`（serde tag `kind`）=
  `Llm { role, model_id, usage, cost_usd }` / `Image { role, provider, count, usage_tokens, elapsed_sec }`
  / `Search { provider, cost_usd }`。`UsageSink = Arc<dyn Fn(&UsageEvent) + Send + Sync>`。
- `LlmClient` に `role`（`with_role`。app が `gm` / `summary` / `editor` / `image_prompt` を付ける。
  エピローグは GM の client を使うので `gm` に数える = 声の主が同じ）・`usage: Mutex<UsageLedger>`・
  `usage_sink: Option<UsageSink>`（`with_usage_sink`）。`usage_ledger()` でスナップショット。
- 記録点は `complete()` の **単一点**（既存の `record_cache` と同じ場所）。`chat` / `generate` /
  `generate_structured` / 4 adapter / 再送・降格の全経路がここを通る。`record_usage(&Usage)` =
  cache 記録（従来）→ ledger → `LLM_CACHE_DEBUG=1` で `[LLM_USAGE] conv= role= model= req= prompt=
  cache_read= completion= cost=` を stderr（`[LLM_CACHE_STAT]` の隣。スイッチは増やさない）→ sink。
- `responses.rs` が `cost.total_cost` を `Usage.cost_usd` へ。

**Phase A の受け入れ**: llm_client に累計が足され、`[LLM_USAGE]` 行が毎リクエスト出て、
**出力トークンが初めて足される**こと。

### B. 画像生成（Phase B）

`image_gen.rs` の decode に usage を足す。OpenAI Images = `usage.{input_tokens, output_tokens}`
（gpt-image-1 系）/ Gemini = `usageMetadata.{promptTokenCount, candidatesTokenCount}` / ComfyUI =
`count + elapsed_sec`（ポーリング開始〜取得）。`UsageEvent::Image { role: "illustration", provider,
count: 1, usage_tokens, elapsed_sec }` を app の sink へ。挿絵の**プロンプト書き**は LLM 側で
`role: image_prompt` として A で数える（画像とは別費目 = 動画素材の Gemini と挿絵の Gemini が
割れる）。

### C. 表示・価格・jsonl（Phase C）

- app: セッション開始で 1 本の sink（jsonl 追記）を全 client と image_gen に配る。行は
  `{"t": unix秒, "kind": ..., ...}`（`UsageEvent` を flatten）。プロンプト本文は載らない（型で載らない）。
- frontend: `aiModelProfiles` に `pricing?: { inputPerMtokUsd, cacheReadPerMtokUsd, outputPerMtokUsd }`
  （3 欄。空なら None = 金額なし）。見積もり = `(prompt − cache_read) × input + cache_read × cache_read
  + completion × output`（/1M）。**申告 (`cost_usd`) があれば申告を優先**。
- 表示: タイトルバーのモデルバッジ hover（このセッション）/ 設定 > AIモデル タブ（役割別累計 +
  価格入りは `$` 併記 + 画像 `N 枚 / token / sec`）。会話ログには出さない。

### D. 較正（Phase D）

実プレイ 1 セッション（`LLM_CACHE_DEBUG=1`）を回し、同時間帯のプロバイダのダッシュボードと
突合して**計器の誤差を記録する**。ここが 09-10 の「請求書を割る」の置き換え。誤差が出た費目は
adapter の usage 正規化を疑う（Anthropic の `input_tokens` が非キャッシュ分のみだった #58 の同族）。

## 名詞（data_contract `UsageMeter` に凍結）

```rust
struct UsageLedger {
    requests: u64,
    prompt_tokens: u64,        // 総入力 (cache_read を含む — spec 14 D5 の分母と同じ正規化)
    cache_read_tokens: u64,
    completion_tokens: u64,
    reported_cost_usd: f64,    // プロバイダ申告の合計 (rev1 追加 = 決定 3 の器)
    reported_cost_requests: u64,
}
enum UsageEvent {              // serde: {"kind": "llm" | "image" | "search", ...}
    Llm { role, model_id, usage: Usage, cost_usd: Option<f64> },
    Image { role, provider, count: u32, usage_tokens: Option<u64>, elapsed_sec: Option<f64> },
    Search { provider, cost_usd: f64 },   // v1 では emit しない (予約)
}
// Usage (canonical) += cost_usd: Option<f64>  (プロバイダ申告。今は Perplexity responses のみ)
```

## スコープ外（v1）

- 価格の自動取得・既定価格・為替。金額は USD 固定、ユーザーが書いた単価だけ。
- jsonl のローテ・集計 UI（月次は Phase D で必要になったら）。
- ゲスト（spec 23）側の集計 — AI を回すのはホストなので、ホストの計器で足りる。
- TTS・ノックサーバー・書庫の通信量。

## Phase 分割

- **A**: llm_client の `UsageLedger` / `UsageEvent` / `with_role` / `record_usage` / `[LLM_USAGE]` /
  Perplexity cost → `Usage.cost_usd`。役割の付与（app 4 箇所・CLI 3 箇所）。PoC 3 本。
- **B**: image_gen の usage decode + `Image` イベント。PoC: 3 プロバイダの decode。
- **C**: sink（jsonl）・pricing・表示。PoC: 見積もりの純関数（申告優先 / 3 欄揃わないと None）。
- **D**: 較正 1 セッション → 誤差を本 spec に記録。

## PoC 方針

- ledger の累計（completion と申告費用が足される・cache_stat の従来計数は不変）。
- sink がイベントを受け取り、role / model_id / cost が載る。
- `UsageEvent` の serde 形（`kind` タグ・`cost_usd` の null）— jsonl の読み手（Phase D の集計）が
  依存する形をここで固定する。
- Perplexity の decode が `cost_usd` を埋める（既存テストに assert を足す）。

## 未決

1. （C）見積もりの表示精度 — `$0.0123` の桁。小さすぎる額をどう見せるか（`< $0.01`）。
2. （C）jsonl の置き場を `app_data/logs/usage.jsonl` に固定するか、会話ログ保存先の設定に従わせるか。
   推奨は固定（会話ログはユーザーが読む物、jsonl は機械が読む物で役割が違う）。

## 波及

- data_contract `UsageMeter` 節（新設）+ `UnifiedToolLayer.chat_request` の usage 記述。
- CLAUDE.md 現状節に 1 bullet。
- outcast `package_spec.md` は無関係（作者向け形式に触れない）。
