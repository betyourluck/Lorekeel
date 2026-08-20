# spec 25: 設定画集 — 置いた絵の見た目で挿絵を描く

**Status**: Done — **v0.5.13 で配布済み (2026-08-20)**（同日起草 → ユーザー査読 4 点 + 細部 3 点を反映 rev2 →
未決 2/3 決着 → Phase 0〜C 同日実装。Phase C: Gemini / OpenAI edits とも参照 1 枚で見た目が引き継がれた
(`data_contract` `settings_sheets` 末尾)。ComfyUI + GUI は同日ユーザー実機で Green — 設定画集でキャラ 5 人の顔が一貫。
残 = outcast 側の書庫審査 (webp/4MB/3 枚) の実装のみ。commits d26be5f (P0) / 6caa935 (A) / e9ede98 (B) / 506e16d (C)。
rev1 の「entity ごとの参照ファイル + プレイヤー側フォルダ + 作者フィールド」は**撤回**し「設定画集 3 枚まで」に単純化）

## 動機

spec 24 の挿絵は、人物の容姿を `profile` の文章からしか知らない。同じメイドが毎回違う顔で描かれる。
**設定画集を置く**だけで、以後の挿絵がその見た目に寄るようにしたい。先行（alslime）はキャラ設定に
LoRA を持たせる方式だが ComfyUI 限定で敷居が高い。ここでは**参照画像（image-to-image の参照）**という
3 プロバイダが共通に持つ最小の口を使う。

## 何を作るか（rev2）

1. **設定画集（settings sheets）は最大 3 枚。** entity ごとにファイルを用意する必要は無い —
   **1 枚に複数人の立ち絵を並べて名前を書き込む**、背景は分割して 1 枚にまとめる、という
   「設定資料集」の形で作る。3 枚の中に設定したいものを埋め込めばよい。
2. **置き場は `images/settings_sheets/`**（パッケージの `images/` 配下の固定サブフォルダ）。
   `*.png|jpg|jpeg|webp`（**拡張子は大文字小文字を区別しない** — `.PNG` も効く）。ファイル名順で
   先頭 `MAX_REFS` 枚。**無ければ添付なし**（spec 24 と byte 不変）。作者が同梱しても、プレイヤーが
   「フォルダを開く」で自分のパッケージ複製に置いても、同じ経路で効く。
3. **誰の参照かを Kataribe は知らなくてよい。** 名前は絵の中に書かれているので、プロンプト書きには
   「参照画像は設定画集（人物の立ち絵に名前が書かれている・背景がまとめられている）。登場人物や場所の
   見た目はそれに従い、`(see reference sheets)` のように参照を指せ」と**一般規律**で接地する。
   entity 照合・優先順・プレイヤー override の機構は**作らない**。
4. **プロバイダ別の運び方**（encode 純関数）:
   - **Gemini**: `parts = [inlineData ×N, text]`（画像を前置）。
   - **OpenAI**: 参照あり → `POST /v1/images/edits`（multipart）。部品列は
     **`image[]`（N 回）, `prompt`, `model`, `size`, `quality`, `n`**（キー名は OpenAI 公式 curl の
     `-F "image[]=@..."` に合わせる。400 が param 名を名指ししたら `image` へ落とす）。
     `input_fidelity` は **送らない**（2026-08-21 実測で決着 → 「実測」節。既定モデル
     `gpt-image-1-mini` では `high` が 400）。参照なし →
     従来 `/images/generations` の JSON（spec 24 と byte 不変）。
   - **ComfyUI**: 各参照を `POST /upload/image`（multipart `image`、`overwrite=true`）→ 返る `name` を
     `%ref_1%`〜`%ref_3%` に置換。**未使用のプレースホルダは置換しない**（空文字で埋めると
     `LoadImage.inputs.image = ""` になり `File not found` で落ちる）。参照が足りない LoadImage ノードの
     無効化は**ワークフロー側の責務**（同梱の汎用ワークフローは参照を持たないので `%ref_%` 自体が無い）。
5. **設定タブの可視化**: 「今の盤面で見つかった設定画集」（ファイル名・サイズ・スキップ理由）を
   一覧で出す。置いたのに効かないとき「見つかっていない / 大きすぎる」が見える（黙って落ちる失敗を
   見せる。`present_at` の typo の反省と同じ）。
6. **画像の扱い**: そのまま送る（リサイズは依存が増えるので v1 はしない）。**4MB 超はスキップ**。
   mime は拡張子から。

## 外部接地（2026-08-20。実装時に再確認）

- **Gemini**: `generateContent` の `contents[0].parts[]` に `inlineData{mimeType, data}` を並べる。公式:
  `gemini-2.5-flash-image` は**参照 3 枚まで**（キャラ一貫性の用途を明記）。後継（Nano Banana 2 / Pro）は
  上限が違いうる → `MAX_REFS` をプロバイダ別定数に。
- **OpenAI（gpt-image-1）**: `/v1/images/edits` multipart。複数画像可（公式 curl は `image[]`）。
  公式ページは取得時 403 → **枚数・サイズ上限・`input_fidelity` の有無は実装時に確認**。
  → 2026-08-21 に実測（「実測」節）: `input_fidelity` は**モデルによって存在しない**。
- **ComfyUI**: `/upload/image` → `LoadImage.inputs.image`。IPAdapter / InstantID 等はユーザーのワークフロー。

## 決定事項（rev2）

1. **設定画集 3 枚まで・`images/settings_sheets/` 固定・ファイル名順。** entity 照合なし・プレイヤー
   side フォルダなし・作者フィールドなし（rev1 を撤回 = **engine 無改修**、harness にも scenario
   フィールドは増えない）。
2. **`MAX_REFS` はプロバイダ別定数**（v1 は openai 3 / gemini 3 / comfy 3。後継モデルで差が出たら
   定数を割る）。
3. **参照の有無で API の口が変わるのは OpenAI だけ**（generations ↔ edits）。decode は同じ。
4. **責務分離**: `pick_settings_sheets(entries: &[(name, size)], max) -> (Vec<name>, Vec<Skipped{name,
   reason: Oversize | Unsupported}>)` は**純関数**（ディレクトリ読みは薄い IO 包み）。トーストは
   呼び出し側（command / 設定タブ）が `Skipped` を見て出す。
5. **byte 不変の保証**: `build_image_prompt_request` は参照の有無を `refs: usize`（枚数）で受け、0 なら
   文言が spec 24 と **byte 一致**（PoC で固定）。
6. **secret/hidden は不変**: 設定画集は見た目であり秘密ではない（同梱した時点で作者が見せる意思）。
7. **揮発・保存・トグルは spec 24 のまま**。参照は生成入力であり、出力の扱いは変わらない。
8. **プロンプト書きへの規律**（参照があるときだけ system に足す）: 「参照画像 N 枚は設定画集。人物の
   立ち絵に名前が書かれ、背景がまとめられている。登場人物と場所の見た目はそれに従い、プロンプト内で
   `(as in the reference sheets)` と指せ。参照に無い人物は profile の範囲で」。

## スコープ外（v1）

- リサイズ・切り抜き（image crate を足さない）/ LoRA の自動適用 / UI からの登録（フォルダに置く
  運用で足りる）/ entity ごとの参照（設定画集が代替）/ ComfyUI の参照つき汎用ワークフロー同梱 /
  `input_fidelity`（2026-08-21 実測で「送らない」を恒久化。opt-in 化は別途）。

## PoC

- `pick_settings_sheets`: 拡張子 4 種（大文字小文字不問）・ファイル名順・上限 `MAX_REFS`・4MB 超は
  `Skipped{Oversize}`・未対応拡張子は `Skipped{Unsupported}`・空なら空。
- Gemini encode: parts が `[inlineData, inlineData, text]` の順、0 枚なら spec 24 と byte 一致。
- OpenAI: 参照ありで edits の multipart 部品列（`image[]` ×N・`prompt`・`model`・`size`・`quality`・`n`、
  `input_fidelity` は**無い**）、参照なしで generations の JSON（spec 24 と byte 一致）。
- ComfyUI: `%ref_1%..3` の置換、**未使用は残る**（置換されない）、参照なしは spec 24 と同一出力。
- プロンプト書き: 参照ありで規律が system に出る・0 枚で byte 一致。

## 未決 → 決着（2026-08-20 ユーザー決定）

1. OpenAI `input_fidelity: high` を既定にするか — **しない**（2026-08-21 実測で決着 → 「実測」節）。
2. **書庫の審査対象にする**（outcast 側）。ただし中身は問わない — `images/settings_sheets/` 配下は
   **webp のみ・1 枚 4MB 以下・3 枚まで** を受入検査に足す（配布サイズの作法 = WebP 推奨の延長）。
3. **`package_spec.md` には書かない。**

## 実測 — `input_fidelity`（2026-08-21、未決 1 の決着）

参照 1 枚（実プレイ出力の 5 人ステージ絵 673KB jpg = 顔と衣装が 5 通り並ぶ「設定画集」相当）に
同一プロンプト（「同じ 5 人が楽屋のソファに座る」）・同一 `size=1536x1024` で、`input_fidelity`
だけを振った。

| モデル | 指定 | HTTP | 画像入力 tok | 出力 tok | 所要 |
|---|---|---|---|---|---|
| `gpt-image-1-mini`（**Kataribe の既定**） | 無指定 | 200 | 1032 | 400 | 33.8s |
| `gpt-image-1-mini` | `low` | 200 | **1032（無指定と完全一致）** | 400 | 13.7s |
| `gpt-image-1-mini` | `high` | **400** | — | — | 0.6s |
| `gpt-image-1` | 無指定 | 200 | 323 | 400 | 21.1s |
| `gpt-image-1` | `high` | 200 | **6531（20.2×）** | 400 | 23.1s |
| `gpt-image-1` `quality=medium` | 無指定 | 200 | 323 | 1568 | 28.9s |
| `gpt-image-1` `quality=medium` | `high` | 200 | 6531 | 1568 | 34.3s |

400 の本文は param を名指しする:
`{"message":"input_fidelity 'high' is not supported for gpt-image-1-mini.","param":"input_fidelity","code":"invalid_input_fidelity_model"}`

**決着: 既定にしない（送らない）。** 根拠 3 つ:

1. **既定モデルで 400。** 2026-08-20 に既定を `gpt-image-1-mini` へ倒した以上、`high` を既定にすると
   **箱から出してすぐ全滅**する。これは #76 / #77 と同じ「モデルによって欄の有無が違う」族で、
   一律送出が成り立たない典型。
2. **画像入力トークン 20.2×。** `quality=low` の 1 枚で $0.019 → $0.081（約 4.2×）、`medium` で
   $0.066 → $0.128（約 1.9×）。挿絵は手動・都度なので破滅的ではないが、既定に据える額ではない
   （$10/1M image input・$40/1M image output、2026-07 時点の公式）。
3. **`quality=low` では逆効果だった。** low + `high` は衣装の細部（茶のブーツ・手袋・金の縁取り）を
   残す代わりに**顔が崩れ砂目が出る**。`medium` で撮り直すと崩れは消え、`high` 側だけが参照の衣装
   細部と髪色（ワインレッド）を保った ⇒ **粗さは fidelity でなく出力品質側の artifact**。
   Kataribe の既定は `Detail::Standard = low` なので、既定の組み合わせでは害の方が出る。

**効くのは「上位モデル + `medium` 以上 + キャラ一貫性が主目的」の三点が揃ったときだけ。**
opt-in（設定の 1 チェック + 400 で自動降格して latch = `tool_mode_downgrade` と同型）にする道は
残っているが、v1 は送らないまま据え置く。

**接地の限界**: トークン数・400・`low` == 無指定 は wire の決定論的事実。**見た目の優劣は各アーム
n=1 で判定者は Neo**（Images API に seed が無いので、2 枚の差にはサンプリングの運も混じる）。
衣装細部の差は公式の説明する機序（`high` は入力の質感を richer に保つ・**複数枚では 1 枚目だけ**が
その恩恵を受ける）と整合するが、対照実験としては弱い。opt-in を判断するならサンプルを増やす。

## rev2 査読の反映記録（2026-08-20）

| # | 指摘 | 反映 |
|---|---|---|
| 1 | entity ごとの参照ファイルは不要。設定画集 3 枚を `images/settings_sheets/` に | 何を作るか 1〜3・決定 1（rev1 の照合/優先順/作者フィールドを撤回 = engine 無改修） |
| 2 | ComfyUI の未使用プレースホルダ空埋めは `File not found` を生む | 何を作るか 4: 未使用は置換しない・LoadImage の無効化はワークフロー側 |
| 3 | OpenAI の部品列が 3 箇所でズレ・`input_fidelity` 不在 | 部品列を `image[]` ×N, prompt, model, size, quality, n に統一・`input_fidelity` は v1 不送出（未決 1 と紐付け） |
| 4 | 4MB スキップの純関数とトーストの責務 | 決定 4: 純関数は `Skipped` を返すだけ・トーストは呼び出し側 |
| 細 1 | 上限 3 のハードコード | 決定 2: `MAX_REFS` プロバイダ別定数 |
| 細 2 | byte 不変の保証 | 決定 5: `refs: usize`、0 で byte 一致を PoC |
| 細 3 | 大文字拡張子 | 何を作るか 2: 大文字小文字不問 |
