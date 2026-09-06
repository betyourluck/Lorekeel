# spec 29: AI 編集 — 編集モードで 1 ファイルずつ、道具で直させる

**Status**: Draft（2026-09-06 起草、ユーザー発案）。spec 28「次の一手」3 を回収する。
起草時点の実装ゼロ。査読待ち。

---

## 動機（ユーザーの言葉から）

> package.yaml の場合は package.yaml の編集方法だけ LLM に渡して直させる。character の yaml の
> 場合は character の書き方の情報を渡して直させる。一気に作らせようとしないで 1 ファイルずつ。
> 今のように package_spec.md（全部）だと誤差が多い。新規ファイルは作らせなくていい。
> diff / grep / read / sd みたいなのが使えればいい。Fuseforks のツールのコードを流用しても良い。

配布サイトの `/guide` が案内する現行の道は「`package_spec.md`（66 KB）をまるごと LLM に渡して
パッケージを一発生成させ、`play lint` かアプリの開幕 ⚠ で検める」だった。誤差の源は 2 つ:

1. **仕様の量**。66 KB のうち scenario 節が 41 KB（62%）で、package.yaml を書くだけの依頼にも
   trigger・challenge・contest・人狼盤面の規則が全部乗る。モデルは無関係な節の語彙を引いて
   幻のキー・幻の参照を書く（`stats` を `stat` と書く、`entity` を `move` に添える、の類）。
2. **一発生成**。複数ファイルを 1 応答で出させると、出力上限で途中が切れ、ファイル間の
   参照（cast ↔ characters、globals.flags ↔ flag_rules）が片側だけ書かれる。

編集モード（spec 28）は既に **ファイル種別ごと**に診断（`lint_editor_text(kind, text)`）・
補完（型から導出した既知キー + 各欄の doc comment）・id 収集（他ファイルの場所・人物・
フラグ・アイテム・アセット）を持っている。AI 編集はこの上に「1 ファイルを対象に、道具で
読み・探し・置換し、置換のたびに同じ診断を返す」ループを 1 本足すだけで成立する。
**新しい検査規則はゼロ**（spec 28 / `play lint` と同じ収縮の判断）。

## 決定（設計の核）

1. **編集のみ**。作成・削除・リネームは道具に無い（ユーザー決定）。対象は編集モードの
   ファイル一覧（各フォルダ直下の YAML）の**閉集合**で、指示の前に一覧を見せる。
   Fuseforks failures #39（作れない道具に作らせようとして 12 周空転）の教訓を先取りし、
   道具の失敗文言は「できない理由 + 代わりの手段（作者が作る）」を必ず書く。
2. **1 リクエスト = 対象ファイル 1 つ + 作者の指示 1 つ**。書き換えられるのは対象だけ。
   **読む・探すはパッケージ内の他 YAML にも許す**（参照先の id を確かめるため。誤差の源は
   「一気に書く」ことであって「読む」ことではない）。
3. **道具は 4 本**: `read` / `grep` / `sd` / `diff`。Fuseforks の `SdTool` / `GrepTool` /
   `DiffTool` / `FileTool(read)` の写経（後述）。`sd` は**既定 preview・`apply: true` で確定**
   （差分を見てから書く、を道具の形で強制）。
4. **LLM はディスクに触らない**。`sd apply` の書き込み先は backend が持つ**作業バッファ**
   （対象ファイルの本文のコピー）で、ループが終わったら本文を frontend へ返し、CodeMirror の
   バッファへ**未保存（●）として**入れる。保存するのは作者で、書庫版ならフォーク確認が
   そのまま効く（spec 28 B）。取り消しはエディタの undo 1 回（AI の差し替えは 1 トランザクション）。
5. **置換のたびに診断を返す**。`sd apply` の結果には `lint_editor_text(kind, 新本文)` の
   診断（parse エラーの行 / 未知キーのパス + 近い既知キー）を添える。モデルは自分の置換が
   壊したものをその場で見る = ターンループの self-repair と同じ輪。
6. **渡す仕様はファイル種別ごとの断片** + **型から導出した既知キーと説明**。断片は手書き
   （意味の話）、キーと説明は機械導出（補完と同じ表 = 「補完に出るのに LLM に教えていない」
   乖離が起きない）。断片の正本は **Kataribe リポジトリ**に置き、配布サイトの
   `package_spec.md` は断片の連結として作る（後述「仕様断片の置き場」）。
7. **停止性**: 周回上限（既定 12）/ 同一呼び出しの反復検知（同じ `sd` を 2 度 = 停止）/
   道具結果の大きさ上限 / キャンセル。Fuseforks の `max_tool_iterations` + repeat guard の写し。
8. **モデルは GM の設定と同じ**（`LLM_*`）。別プロファイル（`EDITOR_LLM_*`）は要ると
   分かってから（あらすじの `SUMMARY_LLM_*` と同じ後付けができる形）。

## 前提の訂正 — llm_client に tool の往復が無い

`crates/llm_client` の canonical は `Role::Tool` を語彙として持つが、**tool 結果を返送する
欄が無い**（`ChatMessage` に `tool_call_id` も assistant 側の `tool_calls` も無く、4 adapter は
`Role::Tool` を user へ降格している）。Kataribe の tool-use は `emit_delta` 1 本を強制して
1 往復で終わる形だけだったので不要だった。道具を持たせるにはここを先に足す。

Fuseforks の `llm/canonical.rs` は同じ形で、`ChatMessage { tool_calls, tool_call_id, tool_name }`
+ `assistant_tool_calls()` / `tool_result()` と、openai_compat / anthropic / gemini / responses の
4 adapter の encode を持つ。**これを写経する**（Phase A）。既存経路の encode が 1 バイトも
変わらないことを PoC で固定する（tool 欄が空のメッセージは従来と同一 = キャッシュ無風）。

**Fuseforks は MPL-2.0**（Kataribe は MIT）。作者が同一なので流用に法的な支障は無いが、
写経したファイルには出所を doc comment に書く（どちらのリポジトリを直したかを後から
辿れるように。Fuseforks 側の修正を Kataribe に写す作業は今後も起きる）。

`ToolMode::Off` のプロバイダ（tools を送れないサーバ）では AI 編集を**出さない**
（「このモデルはツール呼び出しに対応していません」）。JSON フォールバックで道具の往復を
偽装する経路は v1 では作らない — 往復のたびに構文解析の失敗が重なり、誤差を減らす目的と逆。
Meta / Perplexity の Responses 口は `tool_choice` を強制できないが、ここで要るのは `Auto`
なので問題ない（#80 の癖はこの用途では踏まない）。

## 何を作るか

### A. 道具 4 本（app backend、Fuseforks 写経）

| 道具 | 引数 | 返り | 制約 |
|---|---|---|---|
| `read` | `path`（省略 = 対象ファイル） | 行番号つき全文 | パッケージ内の一覧に在る YAML のみ。大きさ上限 |
| `grep` | `pattern`, `path?`, `case_insensitive?`, `context?(0-3)`, `count_only?` | `パス:行: 内容` | 同上。Rust regex |
| `sd` | `pattern`, `replacement`, `apply?` | preview = unified diff / apply = 差分 + **診断** | **対象ファイルだけ**（`path` は受けない） |
| `diff` | なし | 対象の**ディスク上の本文と作業バッファ**の unified diff | 累積の編集を確かめる用 |

Fuseforks からの差分: `sd` の `paths`（複数 preview）と `case_insensitive` 以外の拡張は
持ち込まない / `diff` は 2 ファイル比較でなく「ディスク vs バッファ」/ `file` は `read` だけ
（write・append・mkdir・move・copy・remove は**持ち込まない** = 決定 1）/ パスの containment は
Kataribe の編集ルート（spec 28 A の相対パス + canonicalize）を再利用し Fuseforks の
`resolve_in_work_dir` は写さない。読める対象は一覧の YAML だけ（`.lorekeel_source.json`・
`.env`・画像は読めない）。

**失敗文言**（#39）: 一覧に無いパスは「`{path}` は編集対象の一覧にありません。AI 編集は
既存ファイルの書き換え専用で、新しいファイルは作れません（作るのは作者です）。一覧: …」。
`sd` の不一致は「`{pattern}` は対象ファイルに一致しません（`read` で現在の本文を確かめる）」。

### B. ループ（harness に純関数、app に薄い IO）

- `harness::edit_assist::build_request(kind, target_rel, target_text, instruction, vocabulary, slices)`
  → `system`（役割 + 規律 + 仕様断片 + 既知キー表 + 他ファイルの id 一覧 + 編集対象一覧）と
  `user`（対象の本文 + 指示）を組む純関数。**素材に何が入るかを PoC で固定**（挿絵の
  プロンプト書きと同型: `hidden_*` のような秘密は無いが、`.env` や他パッケージが入らない
  ことを byte で固定する）。
- app の command `edit_assist_run(target_rel, instruction)` が LLM 往復を回す:
  `ToolChoice::Auto` + 道具 4 本 → 応答の `tool_calls` を実行 → `tool_result` を積む → 繰り返し。
  `tool_calls` が空になったら終了（本文の assistant メッセージ = 作者への報告）。
- 終了時に `lint_editor_text` をもう一度走らせ、error が残っていれば**1 周だけ**追加で
  「まだ壊れている: …」を投げる（上限の内側）。それでも残れば診断つきで返す
  （直せなかったことを黙らない）。
- 返り: `{ text: 新本文, changed: bool, summary: 報告文, calls: [{tool, args_brief, ok}],
  diagnostics: [...], usage: {prompt, completion, cache_read}, stopped: null | "limit" | "repeat" | "cancel" }`。

### C. 仕様断片（Kataribe リポジトリを正本に）

`docs/package_spec/` に断片を置く（ファイル種別 × 話題）。現行 `package_spec.md` を
節で割った文字数（2026-09-06 実測）:

| 断片 | 文字数 | 渡す条件 |
|---|---|---|
| 大原則 + 構造 | 1,802 | 常に |
| package.yaml | 1,734 | kind = manifest |
| ┗ facts_policy / image_style | 2,275 / 1,772 | manifest で指示か本文にそのキーが出るとき |
| scenario 本体（場所・出口・アイテム・フラグ・ゴール・エピローグ・トリガー・challenge 基本） | 7,059 | kind = scenario、常に |
| ┗ Gate 語彙 13 種 | 1,802 | scenario、常に |
| ┗ effects の op 語彙 | 1,334 | scenario、常に |
| ┗ image_hold / max_per_turn / entity+threshold / percentile / 確定行動 / expr / プッシュ / contests / `"*"` / presence_is / party / volatile / トリガー→判定 / トリガー→移動 / 可視性 / 人狼 | 1,138〜3,472（計 約 31,000） | scenario で**指示か本文に該当キーが出るとき**だけ |
| characters | 1,017 | kind = character |
| memoria | 451 | kind = memoria |
| campaign | 747 | kind = campaign |
| 作法 / ロード時エラー | 4,973 / 3,925 | 渡さない（診断が機械で返す。作法は作者の領分） |

scenario の**常に渡す量は約 10 KB**（本体 + Gate + op）で、41 KB から 4 分の 1 になる。
話題断片の選択表（`(kind, 指示の語, 本文のキー) → 断片`）は**手で持つ**（spec 28 v2 の
「値カテゴリの写像」と同じく意味の話で、型から導けない）。誤って落とした話題は診断が
未知キーとして拾うので、選択の漏れは「LLM が知らずに書けない」側にしか倒れない。

**配布サイトの `package_spec.md` は断片の連結で作る**。連結順を 1 本のリスト（`docs/package_spec/index`）
で持ち、`docs/package_spec.md`（連結済み）を Kataribe に置いて outcast へ写す（写しは
今までどおりユーザー作業）。連結済みと断片の一致は Kataribe 側のテストで固定する。
これで「アプリ同梱 / サイト / 手元」の三写しにならず、断片が唯一の正本になる。
本 spec の実装で最初にやるのは**現行 66 KB をこの表に沿って割ること**（内容は 1 字も変えない）。

### D. 既知キーと説明（機械導出、常に）

`editor_vocabulary` が補完のために持つ `contexts`（kind ごとの既知キー + doc）と `ids`
（他ファイルから集めた場所・人物・フラグ・アイテム・アセット）を、system に表として載せる:
「このファイルで書けるキー（型から導出。**これ以外は未知キーとして警告される**）」
「参照できる id（**これ以外は死んだ参照**）」。値の語彙（Gate の 13 種、op の一覧）も
`gate_variant_keys` / `op_variant_keys` から出す。**手書きの表を LLM 向けに新設しない**。

### E. UI（frontend）

- エディタヘッダに「AI に直させる」（対象ファイルを開いているとき。卓中・ゲスト・
  `ToolMode::Off` は disable + 理由）。
- 浮遊パネル（`FloatingPanel`、プロンプト工房と同じ流儀）: 指示欄（自由文、揮発）/
  実行 / キャンセル / 進行ログ（`read scenarios/main.yaml` `grep flag_is` `sd (preview)`
  `sd (apply) ✓ 診断 0` … の 1 行ずつ、`synopsis-compacting` と同じ push イベント）/
  終了後は報告文 + 「本文を差し替えました（未保存）」。
- 実行中はエディタを読み取り専用（作業バッファと作者の編集が競合しない = Fuseforks の
  read-modify-write の穴を、**同時に書く者を一人にする**ことで塞ぐ）。
- 差し替え後の ● と保存はエディタの既存経路。undo で戻る。

## 素材と秘密

送るのは「対象ファイルの本文 + 指示 + 仕様断片 + 既知キー表 + 他 YAML の id + 道具で読んだ
本文」だけ。**作者のパッケージは作者のもの**なので `hidden_*` の区別は無いが、
編集ルートの外（`.env`・他パッケージ・セーブ・参照画像）に道具が届かないことを
containment のテストで固定する。

## スコープ外（v1）

- 新規ファイル作成・削除・リネーム・メディア（ユーザー決定。作るのは作者）
- 複数ファイルを 1 依頼で直す（ファイルをまたぐ整合は作者が順に依頼する。順の目安は
  package.yaml → characters → scenario、パネルに 1 行で示すだけ）
- 層 2 診断（`inspect_package` = ファイル間の死んだ参照）のループ内反映。ディスクに無い
  本文を検分するには overlay が要る。v1 は保存時に従来どおり報告する
- 既刊モジュールの取り込み（spec 28「次の一手」3 の用途 2。本 spec のループが土台になるが
  入力が PDF/Markdown で別の話）
- 別モデルプロファイル / ストリーミング表示 / 会話の継続（毎回 1 依頼で完結。前回の
  やり取りは積まない — 積むとファイルをまたぐ「一気に」へ戻る）

## Phase 分割

- **Phase A（llm_client: tool の往復）**: canonical に `tool_calls` / `tool_call_id` / `tool_name` +
  4 adapter の encode/decode（Fuseforks 写経）。PoC: 既存経路の encode が byte 一致 /
  各 adapter で assistant(tool_calls) → tool(result) の対が wire に出る / Gemini の id 合成と
  `functionResponse` / Responses の `function_call_output`。
- **Phase B（仕様断片）**: 66 KB を `docs/package_spec/` へ割る（内容不変）+ 連結テスト +
  連結済み `docs/package_spec.md`。outcast への写しはユーザー作業。
- **Phase C（道具 + ループ）**: 道具 4 本（app backend、containment は編集ルート再利用）+
  `harness::edit_assist`（純関数）+ command。PoC: 素材の byte 固定 / 一覧外パスの拒否と
  文言 / `sd` の対象限定 / 診断が結果に載る / 周回上限と反復検知で止まる / fake クライアントで
  「read → sd preview → sd apply → 終了」の往復。
- **Phase D（UI）**: ヘッダのボタン + 浮遊パネル + 進行ログ + 読み取り専用 + 差し替え。
  目視はユーザー実測（提示層は構造的にユーザーが検出器になる）。
- **Phase E（実測）**: 同梱 4 パッケージに対して「主人公の hp を 12 にして」「湖畔に SAN 判定の
  challenge を 1 つ足して」「モカの profile に趣味を足して」の 3 依頼を 3 モデル（Claude /
  Gemini / Meta）で回し、**未知キー 0・死んだ参照 0・周回数・トークン**を測る。
  核心的未知 = 弱いモデルが `sd` の正規表現を書けるか（書けなければ「行番号で置換」の
  第 5 の道具を足す。#39 と同じく能力を足す側で解く）。

## PoC 方針

- 検査規則は増やさない。増えるのは「道具の境界」「素材の形」「停止性」「wire の形」の 4 つ。
- fake クライアント（`DeltaProposer` の ScriptedProposer と同型）でループ全体を実 API なしで固定。
- live は `--ignored` 1 本（Phase E の 3 依頼のうち 1 つ）。

## 未決

1. `sd` だけで足りるか（大きなブロックの追加は「アンカー行の置換」で書けるが、弱いモデルには
   重い）。Phase E で「行番号 + 挿入」の道具が要るか決める。
2. 診断 error が残ったまま終わったとき、本文を差し替えるか否か。案: 差し替えて赤線を見せる
   （作者が直せる状態にする方が、AI の作業を捨てるより安い）。
3. 話題断片の選択を「指示の語 + 本文のキー」で決める表の初期値。実 content 4 本で偽陰性が
   出るかを Phase B で見る。
4. ループの進行ログをどこまで見せるか（`sd` の差分全文は長い。1 行要約 + クリックで展開）。

## 波及

- `CLAUDE.md` 現状節 / `data_contract.yaml`（`ai_edit_assistant` 節を新設: 道具 4 本の契約・
  素材・停止性 / `UnifiedToolLayer` に tool 往復の翻訳行を追記）
- `specs/12_unified_tool_layer.md`: canonical の拡張（tool 往復）を追記
- `specs/28_package_editor.md`: 「次の一手」3 → 本 spec へ
- outcast `package_spec.md`: **形式は不変**。ただし**正本の置き場が Kataribe の断片へ移る**
  （Phase B 以降、サイト側は連結済みの写し）
- `docs/`（Qiita 用）とは別に `docs/package_spec/` を新設
