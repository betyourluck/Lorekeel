# spec 30: 利用量の計器 — トークンと枚数を役割別に数え、価格はユーザーが持ち込む

**Status**: rev2（2026-09-13 起草 → 4 点はユーザー裁定で凍結 → Phase A 実装 → **同日査読 2 本
（矛盾 9 + 6）を反映 = rev2、Phase A のコードも rev2 に追従**。残 = B 画像 / C 表示・価格・jsonl / D 較正）。
査読の反映は末尾「査読の反映」節に凍結。

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

## 決定（2026-09-13 ユーザー裁定、4 点とも推奨どおり。rev2 で文言を型と一致させた）

1. **価格表を持たない。焼き込み既定なし。** 登録モデル（`aiModelProfiles`）に任意の
   `pricing: Option<Pricing { input_per_mtok_usd: f64, cache_read_per_mtok_usd: f64, output_per_mtok_usd: f64 }>`。
   **`Pricing` の中は全欄 `f64`（個別に Option にしない）** — 3 欄揃わなければ `None`、部分的な
   見積もりは作らない。None なら金額は出さない。08-20（AI モデル既定の撤去）と同型 —
   価格は変わるし、**間違った金額は無いより悪い**。**プロバイダが費用を申告する場合（今は
   Perplexity の `cost` のみ）はそれを捨てず `Usage.cost_usd` に載せ、見積もりより申告を優先する。**
2. **永続化はセッション揮発 + `app_data/logs/usage.jsonl` 追記（置き場は固定）。** セーブには入れない
   （セーブ汚染と再現性崩れを避ける）。セッション累計 = client 単位の `LlmLedger`（プロセス内揮発）+
   app セッション単位の `ImageLedger`。永続 = **1 イベント 1 行**の jsonl で、中身は
   **`t_ms + UsageEvent` だけ**（`LoggedUsageEvent`）。**プロンプト本文は書かない（型に欄が無い）。
   見積もり額も書かない** — 単価は後から変わるので、焼くと `pricing_version` まで要る。金額は読む側が
   その時の単価で出す。ローテは Phase A では考えない（単一ファイル。Phase D の月次集計で要れば切る）。
3. **画像は枚数 + 提供者側トークン（入力・出力の二値）。** OpenAI Images = `usage.{input_tokens,
   output_tokens}` / Gemini = `usageMetadata.{promptTokenCount, candidatesTokenCount}` / ComfyUI =
   `count + elapsed_sec` でトークン None、cost は 0 扱い。`Image` イベントは費用欄を持たない。
4. **表示単位はトークン主、金額は併記。** 会話ログには出さない（ノイズ）。タイトルバーのモデル
   バッジ hover = このセッションの `prompt / cache_read / completion / requests`（**全 client の
   `LlmLedger` を `absorb` で合算**）。設定 > AIモデル タブ = 役割別累計（client 単位）、価格入り
   モデルは `$` 併記。画像は `N 枚 / prompt / completion / sec`。

### 2026-09-10「コードより先に請求書を割れ」との整合

あの順序は「推測で計器の形を決めるな」の意味。手作業の突合は 1 回きりで残らず、ユーザーの時間を
食う。計器を先に入れ、**ダッシュボードとの突合を Phase D の較正**に置き換えれば、同じ検証が以後の
セッションでも機械的に効く。順序の逆転ではなく検証工程の置換。

### rev1 で私が足し、rev2 で査読を通した判断

- **(a) 費用の置き場は `Usage.cost_usd: Option<f64>` の 1 箇所。** Perplexity の `cost` は adapter の
  decode でここに載る。他の adapter は None。rev1 は `UsageEvent::Llm` にも `cost_usd` を持たせていた
  （二重定義 — 査読 #1-1 / #2-1）。**Llm イベントは `usage` だけを運ぶ。**
- **(b) `LlmLedger` に `reported_cost_usd: f64` と `reported_cost_requests: u64`。** 「申告を捨てずに
  ledger へ」の器。申告の無いリクエストは 0 として足す。**加算経路は `Llm` だけ**（`Image` は費用欄を
  持たず、`Search` は emit しない）。
- **(c) `UsageEvent::Search` は v1 では emit しない（予約のみ）。** Perplexity の `cost` はリクエスト
  全体の値で、`Usage.cost_usd` に載る。同じリクエストから Llm と Search の 2 イベントを出すと jsonl を
  素朴に合計した人が二重に数える。検索専用の呼び出しを作る日のために variant は残す。

## 何を作るか

### A. llm_client — 累計と機械可読行（✅Phase A、rev2 追従済み）

- `usage.rs`: `LlmLedger { requests, prompt_tokens, cache_read_tokens, completion_tokens,
  reported_cost_usd, reported_cost_requests }` + `record(&Usage)` + `absorb(&LlmLedger)`（セッション
  合計用）。`ImageLedger { requests, count, prompt_tokens, completion_tokens, elapsed_sec }` +
  `record(...)`（置き場は app、B 節）。`UsageEvent`（serde tag `kind`）= `Llm { role, model_id, usage }`
  / `Image { role, provider, count, prompt_tokens?, completion_tokens?, elapsed_sec? }` /
  `Search { provider, cost_usd }`。`LoggedUsageEvent { t_ms, #[serde(flatten)] event }` = jsonl の
  1 行の形（時刻は **unix ミリ秒** — 秒だと同一秒の順序が潰れる）。
  `UsageSink = Arc<dyn Fn(&UsageEvent) + Send + Sync>`。
- `LlmClient` に `role`（`with_role`）・`usage: Mutex<LlmLedger>`・`usage_sink: Option<UsageSink>`
  （`with_usage_sink`）。`usage_ledger()` でスナップショット。
- 記録点は `complete()` の **単一点**（既存の `record_cache` と同じ場所）。`chat` / `generate` /
  `generate_structured` / 4 adapter / 再送・降格の全経路がここを通る。`record_usage(&Usage)` =
  cache 記録（従来）→ ledger（**ロックを解放してから**）→ `LLM_CACHE_DEBUG=1` で
  `[LLM_USAGE] conv= role= model= req= prompt= cache_read= completion= cost=` を stderr
  （`conv` = `[LLM_CACHE_STAT]` と同じ `LlmClient.conv_id`。スイッチは増やさない）→ sink。
  **sink は Mutex の外で呼ぶ**（sink が `usage_ledger()` を呼んでもデッドロックしない = PoC）。
- `responses.rs` が `cost.total_cost` を `Usage.cost_usd` へ。
- **`prompt_tokens` は総入力**（cache_read を含む）。**正規化は adapter の責務**で、Anthropic は
  `input_tokens`（非キャッシュ分のみ）+ `cache_read` + `cache_creation` を足して canonical へ渡す
  （failures #58 で実装済・PoC あり）。ledger と見積もりは**足し直さない**。

**役割の閉集合**: `gm` / `summary` / `editor` / `image_prompt`（LLM、client 単位）+ `illustration`
（画像。`Image` イベントと `ImageLedger` だけに現れる）。エピローグは GM の client を使うので `gm`
に数える（声の主が同じ）。

### B. 画像生成（Phase B）

`image_gen.rs` の decode に usage を足す。OpenAI Images = `usage.{input_tokens, output_tokens}`
（gpt-image-1 系）/ Gemini = `usageMetadata.{promptTokenCount, candidatesTokenCount}` / ComfyUI =
`count + elapsed_sec`（ポーリング開始〜取得）でトークン None。**累計の置き場は app の管理状態
`UsageState { image: Mutex<ImageLedger> }`**（`LlmClient` ではないので client には置けない。
セッション開始 = `new_game` / `resume` でリセット）。1 枚ごとに `UsageEvent::Image { role:
"illustration", provider, count: 1, prompt_tokens, completion_tokens, elapsed_sec }` を sink へ。
挿絵の**プロンプト書き**は LLM 側で `role: image_prompt` として A で数える（画像とは別費目 =
動画素材の Gemini と挿絵の Gemini が割れる）。

### C. 表示・価格・jsonl（Phase C）

- app: セッション開始で 1 本の sink（jsonl 追記）を全 client と image_gen に配る。行は
  `LoggedUsageEvent`（`{"t_ms": ..., "kind": ..., ...}`）。置き場は `app_data/logs/usage.jsonl` 固定
  （会話ログはユーザーが読む物・jsonl は機械が読む物で役割が違うので、会話ログの保存先設定には
  従わせない）。
- frontend: `aiModelProfiles` に `pricing?: { inputPerMtokUsd, cacheReadPerMtokUsd, outputPerMtokUsd }`。
  **保存時に 3 欄揃わなければ `undefined`**（部分は捨てる）。
- **金額の規則（純関数・PoC）**: ledger 1 つに対し、`reported_cost_requests > 0` なら**申告の合計を出し、
  見積もらない**（`reported_cost_requests < requests` なら「部分申告」と表示 = 未申告分は数えない）。
  申告が 0 件で `pricing` があれば見積もり = `(prompt − cache_read) × input + cache_read × cache_read
  + completion × output`（/1M）。どちらも無ければ金額なし。**client の provider は生存期間中不変**
  （config は生成時に固定）なので、1 つの ledger に申告ありと無しが混ざる形は構造的に起きない —
  この規則は「起きても二重に数えない」ための保険。
- 表示: 決定 4 のとおり。セッション合計は全 client の `LlmLedger` を `absorb` で足す（GM / summary /
  editor / image_prompt の全部）+ `ImageLedger`。

### D. 較正（Phase D）

実プレイ 1 セッション（`LLM_CACHE_DEBUG=1`）を回し、同時間帯のプロバイダのダッシュボードと
突合して**計器の誤差を記録する**。ここが 09-10 の「請求書を割る」の置き換え。誤差が出た費目は
adapter の usage 正規化を疑う（#58 の同族）。

## 名詞（data_contract `UsageMeter` に凍結、rev2）

```rust
// canonical
struct Usage { prompt: u64 /* 総入力 */, completion: u64, cache_read: u64, cost_usd: Option<f64> /* 申告 */ }

// usage.rs
struct LlmLedger {            // client 単位・揮発
    requests: u64,
    prompt_tokens: u64,       // 総入力 (cache_read を含む — spec 14 D5 の分母と同じ正規化、adapter が保証)
    cache_read_tokens: u64,
    completion_tokens: u64,
    reported_cost_usd: f64,   // 申告の合計 (加算経路は Llm のみ)
    reported_cost_requests: u64,
}
struct ImageLedger { requests: u64, count: u64, prompt_tokens: u64, completion_tokens: u64, elapsed_sec: f64 }  // app セッション単位
enum UsageEvent {             // serde: {"kind": "llm" | "image" | "search", ...}
    Llm { role, model_id, usage: Usage },
    Image { role, provider, count: u32, prompt_tokens: Option<u64>, completion_tokens: Option<u64>, elapsed_sec: Option<f64> },
    Search { provider, cost_usd: f64 },   // v1 では emit しない (予約)
}
struct LoggedUsageEvent { t_ms: u64, #[serde(flatten)] event: UsageEvent }   // jsonl の 1 行
type UsageSink = Arc<dyn Fn(&UsageEvent) + Send + Sync>;

// frontend (aiModelProfiles)
type Pricing = { inputPerMtokUsd: number, cacheReadPerMtokUsd: number, outputPerMtokUsd: number } | undefined
```

## スコープ外（v1）

- 価格の自動取得・既定価格・為替。金額は USD 固定、ユーザーが書いた単価だけ。
- jsonl のローテ・集計 UI（月次は Phase D で必要になったら）。
- ゲスト（spec 23）側の集計 — AI を回すのはホストなので、ホストの計器で足りる。
- TTS・ノックサーバー・書庫の通信量。

## Phase 分割

- **A**（✅）: llm_client の `LlmLedger` / `ImageLedger` / `UsageEvent` / `LoggedUsageEvent` /
  `with_role` / `record_usage` / `[LLM_USAGE]` / Perplexity cost → `Usage.cost_usd`。役割の付与
  （app 4 箇所・CLI 3 箇所）。PoC 5 本。
- **B**: image_gen の usage decode + `Image` イベント + `UsageState.image`。PoC: 3 プロバイダの decode。
- **C**: sink（jsonl）・pricing・表示。PoC: 金額の純関数（申告優先 / 部分申告 / 3 欄揃わないと None）。
- **D**: 較正 1 セッション → 誤差を本 spec に記録。

## PoC 方針

- ledger の累計（completion と申告費用が足される・cache_stat の従来計数は不変・`absorb` の合算）。
- sink がイベントを受け取り role / model_id / usage が載る。**sink の中から `usage_ledger()` を呼んでも
  デッドロックしない**（ロック解放後に呼ぶことの固定）。
- serde 形: `kind` タグ・`cost_usd` の null・`LoggedUsageEvent` の flatten（`t_ms` と `kind` が同じ段）・
  本文欄の不在 — jsonl の読み手（Phase D の集計）が依存する形。
- Perplexity の decode が `cost_usd` を埋める（既存テストに assert）。

## 未決

1. （C）見積もりの表示精度 — `$0.0123` の桁。小さすぎる額をどう見せるか（`< $0.01`）。

## 査読の反映（2026-09-13、2 本）

査読 #1（矛盾 4 + 詰む 5 + 細かい 3）/ 査読 #2（6 点）。重なりは 1 行にまとめる。

| # | 指摘 | 反映 |
|---|---|---|
| 1-1 / 2-1 | `cost_usd` が `Usage` と `UsageEvent::Llm` の両方にある | `Usage.cost_usd` に一本化。`Llm { role, model_id, usage }` |
| 1-2 / 2-5 | jsonl の中身が 3 箇所で違う（`cost_estimate` の有無・時刻欄の不在） | `LoggedUsageEvent { t_ms, flatten event }` を型で定義。`cost_estimate` は書かない（単価変更で腐る）。決定 2 の文言を型に合わせた |
| 1-3 / 2-2 | 画像の累計の置き場が無い・「1 リクエスト 1 行」が Image で崩れる | `LlmLedger`（client）と `ImageLedger`（app `UsageState`）に分離。jsonl は「1 イベント 1 行」 |
| 1-4 / 2-3 | `Image.usage_tokens` 単一 vs プロバイダの二値 | `prompt_tokens` / `completion_tokens` に分割（ComfyUI は None） |
| 1-5 | 役割の集合が不一致・セッション合計の処理が無い | 閉集合を明記（`illustration` は画像だけ）。合計は全 client の `absorb` |
| 1-6 | pricing の型 — 欄ごと Option だと部分見積もりが可能 | `Option<Pricing>`、中は全欄 `f64`。frontend は 3 欄揃わなければ undefined |
| 1-7 | `prompt_tokens` の正規化責務 | adapter の責務と明記（Anthropic は #58 で実装済）。ledger・見積もりは足し直さない |
| 1-8 | jsonl 置き場が決定と未決で矛盾 | 固定に倒し未決 2 を削除 |
| 1-9 | `reported_cost` の加算経路と `Search` | 加算経路は `Llm` のみ、`Image` は費用欄を持たない、`Search` は emit しない、と明記 |
| 1-細 1 | `conv` 未定義 | `LlmClient.conv_id`（`[LLM_CACHE_STAT]` と同一）と明記 |
| 1-細 2 | sink を Mutex 保持中に呼ぶとデッドロック | ロック解放後に呼ぶ。PoC で sink 内から `usage_ledger()` を呼ぶ |
| 1-細 3 | `t: unix 秒` は同一秒の順序が潰れる | `t_ms`（ミリ秒） |
| 2-4 | 決定 3（画像）に Perplexity が混ざる | 決定 1 へ移した（申告費用は価格の話） |
| 2-6 | 申告あり・なしの混在で二重計上 | 金額の規則を明記（申告 1 件でもあれば見積もらない・部分申告は表示）。混在は provider 不変ゆえ構造的に起きない = 規則は保険 |

査読 #1 の「凍結済み `UsageLedger` に手を入れるので Phase A 着手前に」は、Phase A 実装後に届いた
ので**コードを rev2 に追従させた**（名前と欄の変更のみ。記録点・経路は不変）。

## 波及

- data_contract `UsageMeter` 節（rev2 で書き換え）。
- CLAUDE.md 現状節の spec 30 bullet。
- outcast `package_spec.md` は無関係（作者向け形式に触れない）。
