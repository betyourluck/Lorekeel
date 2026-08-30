# spec 24: 画像生成 — 物語に合わせた挿絵を、背景と文字の間に重ねる

**Status**: Done — **v0.5.13 で配布済み (2026-08-20、テスト合格・サイトの exe / package_spec.md 差し替え済み)**（同日起草 → 同日査読 rev2 → Phase 0〜D 同日実装。workspace 348 green / app 40 green・clippy clean・vue-tsc + vite build green。
Phase D: OpenAI / Gemini とも実ワイヤで 1 枚ずつ Green、プロンプト書き 3 通り良好 (`data_contract` `ImageGeneration.live_20260820`)。
残なし (ComfyUI + GUI 通しは 2026-08-20 ユーザー実機で Green。Gemini は無料枠 429 のみ = 設計どおりの RateLimited) — commits 0ea2d89 (P0) / aedddc7 (A) / 64c2f2f (B) / backend・bb83ce5 (C) / 本コミット (D)）

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

## 外部接地（2026-08-20 時点。rev2 = ユーザー査読の再接地を反映。**実装時に公式文書で再確認** — #78/#80）

- **OpenAI Images**: `POST {base}/v1/images/generations`
  `{model, prompt, size, quality, n: 1, output_format}` → `data[0].b64_json`（`url` は返らない実装が
  主流）。モデルは `gpt-image-1`（現行既定）/ `gpt-image-1-mini`。`size` = `1024x1024 | 1024x1536 |
  1536x1024 | auto`、`quality` = `low | medium | high | auto`。**quality で消費が 16 倍動く**
  （`high 1024x1024 = 4160 tok` / `low = 272 tok`）→ 手動生成なので **`low` 既定**、UI で上げられる。
  `style` 欄は無い・旧 DALL-E の `quality: hd` は `high` へ読み替え。互換サーバにも同じ口で届きうる。
  **ネガティブプロンプト欄は無い。**
- **Gemini（Nano Banana）**: `POST {base}/v1beta/models/{model}:generateContent` + `x-goog-api-key`
  （**`v1beta` 必須**）。`generationConfig.responseModalities: ["IMAGE","TEXT"]`（または `["IMAGE"]`）+
  **`generationConfig.imageConfig: {aspectRatio: "1:1"|"16:9"|…, imageSize: "1K"|"2K"|"4K"}`** —
  サイズは OpenAI の `size` ではなく **アスペクト比 + 解像度段**。モデル id（2026 時点、**実装時に
  `models.list` で再確認** — id は動く）: `gemini-2.5-flash-image`（Stable）/
  `gemini-3-pro-image-preview`（Pro）/ `gemini-3.1-flash-image-preview`（2）。応答は
  `candidates[0].content.parts[]` の `inlineData{mimeType, data}`。**安全ブロックは 200 + 空**（`parts`
  が空）で来る → `promptFeedback.blockReason` / 候補 `finishReason=SAFETY` + `safetyRatings` を
  理由としてトーストに載せる（#61 の写し・必須）。**既知の癖**: `imageConfig` が無視されて 1:1 で
  返る報告が複数 → Phase D で観測されたらプロンプト末尾に `(aspect ratio 16:9)` を付ける二重化を
  **opt-in** で足す（既定では付けない — プロンプトを汚す）。**ネガティブプロンプト欄は無い。**
- **ComfyUI**: `POST {base}/prompt` `{prompt: <API 形式ワークフロー>, client_id}` → `{prompt_id}` →
  `GET {base}/history/{prompt_id}` を **1 秒間隔でポーリング**（**発行直後の 404/空は正常** =
  キュー消化後に初めて入る。`status` / `node_errors` からエラーを抽出）→ `outputs[*].images[{filename,
  subfolder, type}]` → `GET {base}/view?filename=&subfolder=&type=` で PNG。ワークフローは **UI の
  レイアウト JSON ではなく API 形式**をユーザーが設定に貼る（alslime 方式）。プレースホルダは
  `%prompt%` に統一（SillyTavern 拡張と同じ流儀。`{{positive_prompt}}` 系は採らない）。
  **置換は型を保つ**（決定 13）。**リモート ComfyUI（RunPod 等）も backend が叩くので CSP 対象外**
  （reqwest は CORS 無関係）— 文書に明記。
- **CSP**: `connect-src` は localhost のみ、`img-src` に `data:` あり（`tauri.conf.json`）→
  **HTTP は全部 backend**。表示は `data:image/png;base64,…`。

## 決定事項（rev1 の提案。査読で確定）

1. **HTTP は backend、表示は data URL**（上記）。生成画像のバイト列は `GameSession.last_image`
   に保持し、保存 command はそれを書く（frontend から 1MB を送り返さない）。**二重保持は
   意図的**: backend = 保存用の原本 bytes（1 枚分のみ・差し替えで捨てる）/ frontend = 表示用の
   data URL 文字列（`store.generatedImage.dataUrl`）。**data URL は文字列なので差し替えれば GC
   される**（revoke が要るのは `URL.createObjectURL` の側 — rev2 査読の「GC されない」は逆で、
   ObjectURL を採らない理由がこれ）。IPC は生成 1 回につき 1 往復（1〜2MB）。
2. **プロンプト書きは GM クライアントの `generate`**（`SUMMARY_LLM_*` のような別指定は v1 では
   作らない。要るなら spec 10 と同型で後から足せる）。**秘密は渡さない** — 素材に `state_brief`
   を使わず、語り・場所説明・presence の profile・world だけを渡す。`hidden_*`/`secret_*` は
   構造的に入らない。
3. **作者フィールドは 1 つだけ・任意**: `Scenario.image_style: String`（serde default 空）+
   `package.yaml` の `image_style`（注入で全モジュールを支配、facts_policy と同じ配線）。
   例「水彩画・淡い色・キャラはデフォルメ」。engine 非解釈。**ON/OFF の作者ゲートは作らない**
   — 挿絵は語りに触れないプレイヤー側の鑑賞レイヤーで、押すのもプレイヤー。プロバイダを
   設定した人だけに操作が出る。（起草時は「`use_tts` と違い」と対比したが、その `use_tts`
   自体が 2026-08-31 に同じ理由で撤去され、読み上げもプレイヤー側の設定一本になった。）
4. **生成は手動・都度**（ユーザー要望）。自動生成（毎ターン/トリガー発火時）は v1 外 —
   コストと待ち時間をプレイヤーが握る。
5. **揮発**: 生成画像は `SessionSave` に入れない。新しいゲーム / ロード / campaign 遷移で消す。
   **次の受理ターンでは消さない**（場面が進んでも挿絵は残す。消すのはトグル/差し替え/保存後の
   任意。イベント CG の「瞬間」と違い、挿絵は作者の演出でなくプレイヤーの鑑賞物）。
6. **保存**: `{YYYYMMDD_HHMMSS}_{パッケージ名}_T{turn}.png` を設定フォルダ
   （既定 `app_data/images`、ログ保存と同じ流儀）へ。ファイル名はパス要素禁止（`save_log_file`
   同型のトラバーサル遮断）。フォルダ不在は作成。「フォルダを開く」ボタンも同列。
7. **文字トグル**は `ConversationLog` を `invisible pointer-events-none`（レイアウト維持・スクロール位置不変・クリックを奪わない。決定 22）。
   入力欄 / 右ペインは消さない。**挿絵トグル**は層の `v-show`。どちらも localStorage に
   持たない（セッション内の一時状態 — 次回起動で文字が消えていたら事故）。
8. **設定の置き場**: 非秘密（プロバイダ/URL/モデル/サイズ/様式/接頭辞/ネガ/フォルダ/ワーク
   フロー）は localStorage、**API キーは `app_data/.env`**（`IMAGE_API_KEY_OPENAI` /
   `IMAGE_API_KEY_GEMINI`、`set_llm_config` と同じ `upsert_env` 経路）。キー欄は LLM 設定と
   同じく伏せ字入力。
9. **失敗の見せ方**: トースト 1 行（`logToast` 経路）に理由を載せる — 401/403（キー）/
   429（レート、`Retry-After` 尊重は v1 外）/ ブロック（理由）/ タイムアウト（プロバイダ別、
   決定 21）/ 接続不能（URL）。**沈黙させない**（TTS #69 の反省）。
10. **多人数は v1 外**（ユーザー指示）。挿絵はホストのローカル表示のみ・ゲストへ配らない。
11. **gm_core は `image_style` 1 フィールド以外無改修。** harness は `build_image_prompt_request`
    （純関数・テスト可）だけ。engine の検証・状態には一切触れない。
12. **サイズはプリセット 1 軸 + プロバイダ別写像**（rev2）: UI は `正方形 / 横長 / 縦長` の 3 値
    （+ 解像度段 `標準 / 高`）だけを持ち、data_contract の表で各プロバイダの語彙へ写す —
    OpenAI `size`（`1024x1024 / 1536x1024 / 1024x1536`）+ `quality`（標準=`low` / 高=`medium`。
    `high` は UI の「最高（高コスト）」でのみ）/ Gemini `imageConfig.aspectRatio`（`1:1 / 16:9 / 9:16`）+
    `imageSize`（標準=`1K` / 高=`2K`）/ ComfyUI `%width%`/`%height%`（`1024×1024 / 1344×768 /
    768×1344`、SDXL 系の既定）。プロバイダを切り替えても設定が壊れない（1 軸を写すだけ）。
13. **ComfyUI の置換は `serde_json::Value` を歩いて型を保つ**（rev2）: ワークフロー JSON の文字列値が
    **ちょうど** `"%width%"` / `"%height%"` / `"%seed%"` なら **数値 JSON**（bare integer）に差し替え、
    `"%prompt%"` / `"%negative%"` は文字列値として差し替え（部分一致 = 文字列内の置換も可、serde が
    エスケープするので JSON は壊れない）。**未置換は既定値で埋める**（seed = 乱数、width/height =
    プリセット、negative = 設定値 or 空）— 残すと ComfyUI が `invalid literal` で落ちる。
    単純な文字列置換は採らない（数値欄に `"1024"` とクォート付きで入って 400 になる）。
14. **ネガティブプロンプトは ComfyUI（と A1111）だけ**（rev2）: OpenAI / Gemini には欄が無い。
    UI はプロバイダがそれ以外のとき欄を無効化し、プロンプト書きも「ネガを本文に混ぜない」で固定
    （ネガをプロンプトに書かせると「no hat」を描く類の逆効果が出る）。
15. **第三層の可読性**（rev2・旧未決 4 を昇格）: `object-fit: contain` 既定 + **不透明度スライダ
    （0.3〜1.0、既定 0.85）** + **第三層自体に暗幕 `rgba(0,0,0,0.35)`** を掛ける。`main` の暗幕は
    `main` の背景にしか掛からず、第三層はその上に乗るので文字（白）とのコントラストが飛ぶ — 文字
    トグル無しでも読める状態を既定にする。`cover` は顔が切れるので採らない。設定「グラフィック」
    ではなく「画像生成」タブに置く（機能の所属で分ける）。
16. **揮発境界のレース防御**（rev2）: 生成は非同期（OpenAI high で 60〜90 秒・ComfyUI は分単位）。
    `GameSession.scene_seq: u64` を 新しいゲーム / ロード / campaign 遷移 で進め、`generate_image` は
    発行時の `scene_seq` を持って走り、**完了時に現在値と一致しなければ破棄**してトースト
    「場面が変わったため破棄しました」。frontend 側も `requestId` で古い完了を無視（二層）。
17. **素材は全てプレイヤーが既に見ているもの**（rev2・旧決定 2 の明文化）: `Location.description`
    = 開幕ログに出る文（GM 向けの隠し説明は `Location` に**存在しない**、spine.rs で確認）/
    `CharacterDef.profile` = プロフィールカードで見える / 直前の語り = 会話ログ / `world` =
    scenario_brief 経由で GM に出るがプレイヤーにも世界観として提示済み。`state_brief`・`hidden_*`・
    `secret_*`・`internal_*`・facts は素材に含めない。PoC で「secret/hidden 属性の値が request 本文に
    現れない」を固定。**presence の予算**: 上位 3 名は profile 400 字まで、4 人目以降は **120 字**
    （名前だけにはしない — 容姿の手掛かりは全員に残す。順序は主人公先頭・NPC は id 順＝決定論。
    実装時に改訂）、素材合計 2000 字以内（10 人盤面で 4000 字に溢れる問題）。
18. **設定は 2 ソース**（rev2）: frontend が `invoke generate_image({config: <localStorage の非秘密>})`
    で非秘密を渡し（場面世代は backend が自分で捕まえる — 実装時に改訂）、backend が `app_data/.env` から `IMAGE_API_KEY_{PROVIDER}` を
    **マージ**する。`image_gen_probe` も同じ 2 ソース。図の矢印は 1 本だが実装は 2 本 — 契約に
    `config_sources` として明記。
19. **`image_gen_probe`（接続テスト）**は画像を生成しない: OpenAI = `GET {base}/v1/models`
    （認証のみ）/ Gemini = `GET {base}/v1beta/models/{model}`（認証 + モデル存在）/ ComfyUI =
    `GET {base}/system_stats`（到達性）。結果は「接続できました / 401 キー / 404 モデル / 接続不能」
    の 1 行。サイズ語彙の検証は probe ではやらない（実生成で 400 の本文を surface する方が正確）。
20. **ファイル名**（rev2）: `{YYYYMMDD_HHMMSS}_{slug}_T{turn}.png`。`turn` = `GameState.turn`
    （campaign 跨ぎで持ち越す累積ターン。ロードしてもセーブ時の値から続く）。`slug` = パッケージ
    フォルダ名を ASCII 英数と `-_` だけに落とし、**空になったら（日本語名）タイトルの FNV-1a 8 桁
    hex** で代替（セーブファイルのキーと同じ流儀）。パス要素は拒否（`save_log_file` 同型）。
21. **タイムアウトはプロバイダ別**（rev2）: OpenAI 120 秒 / Gemini 90 秒 / ComfyUI 600 秒
    （ポーリング上限。ワークフロー次第で分単位）。UI で上書き可（`timeout_secs`）。
22. **文字トグルは `invisible pointer-events-none`**（rev2）: `invisible` だけだと透明なログが
    クリックを奪う。
23. **`image_style` は 500 字上限の lint（非 fatal）**（rev2）: 超過は警告して先頭 500 字だけ使う。
    複数行可（改行はそのまま素材に入る）。
24. **プロンプト様式の既定はプロバイダに倒す**（rev2）: OpenAI / Gemini = 自然文、ComfyUI / A1111 =
    タグ。ユーザーが変えられる（UI に「推奨」バッジ）。
25. **エラー型**（rev2）: `Unauthorized / RateLimited / Blocked{reason} / Timeout{provider} /
    ComfyNodeError{node, msg} / Api{status, body}` を 1 行理由でトーストに。`Retry-After` の尊重は
    v1 外（手動なので押し直せばよい）。

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
  `image_style` の注入（facts_policy と同型）。PoC: 素材の予算と順序 / 秘密が素材に入らない
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

## 未決 → rev2 で決着（ユーザー査読の回答）

1. **プロバイダ v1 = OpenAI / Gemini / ComfyUI の 3 つ。** A1111 は Phase D 任意（`/sdapi/v1/txt2img`
   は ComfyUI とほぼ同じ置換で済む・コスト最小）。
2. **プロンプト書きは GM クライアント共用。** ローカル弱モデル時の質は `image_style` に
   「タグで 50 語以内」等を書いて緩和。別指定 `IMAGE_PROMPT_LLM_*` は Phase D 実測後。
3. **揮発は「プレイヤーが消す/差し替えるまで」。** 場所が変わっても残す（回想的に見たい時に不便）。
4. **`contain` 既定 + 不透明度スライダ + 第三層の暗幕**（決定 15）。`cover` は顔が切れる。
5. **PNG 固定。** 配布物ではないので変換依存を増やさない。

## 台帳

- `data_contract.yaml` `ImageGeneration` 節（Phase 0）/ `CLAUDE.md` 現状節 / `failures.md`
  （実測で出た罠）/ outcast `package_spec.md`（`image_style`）/ `.env.example`（`IMAGE_API_KEY_*`）。

## rev2 査読の反映記録（2026-08-20）

ユーザー査読 10 点 + 軽微 3 点 + 外部接地。**採用 12 / 訂正して採用 1**:

| # | 指摘 | 反映 |
|---|---|---|
| 1 | サイズ語彙が 3 プロバイダで違う | 決定 12: プリセット 1 軸 + 写像表 |
| 2 | ネガの対応プロバイダが曖昧 | 決定 14: ComfyUI/A1111 のみ・UI 無効化・本文に混ぜない |
| 3 | 二重保持と IPC、data URL が GC されない | **訂正して採用**: 二重保持は意図的 (原本 bytes / 表示文字列)。data URL は文字列で差し替えれば GC される — revoke が要るのは ObjectURL の側。ObjectURL を採らない理由として決定 1 に明記 |
| 4 | 第三層に暗幕が掛からず文字が飛ぶ | 決定 15: contain + 不透明度 + 層自体の暗幕 (旧未決 4 を昇格) |
| 5 | 生成中の遷移で古い絵が新場面に乗る | 決定 16: `scene_seq` ガード (backend) + `requestId` (frontend) の二層 |
| 6 | プレースホルダの文字列置換は数値欄で 400 | 決定 13: `Value` を歩いて型を保つ・未置換は既定値で埋める |
| 7 | 場所説明に GM 向け秘密が混じる余地 | 決定 17: `Location` に隠し説明は無い (spine.rs 確認)・素材は全てプレイヤー既視のもの・presence 予算 |
| 8 | 設定が 2 ソースのマージ | 決定 18: 契約に `config_sources` |
| 9 | probe が図に無い・範囲 | 決定 19: 認証/到達性のみ・画像は作らない |
| 10 | ファイル名の turn 定義・日本語名の空 slug | 決定 20: `GameState.turn`・FNV 代替 |
| B1 | `invisible` は pointer-events を残す | 決定 22 |
| B2 | タイムアウト一律 180 秒 | 決定 21: プロバイダ別 |
| B3 | `image_style` の上限 | 決定 23: 500 字 lint |

外部接地の差分: OpenAI `quality` (16 倍) → `low` 既定 / Gemini `imageConfig` (aspectRatio+imageSize)
と `v1beta` 必須・モデル id 3 つ・`imageConfig` 無視の癖 (opt-in 二重化) / ComfyUI の 1 秒ポーリング
と発行直後 404 正常・API 形式 JSON・リモートは CSP 対象外。

