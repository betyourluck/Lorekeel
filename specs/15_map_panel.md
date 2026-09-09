# 15. マップパネル — 現在地と行ける場所（右ペイン第4タブ）

Status: **rev2 実装済（2026-09-09。有向グラフ → リスト、ユーザー決定）。実プレイ盤面での目視が残。**
〔rev1: Phase A+B 実装済 2026-07-16、可視範囲=霧をユーザー確定〕
Scope: **app 提示層のみ** — 右ペインに「マップ」縦タブを追加し、**現在地と、そこから行ける
場所**を出す（rev1 は同じデータを有向グラフで SVG 描画していた）。**gm_core / harness / 正本 /
prompt は無改修**（マップは既存データからの派生表示）。

## 動機

プレイヤーが「どこへ行ける?」を GM に尋ねる往復（＝1ターン = フルプロンプト1往復）を減らす。
GM の prompt は **既に**出口を知っている（#42 で `state_brief` に「いま通れる出口」を動的 surface
済み）ので **prompt は不変** — 効くのは**プレイヤー側の探り行動の削減**という間接的トークン節約。
マップは「今どこにいて、どこへ行けて、その先どう繋がるか」を一目にする可視化。

## 北極星との整合

- **engine 無改修**: マップは `Scenario.exits`（有向グラフそのもの）+ `GameState`（現在地・gate
  評価）+ chronicle（`TurnLog.location` = 訪問済み）から導く**派生表示**。可変状態を増やさない
  （訪問済みは history 由来 = `GameState` に `visited` を足さない）。gate 評価は既存 `Gate::eval`。
- **ネタバレ制御 = 霧（fog of war）**: 探索の発見を殺さない（北極星「矛盾しない GM」の体験版）。
  全図/作者制御はスコープ外（将来、必要になれば `Location` に秘匿フラグを足す）。

## 可視範囲（霧）— ユーザー確定

- `visited` = `{start, 現在地}` ∪ `{history の各 TurnLog.location}`、**現 scenario に存在する
  ロケーションに限定**（campaign 遷移で前モジュールの location が history に残るのを除外）。
- `frontier` = `visited` の各ノードの `exits.to`（**1歩先。未訪問でも名前を出す**・中身は伏せる）。
- `nodes` = `visited` ∪ `frontier`。**奥（frontier からさらに先）は霧 = 出さない**。
- `edges` = `visited` ノードから出る `exits` のみ（frontier からの辺は描かない = その先は霧）。

## rev2 — 有向グラフ → リスト（2026-09-09、ユーザー決定）

**やめた**: 全体を俯瞰する有向グラフ（BFS ランク列 + SVG 矢印 + 掴んでパン + 丸クリックで詳細）。
**した**: **現在地を上に置き、その下に「ここから行ける場所」を縦に並べるリスト**。

理由 — ユーザーの言「マップにしてと言ったのは僕だったと思うが、やはりリストのほうが見やすい」。
起草時の動機（「どこへ行ける?」を GM に尋ねる往復を減らす）に照らすと、**その問いの答えは常に
現在地の隣接だけ**で、全体図は問いより広かった。狭い右ペインで読むには重く、行けるかどうかを
読み取るのに図を目で辿る必要があった。

- **backend は無改修**（`map_view` の霧の範囲・gate 評価・DTO の形は不変）。**ただし 1 点だけ
  実装を spec に合わせた** — 下の「可視範囲」は起草時から「未訪問でも名前を出す」と書いて
  あったのに、実装（同じコミット `1a41e5d`）は frontier の `title` を空にして frontend が
  「？」を描いていた。**伏せる判断は spec にも data_contract にも無く、コードのコメントにしか
  無かった**。ユーザーの要望（「行ったことのない場所も名前を表示していい、ただ色は薄くする」）
  はこの矛盾を spec の側で解いたことになる。**中身（`description` / `image`）は依然伏せる** —
  名前は道しるべ、説明はその場所の内容そのもので、ネタバレの重さが違う。
- **リストの導出は純粋関数** `mapList(view)`（`app/src/map.ts`）。現在地から出る辺だけを見て、
  **同じ行き先への複数の出口は 1 行に畳み、一本でも通れるなら通れる扱い**（別々の gate が付いた
  二つの道は、片方が開いていれば行ける）。順序は**作者が書いた `exits` の順**を保つ。
  DOM に依らないので vitest で固定できる（提示層で唯一テストできる部分）。
- **見た目**: 現在地は熾火の縦罫 + 名前 + 説明（画像があれば参照ストックへドラッグ可）。行は
  `→ 名前`、通れる道の矢印だけ熾火。未踏は名前を薄く、gate 未達は 🔒 + 矢印を落とす。行を
  クリックすると訪問済みは説明・画像、未踏は「まだ到達していません」。
- **文字色は Tailwind のユーティリティで書く** — scoped CSS に `rgb(var(--parchment) / 0.5)` と
  直書きすると 2026-09-08 の減光カーブ（textColor スケール）を通らず、**ライトテーマだけ薄い**
  という穴が一箇所ずつ生える。実装時に自分で 1 件作り、既存 3 件（TitleBar のアイコン等）も
  見つかったので `@apply` へ寄せ、`theme.test.ts` に再発を落とす網を張った。

## gate 表示

各辺の gate を backend が評価（`exit.gate.eval(&state)`）:
- `from == 現在地` かつ通れる → **「いま行ける」**（強調・実線矢印）。
- gate 未達 → **🔒（今は不可）**。行き先名は出す（1歩先の存在）が **gate 条件文は出さない**
  （「master_key が必要」等はネタバレ）。
- rev2 以降、リストは**現在地から出る辺だけ**を描くので「その他（現在地からでない繋がり）」は
  表示に現れない（DTO には従来どおり載る）。

## データ（data_contract 追記）

```
MapNode  { id, title, description, image, current: bool, visited: bool }
         # visited=false は frontier（未踏の1歩先）。**title は未踏でも入る**、
         # description / image は訪問済みだけ（中身は伏せる）
MapEdge  { from, to, locked: bool }                    # locked = gate 未達（🔒）
MapView  { nodes: [MapNode], edges: [MapEdge] }
```
`GameView.map` / `TurnView.map` に載せる（開幕 + 毎ターン。移動で現在地・可視範囲が変わる）。
却下ターンは state 不変ゆえマップも不変（現状スナップショットを返す）。

## レイアウト（frontend）

**rev2 = リスト**（上の節）。`mapList(view)` が「現在地 + そこから行ける場所」を導き、
`MapPanel.vue` は縦に並べるだけ。〔rev1 の記録: 現在地起点の BFS ランク列 + SVG 矢印 +
掴んでパン + 丸クリックで詳細。ノード数が数〜十数ゆえ簡易レイアウトで足りていたが、
**読みやすさの問題は密度でなく「問いより広い」ことだった**〕

## 実装（Phase）

- **Phase A — backend `map_view(scenario, state, history) -> MapView` + DTO + PoC**:
  霧の範囲（visited ∪ frontier）・gate ロック・campaign で他モジュール location を除外。
  PoC: `map_view_shows_visited_and_one_hop_frontier_hiding_the_rest` /
  `map_view_marks_locked_exits_and_current`。
- **Phase B — frontend `MapPanel.vue`**: 第4タブ「マップ」（Ctrl+4）+ `Icon(map)` +
  i18n（ja/en）+ types。`activeTab` 拡張、Ctrl+Tab 巡回に追加。
  〔rev2 = リストへ作り替え。`map.ts` の純関数 + vitest 5 本〕
- **Phase C — GUI 目視**: 現在地・🔒・未踏の薄さ・移動でのマップ更新・campaign 遷移で
  遷移先の隣接に差し替わる。

## スコープ外

- モジュール跨ぎの全体マップ（章をまたぐ地図）。
- 手動レイアウト（作者が座標指定）・立ち絵的な意匠。
- 作者の per-location 秘匿（可視範囲は霧に固定。全図/作者制御は将来 spec）。
