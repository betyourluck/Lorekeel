# spec 33 — 直前の語りを「可変 user の逐語」から「会話履歴」へ

**Status**: Draft (2026-09-23 起票)。**P0 の測定が済むまで実装しない。**

---

## 動機 — 同じ情報を 2 箇所に持っている

GM のターンループは**毎ターン messages を新規構築する**（`state` が唯一の真実 = 北極星）。
その帰結として **LLM は自分の直前の語りを記憶しない**（failures #27）ので、2026-06-24 に
`recent_narration` を、2026-09-13 に `recent_narrations`（直前 K ターン）を**可変 user の中へ
逐語で埋める**形で補った。

つまり今の形は「**会話履歴を、会話履歴の場所ではなく本文の中に書き写している**」。
本 spec はこれを**本来の場所（assistant メッセージ）へ戻す**。

副産物としてキャッシュが効く。**ただし順序は逆で、キャッシュが動機ではない** — 動機は重複の解消で、
キャッシュは構造を正した結果として付いてくる。この順序を取り違えると、金額が小さいことを理由に
筋の悪い最適化を入れることになる（下の「接地の限界」を見よ）。

---

## 実測 (2026-09-23)

### `muse-spark` のキャッシュが 89% 読み損ねている

Lorekeel `app_data/logs/usage.jsonl` の GM リクエスト 58 件（2026-09-21〜22）:

- **`cache_read` が 49/58 で 113 トークン**。たまに **7,793**（6 回）/ 9,713 / 10,225 / 0
- **7,793 が Lorekeel の静的 system のキャッシュ量** — 読めるときは読めている。ヒット率 **16%**
- 間隔は無関係（9 秒でも miss・399 秒でもヒット）

### 真因は history の不在 (Fuseforks のログ 4,813 行が決めた)

同じモデル `muse-spark-1.3-contributor` を使う Fuseforks (`jp.outcasts.fuseforks/workspace/fuseforks.log`、
2026-08-18〜09-23) の `cache:` 行を **round=1 だけに絞って** history 件数で割ると:

| history 件数 | `cached>1000` の割合 |
|---|---|
| **0 件** | **8%** |
| 1–4 件 | **63%** |
| 5–9 件 | 64% |
| 10+ 件 | 58% |

**history がゼロだと 8%、1 件でもあれば 63%。** Lorekeel の 16% は hist=0 の帯にいる。

**棄却した仮説 2 つ**:

- **床（最小プレフィックス）説** — prompt サイズ帯で相関が出ない（<8k 33% / 8–12k 58% /
  12–20k 41% / 20–30k 34% / ≥30k 66%）。failures #80 の Perplexity 8,192 と同型かと疑ったが違う
- **間隔（TTL）説** — 全 round では強く相関する（<30s 94% / >1h 4%）が、**history と交絡している**
  （連続ラウンドは間隔が短く、かつ history が長い）。round=1 に絞ると history の差が支配的

### 同じログで他プロバイダ

| model | n | hit 率 | `cached=113` |
|---|---|---|---|
| `gpt-5.6-terra` | 872 | **90.0%** | 0 |
| `claude-sonnet-5` | 730 | 89.3% | 0 |
| `muse-spark-1.3` | 697 | 77.1% | **15%** |
| `gemini-3.5-flash-lite` | 596 | 39.0% | 0 |

**`cached=113` は Meta だけに出る固定値**（Grok の 128 = miss のフロア表示 / failures #57 と同型）。
OpenAI・Anthropic の口は決定論的に効くので、**本 spec が効くのは Meta 系と、多段 breakpoint の
余地が残る Anthropic**。

---

## 設計

### D1. `recent_narrations` を assistant メッセージへ移す

```text
現在: [system(静的), system(synopsis?), user( state_brief + facts + … + 直前 K ターンの語り(逐語) + 行動文 )]
本案: [system(静的), system(synopsis?), user(T-3 行動), assistant(T-3 語り), … , user(可変 + 行動文)]
```

- **積むのは受理ターンだけ**（現在の `push_recent_narration` と同じ。却下された語りを積むのは
  failures #47 の再演になる）
- **K と字数予算は現状の機構をそのまま使う**（`LOREKEEL_RECENT_TURNS` / `RECENT_NARRATIONS_BUDGET`
  8,000 字）。**上限を外さない** — プレフィックスが単調増加すると、キャッシュが効いても
  cache_read 単価ぶんは増え続ける
- **ビート・判定結末の継ぎ足し**（`extend_last_narration`）は assistant 本文の末尾へそのまま乗る

### D2. 規律文は可変 user に残す

`recent_narrations_note` が持っている 2 つの規律 —「既に確立した静的な情景を再び描写しないこと」
「ここに書かれた細部は確定した出来事として扱い、食い違う語りをしないこと」— は**本文でなく指示**なので、
assistant メッセージには入れられない。**可変 user 側に 1 ブロックとして残す**（語りの逐語だけが移動する）。

### D3. Anthropic — breakpoint を 3 本目へ (cap 4 のうち 2 本使用中)

`anthropic.rs` は **leading system メッセージ毎に `cache_control`（先頭から 4 個まで）** を置く
（spec 14 D2）。現在の使用は 2 本（静的 / synopsis）。history は leading system ではないので、
**messages 配列の中の「最後の assistant ブロック」に 3 本目の `cache_control` を置く**。

- 4 本目は**空けておく**（将来の段のため。spec 14 が cap を使い切らない設計にしている）
- 章追加ターンは synopsis 段が失効するが静的段は生存する、という spec 14 の性質は不変

### D4. Gemini — 1 本目のみ pin は不変 (spec 13/14 D4 の凍結を壊さない)

`gemini.rs` の `fingerprint` は**1 本目の leading system だけ**を見る（2 本目以降は inline 降格）。
history は contents に inline で入るだけで、**明示 cachedContent には入れない**。
入れると毎ターン再 pin の churn になり、spec 13 Phase D で確認した「章圧縮後も再 pin なし」が壊れる。

### D5. OpenAI / Grok / Meta — 自動延伸なので改修ゼロ

並べ替えの副産物としてプレフィックスが伸びる。**本 spec の効き目の本体はここ**。

### D6. 順序が behavior を変えうる — 対照を取ってから凍結する

spec 14 の未決 2（synopsis を user から system へ移した）では、**位置と role の変更が
Grok の「過去章の過強調」を生み**、`synopsis_note` に salience 規律を足して決着した（failures #60）。

本案は **state_brief より前に語りが来る**という、それより大きい並べ替えなので、**同じ検証が要る**:

- 直前の語りの細部が保たれるか（継続性 = そもそもの目的）
- **既出の情景を繰り返さないか**（D2 の規律が本文から離れても効くか — ここが一番危ない）
- 過去ターンの語りを過剰に引きずらないか（#60 と同型）

---

## Phase

### P0 — 測ってから凍結する (**実装の前・実プレイ不要**)

既存のセーブ（`app_data/saves/*.yaml` に turn 319 / 341 の実データがある）から
**現行形と本案形の messages を両方組み、実 API へ各 2 回ずつ投げて `cache_read` を比べる**。

- 測るのは 3 プロバイダ（Meta `muse-spark` / Anthropic `opus` / もう 1 つ）
- **2 回投げるのが要点** — 1 回目は書き込み、2 回目で読めるかを見る
- 期待: Meta で 113 → 7,793+ / Anthropic で 3 段目が乗る
- **効かなければここで畳む**（failures #106 と同じ規律 — コードを書く前に測る）

### P1 — 実装

`turn.rs` の messages 構築 / `prompt::recent_narrations_note` の分割（規律だけ残す）/
`anthropic.rs` の 3 本目 breakpoint。PoC は **K=0 のとき現行と byte 一致**を固定する
（既存 content の挙動を変えない = spec 14 Phase A と同じ流儀）。

### P2 — 対照 (D6)

実プレイで継続性・繰り返し・過強調の 3 点。**spec 32 の `consistency.jsonl` が計器になる**
（`past_contradiction` 軸が動けば継続性の破れが観測できる）。

### P3 — 台帳

CLAUDE.md / data_contract `prompt.recent_narrations` / spec 14 の「メッセージ構成の凍結」を改訂。
**spec 14 は凍結を明文化した spec なので、改訂の記録をあちらにも残す**（mandate: 撤去したら grep）。

---

## 接地の限界 (**起票時点で正直に**)

- **金額インパクトは小さい可能性が高い。** muse-spark は 0.2 円/リクエストなので、入力課金が 6 割
  減っても実ログ 2 日分で **約 4.6 円**。opus は Anthropic ネイティブで既に 17,954/30,000 = 60%
  効いており、**3 段目がどれだけ足せるかは未測定**（P0 で測る）
- **本 spec の主な価値は金額ではなく構造**（同じ情報を 2 箇所に持たない）。金額だけで判断するなら
  `LOREKEEL_RECENT_TURNS` を 8→3 にするほうが 3.7 円/ターン・コード変更ゼロで効く（failures #106）
- Fuseforks のログは**他プロジェクトの運用データ**で、ワークロードが違う（ツールを回す
  エージェント 対 1 ターン 1 往復の GM）。history の効果という**機構の性質**は移ると見ているが、
  数字そのものは移らない
- **`cached=113` が何を意味するかは推定**（Grok の 128 と同型の「miss のフロア表示」と読んでいるが、
  Meta の公式説明で確かめていない）
- D6 の behavior 変化は**起きるかどうかも未知**。spec 14 未決 2 の前例があるので検証を Phase に
  入れたが、前例が当たる保証はない
