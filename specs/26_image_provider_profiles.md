# spec 26: 画像生成設定のプロバイダ別プロファイル — 切替で前の設定を漏らさない

**Status**: Draft rev2（2026-08-21 起草 → 同日ユーザー査読 3+3 点反映。実装待ち）

## 動機（実測 2 件、2026-08-21）

spec 24/25 の画像生成設定は 1 枚のフラットな器（`localStorage["kataribe.imageGen"]`）に
全プロバイダの値を同居させている。プロバイダを切り替える運用（クラウドで試す → ComfyUI に
移る、が受領者の自然な動線）で、**前のプロバイダの値が次のプロバイダに漏れて黙って壊れる**
事故が実機で 2 件出た。

1. **`baseUrl` の漏れ**: ユーザーが Gemini（`…googleapis.com/v1beta`）から ComfyUI へ切替 →
   `POST {base}/prompt` が **Google に飛んで失敗**。既存の緩和策 `onImageProviderChange` は
   「旧プロバイダの**既定値のまま**なら新既定へ差し替え、カスタム値なら温存」というヒューリ
   スティックだが、ユーザーの URL は `/v1beta` 付き＝既定（`/v1beta` 無し）と不一致＝カスタム
   扱いで温存された。**「カスタムなら温存」は同一プロバイダ内でしか正しくない** — A 用の
   カスタム URL が B で有効なことはほぼ無い。ヒューリスティックの穴でなく、器の形の問題。
2. **様式 `tags` の誤適用**: 様式の既定を**プロバイダで決める**（comfy=tags）ため、写実系
   モデル（Krea2）を ComfyUI に置いたユーザーに danbooru タグ体が送られた。実測（同一
   ワークフロー・seed 固定の A/B）: 散文は三十代の男を正しく描き、タグ体は **`1boy` が
   主人公を子供に変えた**（danbooru 語彙は年齢無指定だと若年に倒れる）。**タグ語彙が合うか
   はプロバイダでなくその奥のモデル族で決まり、Kataribe は ComfyUI の奥のモデルを知らない**
   — だからこの判断はユーザーの選択として保存されるべきで、その選択もプロバイダ別に
   持たないと「comfy では prose にした」が openai を試した往復で意味を失う。

## 何を作るか（rev2）

1. **frontend の設定を共有部とプロバイダ別部に分離する**（backend `ImageGenConfig` は無改修
   — `toBackendConfig` が実効値へ畳む現契約のまま）。

   - **プロバイダ別** `perProvider: Record<ImageProvider, {...}>`:
     `baseUrl` / `model` / `style` / `negative` / `workflowJson` / `timeoutSecs`
     （どれも値の意味がプロバイダ（とその奥のモデル）に紐づく。negative/workflowJson は
     実質 comfy 専用だが、器として各プロバイダに持たせ UI 側の表示条件は従来どおり）
   - **共有のまま**: `enabled` / `shape` / `detail` / `userPrefix` / `opacity` / `folder`
     （「どんな絵が欲しいか」というユーザー意図。プロバイダをまたいで追従するのが正しい。
     shape/detail は既存の `SizeMap` がプロバイダ別に写像する）
   - **`enabled` 共有の制約（意図的）**: プロバイダごとの ON/OFF は持たない。「comfy が
     死んでいる間だけ openai を使う」はプロバイダ切替そのもので表現する（スロット分離に
     より切替が無損失になるのが本 spec なので、この運用は切替で足りる）。

2. **プロバイダ切替は表示スロットの切替だけにする。** `onImageProviderChange` の
   差し替えヒューリスティック（既定値比較）は**撤去** — 各プロバイダのスロットが自分の値を
   持つので、推測で書き換える必要自体が消える。切替で値は一切失われない（戻れば戻る）。

3. **`toBackendConfig` の実効値写像を凍結する**（動機 1 の逆向きの漏れ＝「openai スロットに
   残った workflowJson が backend へ流れる」を仕様で塞ぐ。現行コードに既にあるゲートの明文化
   も含む）:

   ```
   slot = perProvider[provider]
   effective = {
     provider,
     base_url:     slot.baseUrl.trim()  || DEFAULT_BASE_URL[provider],
     model:        slot.model.trim()    || DEFAULT_MODEL[provider],   // comfy は "" のまま
     style:        slot.style           || null,                      // null = 自動 (プロバイダ既定)
     user_prefix:  userPrefix,                                        // 共有
     negative:     provider == comfy ? slot.negative : "",            // 他プロバイダへは送らない
     workflow_json: provider == comfy && slot.workflowJson.trim() ? slot.workflowJson : null,
     timeout_secs: slot.timeoutSecs > 0 ? floor(slot.timeoutSecs) : null,
   }
   ```

   `timeout_secs: null` のフォールバックは **backend 既存の `ImageGenConfig::timeout()`**
   （openai 120 / gemini 90 / comfy 600、契約 `timeouts`）が受ける — frontend に既定表を
   複製しない（二重定義の芽）。**他プロバイダのスロットは一切読まない**（provider の
   スロット + 共有部だけが wire に乗る）。

4. **旧形式の移行（一方向・初回ロード時）**: `perProvider` キーが無ければ旧フラット形と
   みなし、フラットの `baseUrl`/`model`/`style`/`negative`/`workflowJson`/`timeoutSecs` を
   **その時の `provider` のスロットのみに**写す。**他プロバイダのスロットは既定
   （baseUrl/model は `DEFAULT_*`、style は自動、他は空）で初期化し、旧 `style` が明示
   `tags` であっても複製しない** — 移行直後の実効値が全プロバイダで従来と一致する
   （挙動変化ゼロ）と同時に、明示 tags が他プロバイダへ漏れない（動機 2 の根治と同じ向き）。
   `LocationItem`/`StatInit` の両受けと同じ「旧を黙って読める」路線（localStorage なので
   serde でなく JSON 手当て）。書き戻しは常に新形式。
   **部分欠損も同じ規則**: `perProvider` は在るが一部プロバイダのスロットが無い JSON
   （将来プロバイダを追加した後の旧データ）は、欠けたスロットを既定で埋める。

5. **様式 `tags` の年齢・体格接地（付随改善・prompt 層・harness）**: `ImagePromptStyle::Tags`
   の出力形式指示に「**人物には年齢・体格のタグを必ず含める**（`adult man`, `mature male`,
   `30 years old` 等。danbooru 語彙は年齢無指定だと若年に倒れる）」を追記する。
   writer の system は挿絵ごとの単発 `generate` で GM ターンのキャッシュ prefix とは別物
   ＝**prompt caching 無風**。tags を使い続ける（SD/アニメ系モデルの）ユーザーにも効く
   独立の改善で、様式の既定変更はしない（comfy=tags の既定自体は SD 系多数派に正しい。
   誤適用の根治は 1〜4 のプロファイル分離が担う）。

## 決定事項（rev2）

1. **設定の分離は frontend のみ**（backend `image_gen::ImageGenConfig` / command の signature
   不変、`toBackendConfig` が実効スロットを畳んで渡す）。**ただし本 spec は付随改善として
   harness の writer プロンプト文言変更（Tags 形式のみ、何を作るか 5）を含む** — こちらも
   signature 不変・prose は byte 不変。
2. **`userPrefix` は共有**。「写実で撮れ」という意図はプロバイダをまたぐ。ただし tags 体で
   書いた接頭辞が prose の writer に渡ることは引き続き起こる — writer が言い換えるので
   壊れはしない（2026-08-21 実測: `anime, 2d` 接頭辞も prose 文へ正しく織り込まれた）が、
   `1boy` のような**年齢バイアスを含むタグは言い換え後もバイアスが残りうる**。
   **接頭辞は散文（またはバイアスの無い形容）で書くことを推奨**とし、設定 UI の
   placeholder / ヘルプにその旨を出す。per-provider 化は「同じ意図を二度書かせる」コストが
   上回るため却下。
3. **切替ヒューリスティックは撤去**（縮小・修繕ではなく）。「既定値なら差し替え」は
   スロット分離後は無意味になり、残すと二重管理になる。
4. **様式の「プロバイダ既定（自動）」選択肢は残す**。既定の写像（openai/gemini=prose,
   comfy=tags）も不変。変わるのは「明示選択がプロバイダ別に保存される」ことだけ。
5. **API キーは対象外**: もともと backend `.env` の `IMAGE_API_KEY_{OPENAI|GEMINI}` で
   プロバイダ別（spec 24 契約 `config_sources`）。今回の分離は非秘密側を追いつかせる形。

## スコープ外

- ComfyUI ワークフロー JSON の鮮度検査（画面のグラフと貼り付けコピーの乖離 — 2026-08-21
  実測で強化 LLM の ON/OFF が食い違っていた）。ユーザー content の検査は UI 形式 JSON 検知
  （2828d8a）の線までで止める。
- モデル族の自動判別（ComfyUI の奥が SD 系か Flux/Krea 系か）。判別材料が wire に無い。
- プロバイダ別の `shape`/`detail`（SizeMap の写像で足りている実績）。
- プロバイダ別の `enabled`（何を作るか 1 の制約note どおり、切替で表現する）。

## PoC

- `loadImageGenSettings`: 旧フラット形 → 現 provider のスロットのみに移行・他スロットは既定・
  **旧 style=tags が他スロットへ複製されない**・書き戻しは新形式（roundtrip）。新形式は
  そのまま読める。**`perProvider` 部分欠損は欠けたスロットだけ既定で埋まる**。壊れた JSON
  は既定。
- 切替の非破壊: comfy にカスタム URL を書き → openai へ切替 → comfy へ戻すと URL が残る。
  切替直後の実効 `base_url` は新プロバイダのスロット値（前プロバイダの値が漏れない —
  動機 1 の再現ケースを Red で固定）。
- `toBackendConfig` の写像凍結: 空スロットの `DEFAULT_BASE_URL`/`DEFAULT_MODEL` フォール
  バック / **openai・gemini スロットに workflowJson・negative が残っていても wire に
  乗らない**（動機 1 の逆向き漏れ） / `timeoutSecs: 0` は `null`（backend 既定に倒す）。
- harness: tags 形式指示に年齢・体格の要求が含まれる（文言固定、prose は不変・byte 一致）。

## 査読反映記録

### rev2（2026-08-21 ユーザー査読）

| # | 指摘 | 反映 |
|---|---|---|
| 1 | 「分離は frontend のみ」と harness 修正が矛盾 | 決定 1 を「設定の分離は frontend のみ + 付随改善として harness 文言変更（signature 不変）」へ精密化 |
| 2 | `toBackendConfig` の写像が未定義（workflowJson/negative の逆向き漏れ・timeout 既定） | 何を作るか 3 に写像を凍結。negative/workflowJson は comfy 以外で送らない（現行コードのゲートを明文化）。timeout 既定は backend 既存 `timeout()` が受ける — frontend に既定表を複製しない（**査読案からの divergence**: 未定義でなく backend に定義済みだった） |
| 3 | 移行の「既定で埋める」と未決 1「style 複製するか」が二重定義 | 未決 1 を廃し、何を作るか 4 に確定を畳み込み（現 provider のみ・明示 tags も複製しない） |
| 補 1 | `enabled` 共有の制約が暗黙 | 何を作るか 1 に制約 note（プロバイダ別 ON/OFF は切替で表現）+ スコープ外に明記 |
| 補 2 | 新プロバイダ追加時の部分欠損が未定義 | 何を作るか 4 に「欠けたスロットは既定で埋める」を追記 + PoC |
| 補 3 | userPrefix のタグ語彙バイアスは言い換え後も残りうる | 決定 2 に「散文推奨」を追記し UI placeholder/ヘルプへ出す |
