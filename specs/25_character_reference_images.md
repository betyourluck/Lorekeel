# spec 25: キャラクター参照画像 — 置いた絵の顔で挿絵を描く

**Status**: Draft rev1（2026-08-20 起草。ユーザー要望「キャラクターの設定画像のフォルダを作り、そこに
画像を入れておくとそのキャラクターが反映される」。spec 24 が v2 に外した「キャラ一貫性」の実体。
査読待ち）

## 動機

spec 24 の挿絵は、人物の容姿を `profile` の文章からしか知らない。同じメイドが毎回違う顔で描かれる。
プレイヤーが（あるいは作者が）**そのキャラの絵を 1 枚置く**だけで、以後の挿絵がその顔で描かれる
ようにしたい。先行（alslime）はキャラ設定に LoRA を持たせる方式だが、ComfyUI 限定で敷居が高い。
ここでは**参照画像（image-to-image の参照）**という、3 プロバイダが共通に持つ最小の口を使う。

## 外部接地（2026-08-20。実装時に再確認）

- **Gemini（Nano Banana）**: 同じ `generateContent` の `contents[0].parts[]` に `inlineData{mimeType,
  data}` を**テキストの前に並べる**だけ。公式: `gemini-2.5-flash-image` は**参照 3 枚まで**
  （キャラ一貫性の用途を明記）。後継（Nano Banana 2 / Pro）は上限が違いうる。
- **OpenAI（gpt-image-1）**: `POST {base}/v1/images/edits`（**multipart/form-data**）に `image[]`
  （複数可）+ `prompt` + `model` + `size` + `quality` + `n`。参照が無いときは従来の
  `/images/generations`。`input_fidelity: high` で顔の保持が上がる（gpt-image-1 の追加パラメータ、
  要再接地）。公式ページは取得時 403 だったので **枚数・サイズ上限は実装時に確認**。
- **ComfyUI**: `POST {base}/upload/image`（multipart `image`、`overwrite=true`）→ 返る `name` を
  ワークフローの `LoadImage.inputs.image` に差す。IPAdapter / InstantID 等の参照ノードは
  **ユーザーのワークフローの責務**（Kataribe はファイル名を差すだけ）。

## 何を作るか

1. **参照の供給源は 2 つ**（プレイヤー側が勝つ）:
   - **プレイヤー**: 設定「画像生成」の**参照画像フォルダ**（既定 `app_data/character_refs/`、
     「フォルダを開く」あり）。`<entity_id>.(png|jpg|jpeg|webp)` を置くと効く。主人公は `player`。
     パッケージ別に分けたいときは `<フォルダ>/<パッケージフォルダ名>/<entity_id>.png`（**こちらを先に
     探す**）。**これがユーザー要望の「フォルダに入れておくと反映される」の実体。**
   - **作者**: `CharacterDef.reference_image: Option<String>`（`images/` のアセット ID、`icon` と同じ
     不透明 ID・`resolve_asset` で解決）+ `Protagonist.reference_image`。配布サイズの作法（WebP）に従う。
     `icon`（顔アイコン）はフォールバックに**使わない**（小さい / SVG が多い / 用途が違う）。
2. **誰の参照を送るか**: spec 24 のプロンプト素材と同じ **presence**（主人公先頭・NPC は id 順）から、
   参照を持つ者を先頭から **最大 3 枚**（Gemini 2.5 の上限・コスト）。居ない人物の参照は送らない。
3. **プロンプト書きへの接地**: 素材に「参照画像: 1=メイド、2=執事」と**番号つき**で渡し、規律に
   「参照画像の人物は `(reference image N)` のように番号で指す・容姿と服装は参照に従う・参照の無い
   人物は profile の範囲で」を足す。参照が無ければ従来どおり（byte 不変）。
4. **プロバイダ別の運び方**（encode 純関数）:
   - Gemini: `parts = [inlineData ×N, text]`。
   - OpenAI: 参照あり → `/images/edits` multipart（`image[]` ×N）。無し → 従来 `/generations`。
   - ComfyUI: 各参照を `/upload/image` → 返名を `%ref_1%`〜`%ref_3%` に置換（**未使用の
     プレースホルダは空文字で埋める**＝LoadImage を持たないワークフローでも壊れない）。同梱の
     汎用ワークフローは参照なしのまま（参照つきは IPAdapter 版を別途同梱するか v2）。
5. **設定タブの可視化**: 参照フォルダ欄 + 「フォルダを開く」+ **今の盤面で見つかった参照の一覧**
   （entity id → ファイル名・出所 player|author）。置いたのに効かないときに「見つかっていない」が
   分かる（`present_at` の typo が黙って落ちる 2026-07 の反省と同じ = 見えない失敗を見せる）。
6. **画像の扱い**: そのまま送る（リサイズは依存が増えるので v1 はしない）。**4MB 超はスキップして
   トースト**（置いたファイルが大きすぎる、と言う）。mime は拡張子から。

## 決定事項（rev1 の提案）

1. **プレイヤーの参照がパッケージの参照に勝つ。** 置き換える自由はプレイヤーのもの（挿絵は鑑賞物）。
2. **枚数上限 3・順序は presence 順**（主人公が参照を持てば常に 1 枚目）。4 人目以降は文章だけ。
3. **参照の有無で API の口が変わるのは OpenAI だけ**（generations ↔ edits）。encode は `refs.is_empty()`
   で分岐し、decode は同じ（`data[0].b64_json`）。
4. **engine は `reference_image` 1 フィールド ×2（CharacterDef / Protagonist）以外無改修**。
   不透明 ID・非検証（`icon` と同列）。harness は `resolve_reference_images(scenario, state, folders)`
   の純関数（優先順・拡張子・上限）+ `build_image_prompt_request` に `refs: Vec<(index, name)>`。
5. **secret/hidden は不変**: 参照画像は「見た目」であり秘密ではない。hidden 属性のキャラでも参照は送る
   （顔は公開情報）。
6. **揮発・保存・トグルは spec 24 のまま**。参照は生成入力であり、出力の扱いは変わらない。

## スコープ外（v1）

- リサイズ・切り抜き（image crate を足さない）/ LoRA の自動適用 / 参照の UI からのドラッグ登録
  （フォルダに置く運用で足りる）/ 1 キャラ複数参照（表情差分）/ ComfyUI の参照つき汎用ワークフロー同梱。

## PoC

- `resolve_reference_images`: パッケージ別サブフォルダ > フラット > 作者の順、拡張子 4 種、presence に
  居ない者は無視、上限 3、4MB 超はスキップ（戻り値に理由）。
- Gemini encode: parts が `[inlineData, inlineData, text]` の順。
- OpenAI: 参照ありで edits の multipart 部品列（`image[]` が N 個・`prompt`・`model`・`size`・`quality`）、
  参照なしで generations の JSON（spec 24 と byte 不変）。
- ComfyUI: `%ref_1%..3` の置換と未使用の空埋め。
- プロンプト書き: 参照番号が user に出る・規律が system に出る・参照なしは byte 不変。

## 未決

1. 主人公の参照ファイル名は `player` 固定でよいか（`Protagonist.name` でも探すか）。私は `player` 固定
   + 作者側は `Protagonist.reference_image`。
2. OpenAI `input_fidelity: high` を既定にするか（コスト増・顔の保持は上がる）。実測後。
3. パッケージ同梱の参照を**書庫の審査**で見るか（outcast 側、画像は今も審査外）。
