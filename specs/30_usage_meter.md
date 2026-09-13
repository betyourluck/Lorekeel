# spec 30: 利用量の計器 — トークンと枚数を役割別に数え、価格はユーザーが持ち込む

**Status**: rev2（2026-09-13 起草 → 4 点はユーザー裁定で凍結 → Phase A 実装 → **同日査読 2 本
（矛盾 9 + 6）を反映 = rev2、Phase A のコードも rev2 に追従** → ユーザー承認 → **✅Phase B（同日）** → **✅Phase C（同日）**。
残 = GUI 目視（ユーザー実機）。**✅Phase D 実測 1（同日）**: Anthropic Console と 5 ターンを突合し、計器 $0.1224 vs Console $0.13 = 約 6% の過小で原因はキャッシュ書き込みの割増（下の D 節）。
**追補（同日）**: 価格表 `prices.json` からの取り込み（裁定 1 を「焼き込み既定なし・取り込みは opt-in」へ改訂、下の C-2 節）。
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
   **←追補 2026-09-13（ユーザー要望「単価表は prices.json から取得できる。自動で割り当てられるか」）**:
   文言を「**焼き込み既定なし・取り込みは opt-in**」へ改訂。アプリは価格を**持たない**ままで、ユーザーが保守する
   外部の表（既定 `https://betyourluck.github.io/prices.json`、URL は差し替え可）を**押したときだけ**取りに行き、
   モデル名で照合して単価 3 欄とコンテキスト長を**フォームに埋めるだけ**。保存は従来の経路（黙って登録を書き換えない）、
   出所と表の取得日を状態行に必ず添える。裁定の根拠（価格は変わる・間違った金額は無いより悪い）はそのまま —
   焼き込みは「いつの価格か分からない値が黙って居座る」から悪く、取り込みは「いつのどの表から来たか」が見えるので両立する。
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

### B. 画像生成（✅Phase B、2026-09-13）

**合算則（ユーザー指摘 1 への決定）**: `ImageLedger` はトークンを 0 として足し、表示側は
`prompt_tokens + completion_tokens == 0` かつ `requests > 0` なら `-`（ComfyUI だけのセッション）。
二値のどちらかが返れば数える。`elapsed_sec` は合計。**リセット点（指摘 2）**: `new_game` /
`restore_session`（resume / load_slot / resume_from_file の共通経路）= 新セッションでメモリの
`ImageLedger` だけ消し、jsonl は追記のまま。実装: `image_gen::ImageUsage { count, prompt_tokens,
completion_tokens, elapsed_sec }` を `Generated` に載せ、decode とは**別の純関数** `openai_usage` /
`gemini_usage` で拾う（null・欠落・JSON でない本文は None = 計器の失敗で絵を失わない）。秒は
OpenAI / Gemini = 往復、ComfyUI = `/prompt` 発行〜`/view` 取得（参照のアップロードは含めない）。
`count` は常に 1（Kataribe は history の先頭 1 枚しか取らない）。app `UsageState { image, sink }`
（std Mutex・`app.state()` で引く）、`generate_image` は**世代不一致で捨てる判定の前に**記録する
（課金は起きている）。PoC: image_gen 1 本（3 プロバイダ × null / 欠落 / 壊れた本文）+ app 1 本
（累計・イベント・リセットで sink は残る）。

`image_gen.rs` の decode に usage を足す。OpenAI Images = `usage.{input_tokens, output_tokens}`
（gpt-image-1 系）/ Gemini = `usageMetadata.{promptTokenCount, candidatesTokenCount}` / ComfyUI =
`count + elapsed_sec`（ポーリング開始〜取得）でトークン None。**累計の置き場は app の管理状態
`UsageState { image: Mutex<ImageLedger> }`**（`LlmClient` ではないので client には置けない。
セッション開始 = `new_game` / `resume` でリセット）。1 枚ごとに `UsageEvent::Image { role:
"illustration", provider, count: 1, prompt_tokens, completion_tokens, elapsed_sec }` を sink へ。
挿絵の**プロンプト書き**は LLM 側で `role: image_prompt` として A で数える（画像とは別費目 =
動画素材の Gemini と挿絵の Gemini が割れる）。

### C. 表示・価格・jsonl（✅Phase C、2026-09-13）

**実装で 1 点だけ設計を動かした**: editor と image_prompt の client は**呼び出しごとに生成・破棄**される
ので、client 内の `LlmLedger` だけでは役割別累計が消える。ゆえに **app の `UsageState` を sink そのもの**
にし（`UsageState::sink()` が `Arc<UsageInner>` を掴む closure を返す）、全イベントをここで①役割別
`BTreeMap<role, RoleUsage{model_id, ledger}>` に累計 ②jsonl へ追記する。client 内の ledger は GM の
スナップショットとして残る（`usage_ledger()`）。合計は snapshot 時に全役割を `absorb`。
`usage_snapshot` command → `UsageSnapshotView { llm: [{role, model_id, ledger}], total, image }`。
jsonl は `append_usage_line`（`create_dir_all` → append open → 1 行、時刻はここで打つ）、失敗は
stderr（計器の失敗でプレイを止めない）。置き場は setup で `app_data/logs/usage.jsonl` を固定。
配線 = gm / summary（new_game と restore_session の 2 箇所、同じ 1 本を共有）/ editor / image_prompt。
frontend = `usage.ts`（純関数: `costOf` / `parsePricing` / `readPricing` / `pricingFor` / `formatUsd` /
`imageTokensLabel`、vitest 10 本）/ `aiModelProfiles[].pricing?`（`readPricing` で検査、部分は捨てる）/
設定 > AIモデル に単価 3 欄（登録モデルの欄。「保存 + 登録モデルを更新」と新規登録で保存、.env には
書かない）と「利用量（このセッション）」の表（役割別・画像行・合計。合計の金額は出せる役割だけ足し、
出せない役割があれば `≥` で下限と示す）/ タイトルバーのバッジ hover に合計（mouseenter で取り直す）。
未決 1 の暫定 = `formatUsd`: 0 は `$0`、1e-4 未満は `< $0.0001`、1 ドル未満は 4 桁、以上は 2 桁。


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

### C-2. 価格表の取り込み（✅追補、2026-09-13）

- **backend** `fetch_price_table(url)` → `PriceTable { version, fetched, models: [PriceEntry { key, max_input_tokens?, input_per_mtok, output_per_mtok, cache_read_per_mtok? }] }`。
  取ってきて形を検めるだけ（`parse_price_table` 純関数: 版 1 以外・空キー・負や非数の単価は拒む = 版が上がって欄の意味が
  変わった表を黙って読むと間違った金額が入る）。上限 8MB・30 秒。WebView からは CSP（connect-src localhost 限定）で
  直接叩けないので backend に居る（画像生成・書庫と同じ理由）。知らない欄（cache_write 等）は読み捨てる。
- **frontend** `prices.ts`（純関数・vitest 11 本）: `normalizeModelKey`（`/` の後ろ・`@` の前・**英字だけの段** `anthropic.` `us.` を剥ぐ。
  `gemini-3.5-flash` の `3.5` は段ではない）/ `matchPrice`（**完全一致 → 正規化一致**。正規化の候補が複数で**価格が食い違えば
  `ambiguous`** = 埋めずに候補を見せる。同価格なら素の行を優先）/ `pricingFormFromEntry`（**キャッシュ読みが表に無い行は空のまま**
  = 入力単価を写して「割引なし」とするのは推測なので埋めない。3 欄揃わないと見積もりは出ない、が既存の規則）/
  `parseContextTokens` / `readContextTokens`（正の整数だけ）。
- **実表の形が照合規則を決めた**: 1,732 行のうち正規化で 1,421 群、**131 群が価格の食い違う候補を持つ**。`claude-opus-4-8` は
  接頭辞違いで 9 行あり、**地域つき（us./eu./jp./au. = Bedrock）は 1 割高い**（5.0 vs 5.5）。完全一致が無ければ
  `ambiguous` に落ちる = 「近い行を黙って選ばない」が実データで必要だった。`gpt-5.6-luna` も `openai.` 行だけ 1 割高い。
  キャッシュ読みは 490 行にしか無い（`grok-4.3` に無い）。文脈長は 1,493 行（`openai/…` の転記行には無い →
  同じモデルの他の行から補う）。
- **登録モデル** `aiModelProfiles[].contextTokens?: number`（任意・正の整数・.env には書かない・engine も llm_client も読まない）。
  最初の使い道 = 利用量の表のモデル欄に「コンテキスト長 N」、バッジ hover に 1 行。上限に近づいた警告は次（1 リクエストの
  入力トークンを ledger は持たない = `CacheStat.recent` の `prompt` を引く形が候補）。
- **UI**: 単価 3 欄の下にコンテキスト長 + 価格表の URL（`kataribe.priceTableUrl`、既定のときは書かない）+「単価を取り込む」。
  状態行 = 埋めた行のキーと取得日 / キャッシュ読みが無い旨 / 候補一覧（`key: in / cache / out`）/ 表に無い / 取得失敗。
  モデル名が空なら取りに行かない。
- **新規登録時の自動前埋めは入れなかった** — 埋める前提として表の取得（ネットワーク）が要り、モデル名を打つたびに走らせる形は
  opt-in の線を越える。ボタンは新規登録の前にも押せるので同じ動線で足りる。

### D. 較正（Phase D）

実プレイ 1 セッション（`LLM_CACHE_DEBUG=1`）を回し、同時間帯のプロバイダのダッシュボードと
突合して**計器の誤差を記録する**。ここが 09-10 の「請求書を割る」の置き換え。誤差が出た費目は
adapter の usage 正規化を疑う（#58 の同族）。

**✅実測 1（2026-09-13、Anthropic Console × claude-sonnet-4-6、ユーザー実プレイ 5 ターン）**:

| 出所 | 値 |
|---|---|
| 計器（利用量の表） | 5 回 / 入力 68,900（うちキャッシュ 45,688）/ 出力 2,603 / **≈ $0.1224**（単価 3 / 0.3 / 15） |
| jsonl（5 行の和） | 表と一致（12,223+14,314+13,546+14,133+14,684 = 68,900 / cache_read 0+11,422×4 / 出力 2,603） |
| Console 組織クレジット | $16.10 → $15.97 = **$0.13**（表示はセント単位） |
| Console 今月の使用額 | $71.8x → $71.93（同じ $0.13 と整合） |

**誤差 = 計器が約 $0.008（約 6%）少なく、方向は過小・原因は同定できた**: Console の表示がセント丸めなので
真値は $0.125〜0.135 の幅にあり、計器の 0.1224 はその下にわずかに外れる。差の説明は**キャッシュ書き込みの割増**
— 1 リクエスト目は `cache_read=0` で prompt 12,223、2 回目から `cache_read=11,422` なので、1 回目に 11,422
トークンが書き込まれている（Anthropic は書き込みを入力の 1.25 倍で課金）。割増分 = 11,422 × (1.25 − 1) × $3 / 1M
= **$0.0086** → 計器 + 割増 = $0.1310 ≒ Console の $0.13。**計器はキャッシュ書き込みを素の入力単価で数えている**
（adapter の正規化 #58 が `input + cache_read + cache_creation` を prompt に畳み、ledger は書き込みを別に持たない。
`Pricing` も 3 欄で書き込み単価が無い）。

**判断**: 誤差の向きと原因が分かったので v1 はこのまま（過小 6% は「内訳を割る」目的には足りる）。直すなら
①adapter が `cache_creation` を canonical `Usage` に別欄で出す（Anthropic だけが返す。他は 0）②`LlmLedger` に
`cache_write_tokens` ③`Pricing` に 4 欄目 `cacheWritePerMtokUsd`（prices.json は `cache_write_per_mtok` を持つ行が
あるので取り込みで埋まる）④`costOf` が `cache_write × cache_write 単価` を足す — の 4 点セットで、単価が無い登録
では従来どおり素の入力単価で数える（見積もりが今より良くなるだけで、悪くなる経路は無い）。優先度は低い
（Anthropic 以外では差が出ない・長いセッションほど読みが支配して割増の比率は下がる）。

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

- ~~価格の自動取得~~・既定価格・為替。金額は USD 固定。価格表からの**手動取り込み**は追補 C-2 で入った（自動＝黙って登録を書き換える形は今も外）。
- jsonl のローテ・集計 UI（月次は Phase D で必要になったら）。
- ゲスト（spec 23）側の集計 — AI を回すのはホストなので、ホストの計器で足りる。
- TTS・ノックサーバー・書庫の通信量。

## Phase 分割

- **A**（✅）: llm_client の `LlmLedger` / `ImageLedger` / `UsageEvent` / `LoggedUsageEvent` /
  `with_role` / `record_usage` / `[LLM_USAGE]` / Perplexity cost → `Usage.cost_usd`。役割の付与
  （app 4 箇所・CLI 3 箇所）。PoC 5 本。
- **B**（✅）: image_gen の usage decode + `Image` イベント + `UsageState.image`。PoC: 3 プロバイダの decode + app の状態。
- **C**（✅）: sink（jsonl）・pricing・表示。PoC: app 1 本（役割別累計・jsonl 1 イベント 1 行・リセットで jsonl は残る）+ frontend vitest 10 本（申告優先 / 部分申告 / 見積もりの式 / 3 欄揃わないと None / 表示）。
- **C-2**（✅追補）: 価格表の取り込み。PoC: app 1 本（`parse_price_table` の任意欄・版違い・不正行）+ frontend vitest 11 本（正規化 / 完全一致が地域つきの高い行に負けない / 同価格の候補は素の行 / 食い違えば ambiguous / 表に無い / キャッシュ読み無しは空 / 文脈長の入力と保存値）+ aiProfiles +1（contextTokens の前方互換）。
- **D**: 較正 1 セッション → 誤差を本 spec に記録。

## PoC 方針

- ledger の累計（completion と申告費用が足される・cache_stat の従来計数は不変・`absorb` の合算）。
- sink がイベントを受け取り role / model_id / usage が載る。**sink の中から `usage_ledger()` を呼んでも
  デッドロックしない**（ロック解放後に呼ぶことの固定）。
- serde 形: `kind` タグ・`cost_usd` の null・`LoggedUsageEvent` の flatten（`t_ms` と `kind` が同じ段）・
  本文欄の不在 — jsonl の読み手（Phase D の集計）が依存する形。
- Perplexity の decode が `cost_usd` を埋める（既存テストに assert）。

## 未決

1. （C）見積もりの表示精度 — 暫定を `formatUsd` に置いた（0 / `< $0.0001` / 4 桁 / 2 桁）。実プレイで額を見てから調整。

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
