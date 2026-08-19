# spec 24: 画像生成 — 物語に合わせた挿絵を、背景と文字の間に重ねる

**Status**: Draft（2026-08-20 起草。ユーザー要望 → 三点測量（先行アプリ 2 本・Kataribe の提示層・Memoria）→ rev1。査読待ち）

## 動機

この種のアプリ（AI ロールプレイ / ノベル）では画像生成が大きい。先行アプリ 2 本がどちらも
持っている:

| 先行 | 対応プロバイダ | 起動 | 見せ方 | プロンプトの作り方 |
|---|---|---|---|---|
| SAIVerse（Next.js+FastAPI） | **Nano Banana / Nano Banana Pro（Gemini 画像）・ChatGPT 画像生成（OpenAI）** = 有料 API 3 つ | ペルソナのツール | （README に記載なし） | （README に記載なし） |
| alslime（Go+React） | **ComfyUI** のみ（支援者限定機能） | 応答メッセージの「画像生成」ボタン（手動） | メッセージに添付 + **会話画面の背景**（不透明度・配置を調整可） | **分析 AI** が会話の流れを読んで **danbooru タグ / 自然文** を生成。キャラ設定（容姿・服装・LoRA）を結合 |

Kataribe は既に **背景画像（場所）とイベント CG（トリガー）** を authored アセットとして持つ
（spec 01）。ここに足すのは**プレイスルー固有の一枚絵** — 作者が描いておけない、その卓で
起きた場面の挿絵。位置づけは **authored アセットの上・文字の下に重ねる第三の画像層**で、
既存の背景/CG は一切変えない（ユーザー要望）。

**北極星との関係**: 画像は**提示層**の仕掛けで、正本にも語りにも触れない。生成プロンプトを書く
LLM 呼び出しは GM のターンとは別の独立した 1 呼び出し（あらすじ要約・エピローグと同型の
`generate`）で、**GM の秘密（`hidden_*`/`secret_*`）を見ない**ようにも組める — 挿絵書きは
GM ではない。

## 何を作るか

1. **設定 → 「画像生成」タブ**（新設）: プロバイダ選択 / サーバー URL / モデル / API キー /
   画像サイズ / スタイル接頭辞 / ネガティブプロンプト（対応プロバイダのみ）/
   プロンプト様式（自然文 | タグ）/ **保存フォルダ** / ComfyUI はワークフロー JSON。
2. **会話ペインの操作**（読み上げ操作 `TtsControls` と同じ「ホバーで右下に浮き出る」列）:
   - 🖼 **生成** — 押すと現在の場面から挿絵を生成。処理中は押せない（スピナー）。
     気に入らなければ**何度でも押せる**（押すたび前の画像を差し替え）。
   - 💾 **保存** — 画像が在るときだけ出る。設定のフォルダへ書く。
   - 🖼 **生成画像 表示/非表示** トグル。
   - **文字 表示/非表示** トグル（挿絵を鑑賞するため。入力欄は残す）。
3. **第三の画像層**: `main`（背景/CG + 暗幕）の上・`ConversationLog` の下に
   `absolute inset-0` の層を 1 枚。生成画像を `object-fit: contain`（既定）で敷く。
4. **プロンプト生成**: GM の LLM クライアントで **1 回の `generate`**（ツール無し）。素材 =
   直前の語り + 現在地の説明 + この場にいる人物の profile + world + 作者のスタイル指針
   （任意）+ ユーザーのスタイル接頭辞。出力 = 画像プロンプト 1 本（様式はタグ/自然文）。
5. **プロバイダ（v1）**: **OpenAI Images** / **Gemini（Nano Banana）** / **ComfyUI**。
   先行 2 本の和集合。Stable Diffusion WebUI（A1111 `/sdapi/v1/txt2img`）は Phase D で任意追加
   （ローカルで最も普及しているが先行には無い。実装コストは最小）。

## 外部接地（2026-08-20 時点。**実装時に公式文書で形を再確認する** — #78/#80 の教訓）

- **OpenAI Images**: `POST {base}/v1/images/generations` `{model: "gpt-image-1", prompt, size, n: 1}`
  → `data[0].b64_json`。サイズ語彙はモデル依存（`1024x1024` / `1536x1024` / `1024x1536`）。
  互換サーバ（`/v1/images/generations` を話すローカル）にも同じ口で届きうる。
- **Gemini（Nano Banana）**: `POST {base}/v1beta/models/{model}:generateContent` +
  `x-goog-api-key`、`generationConfig.responseModalities: ["IMAGE","TEXT"]`、
  モデルは `gemini-2.5-flash-image` 系（Pro 版は別 id）→ `candidates[0].content.parts[]` の
  `inlineData{mimeType, data(base64)}`。**安全ブロックは 200 + 空**で来る（#61 と同型）→
  `promptFeedback.blockReason` / 候補 `finishReason` を理由として surface。
- **ComfyUI**: `POST {base}/prompt` `{prompt: <workflow API 形式>, client_id}` → `{prompt_id}`
  → `GET {base}/history/{prompt_id}` をポーリング → `outputs[*].images[{filename, subfolder, type}]`
  → `GET {base}/view?filename=&subfolder=&type=` で PNG。ワークフローは**ユーザーが API 形式
  JSON を設定に貼る**（alslime 方式: 汎用ワークフローを配布し、ComfyUI で一度開いて確認させる）。
  **プレースホルダ**を Kataribe が置換: `%prompt%` / `%negative%` / `%seed%` / `%width%` / `%height%`。
  同梱の汎用ワークフロー（SDXL/Anima 系の最小 txt2img）を `app/src/assets/comfy_generic.json` に置く。
- **CSP**: `connect-src` は localhost のみ、`img-src` に `data:` あり（`tauri.conf.json`）。
  → **HTTP は全部 backend（Rust/reqwest）**。クラウドのキーを WebView に置かず、CSP も触らない。
  ComfyUI も backend から叩く（経路を 1 本に）。表示は `data:image/png;base64,…`（IPC で 1〜2MB、許容）。

## 決定事項（rev1 の提案。査読で確定）

1. **HTTP は backend、表示は data URL**（上記）。生成画像のバイト列は `GameSession.last_image`
   に保持し、保存 command はそれを書く（frontend から 1MB を送り返さない）。
2. **プロンプト書きは GM クライアントの `generate`**（`SUMMARY_LLM_*` のような別指定は v1 では
   作らない。要るなら spec 10 と同型で後から足せる）。**秘密は渡さない** — 素材に `state_brief`
   を使わず、語り・場所説明・presence の profile・world だけを渡す。`hidden_*`/`secret_*` は
   構造的に入らない。
3. **作者フィールドは 1 つだけ・任意**: `Scenario.image_style: String`（serde default 空）+
   `package.yaml` の `image_style`（注入で全モジュールを支配、`use_tts` と同じ配線）。
   例「水彩画・淡い色・キャラはデフォルメ」。engine 非解釈。**`use_tts` のような ON/OFF の
   作者ゲートは作らない** — TTS は語りの文体（作者の領分）に効いたが、挿絵は語りに触れない
   プレイヤー側の鑑賞レイヤーで、押すのもプレイヤー。プロバイダを設定した人だけに操作が出る。
4. **生成は手動・都度**（ユーザー要望）。自動生成（毎ターン/トリガー発火時）は v1 外 —
   コストと待ち時間をプレイヤーが握る。
5. **揮発**: 生成画像は `SessionSave` に入れない。新しいゲーム / ロード / campaign 遷移で消す。
   **次の受理ターンでは消さない**（場面が進んでも挿絵は残す。消すのはトグル/差し替え/保存後の
   任意。イベント CG の「瞬間」と違い、挿絵は作者の演出でなくプレイヤーの鑑賞物）。
6. **保存**: `{YYYYMMDD_HHMMSS}_{パッケージ名}_T{turn}.png` を設定フォルダ
   （既定 `app_data/images`、ログ保存と同じ流儀）へ。ファイル名はパス要素禁止（`save_log_file`
   同型のトラバーサル遮断）。フォルダ不在は作成。「フォルダを開く」ボタンも同列。
7. **文字トグル**は `ConversationLog` を `invisible`（レイアウト維持・スクロール位置不変）。
   入力欄 / 右ペインは消さない。**挿絵トグル**は層の `v-show`。どちらも localStorage に
   持たない（セッション内の一時状態 — 次回起動で文字が消えていたら事故）。
8. **設定の置き場**: 非秘密（プロバイダ/URL/モデル/サイズ/様式/接頭辞/ネガ/フォルダ/ワーク
   フロー）は localStorage、**API キーは `app_data/.env`**（`IMAGE_API_KEY_OPENAI` /
   `IMAGE_API_KEY_GEMINI`、`set_llm_config` と同じ `upsert_env` 経路）。キー欄は LLM 設定と
   同じく伏せ字入力。
9. **失敗の見せ方**: トースト 1 行（`logToast` 経路）に理由を載せる — 401/403（キー）/
   429（レート、`Retry-After` 尊重は v1 外）/ ブロック（理由）/ タイムアウト（既定 180 秒、
   ComfyUI は長い）/ 接続不能（URL）。**沈黙させない**（TTS #69 の反省）。
10. **多人数は v1 外**（ユーザー指示）。挿絵はホストのローカル表示のみ・ゲストへ配らない。
11. **gm_core は `image_style` 1 フィールド以外無改修。** harness は `build_image_prompt_request`
    （純関数・テスト可）だけ。engine の検証・状態には一切触れない。

## アーキテクチャ

```
[frontend]                         [backend (Tauri)]                 [外]
設定「画像生成」 ──localStorage/.env──▶ image_gen::Config
🖼 生成 ──invoke generate_image──▶  1. harness::build_image_prompt_request(scenario, state, last_narration, style)
                                     2. llm.generate(req) → prompt 文字列 (様式: tags|prose)
                                     3. image_gen::Provider::{OpenAi,Gemini,Comfy}.generate(prompt) ──HTTP──▶ API
                                     4. bytes → session.last_image; 返値 {data_url, prompt, width,height}
層に敷く / トグル
💾 保存 ──invoke save_generated_image(folder)──▶ bytes を書く → path を返す → トースト
```

- `app/src-tauri/src/image_gen.rs`: `Provider` enum + 各 encode/decode **純関数**
  （request body の組み立て / response からの bytes 抽出 / ComfyUI のプレースホルダ置換と
  history の走査）+ HTTP 実行（reqwest、タイムアウト設定）。純関数は PoC で固める。
- `crates/harness/src/image_prompt.rs`: `build_image_prompt_request` — 素材の選別と予算
  （語り 1200 字 / 場所説明 / presence 各 profile 400 字 / world 600 字 / image_style）、
  様式別の指示（タグ = 「danbooru 形式・カンマ区切り・英語・50 語以内・人物は容姿と服装・
  構図・光」/ 自然文 = 「英語 1〜3 文・被写体・構図・光・画風」）、**禁止** = 固有名詞の
  発明・語りに無い人物・状態の断定。出力はプロンプトのみ（前置き禁止、CoT 除去 #30 同型）。
- frontend: `ImageControls.vue`（TtsControls の隣・同じホバー列）+ `store` に
  `generatedImage: {dataUrl, prompt} | null` / `imageBusy` / `showGeneratedImage` / `showText`。
  層は `App.vue` の `relative` コンテナ内、`ConversationLog` の前に 1 枚。

## Phase 分割

- **Phase 0 — 契約凍結**: data_contract `ImageGeneration` 節（provider enum / 設定項目 /
  command 3 つ `generate_image` `save_generated_image` `image_gen_probe`(接続テスト) /
  プロンプト様式 / 揮発と保存の規則 / `image_style` の注入）。outcast `package_spec.md` に
  `image_style` 欄を追記（**仕様変更なので報告対象**）。
- **Phase A — backend プロバイダ（純関数 + PoC）**: OpenAI / Gemini / ComfyUI の encode・decode、
  ComfyUI プレースホルダ置換、ファイル名の生成とトラバーサル遮断、ブロック理由の surface。
  PoC: 各 decode を実応答の形（文書 + probe で接地）で Red→Green、置換の全プレースホルダ、
  不正ファイル名の拒否。
- **Phase B — harness プロンプト書き**: `build_image_prompt_request` と様式別指示、
  `image_style` の注入（`use_tts` と同型）。PoC: 素材の予算と順序 / 秘密が素材に入らない
  （`hidden_*` 属性を持つ盤面で request 本文に値が現れない）/ 様式の切替 / 空の
  `image_style` で行が出ない。
- **Phase C — frontend**: 設定タブ（接続テストつき）・操作列・第三層・トグル・保存・トースト。
  vue-tsc / vite build green。
- **Phase D — live 実測**: 3 プロバイダで 1 枚ずつ（ComfyUI は手元環境があれば）。見るもの =
  プロンプトの質（語りに無い人物が混じらないか）・待ち時間・ブロック時の文言・保存の実体。
  任意で A1111 を足す。

## PoC（書くもの）

- `image_gen::openai::decode` が `data[0].b64_json` を bytes に / 空・エラー形は理由つき Err。
- `image_gen::gemini::decode` が `inlineData` を拾い、`blockReason`/`finishReason` を
  `Blocked{reason}` に（#61 の写し）。
- `image_gen::comfy::substitute` が 5 プレースホルダを置換し、未知プレースホルダは残さない
  （残すと JSON が壊れたまま送られる）。`comfy::first_image(history)` が `outputs` 走査で
  最初の画像参照を返す。
- `image_file_name(now, package, turn)` がパス要素を含まず、パッケージ名の不正文字を落とす。
- `build_image_prompt_request` の 4 本（上記 Phase B）。
- 設定の往復（localStorage 形 / `.env` キー名）は TS 側の unit（あれば）。

## スコープ外（v1）

- 自動生成（毎ターン / トリガー / CG の代替）— まず手動で質とコストを見る。
- キャラ一貫性（参照画像 / LoRA / IP-Adapter）— `CharacterDef` に参照画像 ID を持たせる案は
  Memoria の `kataribe_anime_idea`（playlog→挿絵）と同じ方向だが、プロバイダ横断で揃わないので
  v2。ComfyUI のワークフローにユーザーが LoRA を書く分には v1 でも通る。
- 生成画像の chronicle / SessionSave / あらすじへの同梱（揮発で割り切る）。
- 多人数（ゲストへの配布・卓内共有）。
- 会話ログ（テキスト保存）への画像パス併記。
- NovelAI / Stability / Replicate 等（要望が出たら Provider を 1 つ足すだけ）。

## 未決（査読で決める）

1. **プロバイダ v1 の範囲**: OpenAI / Gemini / ComfyUI の 3 つで足りるか。A1111 を v1 に
   入れるか（ローカル派の最大公約数。私は Phase D 任意で十分と見る）。
2. **プロンプト書きを GM クライアントに任せるか**（決定 2）: ローカル弱モデルが GM のとき
   画像プロンプトの質が落ちる。`IMAGE_PROMPT_LLM_*` の別指定を v1 から持つか（spec 10 と同型
   で安い）。私は v1 は GM 共用・別指定は Phase D の実測後。
3. **揮発の境界**（決定 5）: 受理ターンで消さない、でよいか。場所が変わったら消す案もある
   （CG と同じ「場所を離れたら」）。私は「プレイヤーが消すまで」を推す。
4. **挿絵の敷き方**: `contain`（余白あり・全体が見える）か `cover`（背景のように満たす）か。
   設定で切替可にするか（alslime は不透明度と配置を調整可）。私は `contain` 既定 + 不透明度
   スライダ 1 本。
5. **保存の形式**: PNG 固定か WebP 変換（配布サイズの作法 2026-07-18）か。保存は鑑賞用で
   配布物ではないので **PNG 固定**でよいと見る（変換は依存が増える）。

## 台帳

- `data_contract.yaml` `ImageGeneration` 節（Phase 0）/ `CLAUDE.md` 現状節 / `failures.md`
  （実測で出た罠）/ outcast `package_spec.md`（`image_style`）/ `.env.example`（`IMAGE_API_KEY_*`）。
