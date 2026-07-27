# 04. キャラクターの入退場制御 — presence を可変状態へ

Status: **Done（オーバーライド層モデル実装済・PoC green）** / 2026-06-28
Scope: キャラクターの「登場（入場）/退場」をトリガーで制御し、モジュール跨ぎで持ち越す。
顔アイコン行（presence）を authored 静的から可変状態へ昇格させる。

> **改訂 (2026-07-02, ユーザーFB)**: `Location.present` は**明示宣言**になった——空（未宣言含む）
> なら**誰もいない**。本 spec 起草時の「空なら全 characters」フォールバックは廃止（無人の場所を
> 作るのに全キャラを `set_presence false` する逆転が起きるため）。NPC を出す場所には `present` を
> 必ず書く。override による登場/退場は不変。PoC: `empty_present_means_nobody`。

## 北極星整合

- **authored 専権の第4例**: `SetPresence` は `grant_skill`/`set_attribute`/`record_turn` と同型。
  LLM が提案すると `adjudicate` が却下（キャラ勝手登場の捏造遮断＝閉世界）、trigger effects は
  `apply_ops` 直行で適用できる。
- **gm_core が正本**: presence は提示の派生でなく**可変状態**（`GameState.present_overrides`）になり、
  セーブ対象・`transition` で持ち越す。誰が画面に居るかの真実をエンジンが握る。

## 問題（現状の presence は静的）

`Location.present`（場所ごと authored 静的、空なら全 characters）は**動かせない**。
「仲間が加わる／離脱する」「次の画面に連れていく」を表せない。トリガーで制御し、
モジュール跨ぎで保持したい（仲間がキャンペーンを通して同行する）。

## モデル（合意: オーバーライド層）

`Location.present`（authored の場所ベース）を**残したまま**、可変の override 層を乗せる:

- **`GameState.present_overrides: BTreeMap<EntityId, bool>`**（新・第5の可変状態、セーブ対象）。
  `entity → true`（強制登場）/ `entity → false`（強制退場）。
- **実効 presence** = 場所ベース ± override:
  ```
  base = Location.present が非空ならそれ、空なら全 characters
  for (entity, present) in present_overrides:
      present なら base に追加、!present なら base から除去
  既知 characters に retain（このモジュールが解決できる NPC だけ）
  ```
- **`StateOp::SetPresence { entity, present: bool }`**（authored 専権）→ override に書く。
  LLM 提案は `RejectReason::PresenceSetNotAllowed { entity }` で却下。
- **`transition` が `present_overrides` を持ち越す**（「次の画面でセットしたいキャラをグローバルに保持」）。
- **`Scenario::present_at(&self, state) -> BTreeSet<EntityId>`**（純粋）= 実効 NPC presence。
  app の `present_characters` がこれを使い、主人公を先頭に名前/アイコンを解決する。

### 跨ぎ注入の依存（設計上の制約）

override は bool を運ぶだけ。あるモジュールで override が意味を持つには、その**キャラがそのモジュールの
`characters`（cast 注入）に居る**必要がある（名前/アイコンを解決するため）。未注入のキャラへの
force-present は `present_at` の retain で**そのモジュールでは黙ってスキップ**され、override 自体は
state に残るので**そのキャラを持つ次のモジュールで現れる**（同行者は各モジュールの cast に宣言する）。

## データモデル（gm_core 追加）

```
GameState.present_overrides: BTreeMap<EntityId, bool>   # 可変・セーブ・transition 持ち越し
StateOp::SetPresence { entity, present: bool }          # authored 専権 op
RejectReason::PresenceSetNotAllowed { entity }          # LLM 提案の却下理由
Scenario::present_at(&self, &GameState) -> BTreeSet<EntityId>  # 純粋: 実効 NPC presence
```

- **validate なし**（override は跨ぎ roster ゆえ entity の load 時閉世界検査はしない。
  authored-only の adjudicate 却下が主たる guard。未知 entity は `present_at` の retain で無害化）。

## app / 提示

- `present_characters` を `scenario.present_at(state)` 経由に置換（主人公先頭 + 名前/アイコン解決は不変）。
- frontend は既存の顔アイコン行をそのまま使う（実効 presence が backend で解決済み）。

## PoC（Red→Green）

- gm_core:
  - `presence_override_via_trigger_changes_present_at`: トリガーの `set_presence` で entity が
    登場/退場し、`present_at` が反映する（base ± override）。
  - `llm_proposed_set_presence_is_rejected`: LLM 提案の `SetPresence` が `PresenceSetNotAllowed` で却下。
  - `transition_carries_present_overrides`: 退場/登場が次モジュールへ持ち越される。

## スコープ外

- 立ち絵・差分表情（presence は顔アイコン行の在/不在のみ）。
- ~~presence の per-location override（現状は override がモジュール内全 location に効く）。~~
  → **揮発 override として 2026-07-25 に解消**（下記追補）。恒久的な per-location override
  （フラグが立ってからその場所に住み着く）は依然スコープ外。静的な常駐は `Location.present` で書く。

---

## 追補: 揮発 presence — 来訪者と同行者（2026-07-25）

上の「スコープ外」に置いた per-location override が、実プレイで請求書になって戻ってきた。
ユーザーの言葉では「`set_presence` にさせちゃうとずっと着いてきてしまう。その瞬間だけその場に
いさせたいときとか、またわざわざセットし直さないといけない。それは面倒だ」。

### 真因は層の非対称

`Location.present` は**場所ごとの** authored 宣言なのに、それを上書きする `present_overrides` は
`EntityId → bool` で**場所を持たなかった**。土台が場所で索かれ、上書き層が場所を持たなければ、
上書きは必然的にモジュール全域に効く。つまりこの層は最初から「同行者」専用で、「来訪者」を
表現する語彙が存在しなかった。機能が無いのではなく、**回避策で書けるが面倒**という形の負債
（退場トリガーを来訪者ごとに 1 本、書き忘れれば永久同行）だったため、要望として上がりにくかった。

### 機構

`StateOp::SetPresence` に `volatile: bool`（serde default false ＝既存 YAML 無改修・挙動不変）を
足し、`present_overrides` の値を untagged 両受けの `PresenceOverride` へ:

| | 値 | 意味 | 消え方 |
|---|---|---|---|
| 同行者 | `Persistent(bool)`（旧形式 `bool` がこれとして読める） | 場所を持たない | 明示的に `set_presence`。`transition` で持ち越す |
| 来訪者 | `Volatile { present, at: LocationId }` | 立てた場所でだけ効く | その場所を離れた瞬間に**破棄**。`transition` では捨てる |

- `present_at` は揮発分を `at == state.location` のときだけ重ねる。破棄後の実効 presence は
  `Location.present` の土台へ戻る（**再訪しても蘇らない**＝「その訪問限り」）。
- 破棄は `apply_deterministic_op` の `Move` 適用点に置いた。ここは spec 09 の逐次射影と実適用が
  **共有する**ので、裁定のドライランと実行が構造的に一致する。
- `present: false` 側も揮発にできる（「この訪問の間だけ席を外している」）。

### 期限を「場所を離れたら」にした理由

- **「そのターンだけ」**は使えない。トリガーは GM の語りの**後**に発火するので、来訪者が
  一言も喋らないうちに消える。
- **「N ターン」**は engine に置く価値が薄い。`record_turn` + `Gate::TurnsSince` で今日書ける。
- 「場所を離れたら」だけが、engine 側にしか置けず、かつ作者の意図（この場面限り）と一致する。

### 実装中に見つかった分岐

`ResolveVote` の死亡投影を揮発で書くと、**場所を変えた瞬間に処刑された者が蘇る**。永続で書く。
新しい様式を足したときは、既存の呼び出し元がどちらの様式であるべきかを全部数えること。

### PoC

- `volatile_presence_expires_on_leaving_and_falls_back_to_location_set`
  — 同一テスト内に `volatile: false` の対照群を置き、移動後に来訪者だけが消えて同行者が残ることを
  弁別する。揮発の「不在」も検証。
- `old_saves_parse_bool_presence_overrides_as_persistent`
  — 旧セーブの `{ alice: true }` が untagged で `Persistent` として読める
  （`LocationItem` / `StatInit` と同じ後方互換パターン、これで三例目）。

既存 320 本は**回帰ゼロ**。harness / app / frontend は `present_at` を呼ぶだけなので無改修。

---

## 追補: 内部 stat の秘匿（hidden_stats、2026-06-28）

presence とは別件だが同セッションで発覚した提示の穴。`record_turn` のタイマー stat（`x_turn` 等）は
`GameState.entities` に他の数値と同居するので、一度刻むと主人公の可視ステータス（UI の状態パネル /
prompt の `state_brief` / CLI）に露出してしまう（内部帳簿が表に出る）。

**解**: `Scenario.hidden_stats: BTreeSet<StatKey>`（表示から隠す stat キーの宣言）。**engine 非使用・
非検証の提示ヒント**（`flag_hints`/`world` と同類）。提示層の 3 経路（app `state_view` / prompt
`state_brief` / CLI `stats_line`）がこのキーの stat を skip する。engine ロジックは無改修
（stat は `entities` のまま正常に動き、表示だけ隠れる）。タイマーも repeatable カウンタも一括で隠せる。

- 補足: `x_turn` 等は `initial_stats` に**宣言不要**（`record_turn` がトリガー効果で生成、`turns_since` は
  未記録なら 0 を読む）。`hidden_stats` は宣言の有無に関わらず、その**キーの stat の表示**を隠す。
- PoC: `state_brief_hides_hidden_stats`（harness）。
