# spec 29: AI 編集 — 編集モードで 1 ファイルずつ、道具で直させる

**Status**: rev2 → **Phase A 実装済（2026-09-06、査読の着手指示どおり）**。2026-09-06 起草 →
同日査読 2 本（重大 7 + 曖昧 5 + 軽微 4 / 矛盾 7）を反映 → Phase A: canonical に tool の往復欄
+ 4 adapter の encode（Fuseforks 写経）+ `LlmClient::chat` + `EDITOR_LLM_*`（`profile_from_env`
に一般化）。PoC: **golden 10 本 = 改修前のコードで採取**した wire のバイト列に対する同一性
（4 adapter × forced/auto/plain）+ 往復 5 本（4 adapter の tool_calls/tool_result の形・Gemini
id なし・EDITOR プロファイルの継承）。llm_client 68 → 77、workspace 381、clippy clean。
査読の反映は末尾「査読の反映」節に凍結。spec 28「次の一手」3 を回収する。
**Phase B 実装済（同日）**: `docs/package_spec/` 30 断片 + `index.yaml`（連結順・base・話題 19 本 =
辞書の初期値）+ `harness::package_spec`（include_str! で焼く / 型から機械生成した Gate・op の列挙 /
連結）+ `play spec-vocab` / `play spec-assemble` + doc 抽出器を app から harness へ移設
（`harness::docs`）。サイト版 `docs/package_spec.md` は連結の写しで、鮮度はテストが固定。
PoC 3 本（索引と焼き込みの 1:1 / 列挙が型の全バリアントを含む / 連結の鮮度）。harness 144、
app backend 82（doc テスト 2 本が harness へ）、workspace 386。
**Phase C 実装済（同日）**: `harness::edit_assist`（素材の組み立て・話題選択・道具 5 本の宣言・
ループ = ToolChat / ToolExecutor で依存性逆転）+ app `edit_assist::EditSession`（道具の実体、
Fuseforks 写経）+ command `edit_assist_run` / `edit_assist_cancel` + `edit-assist-progress`
イベント（中継表に追加）。PoC: harness 9 本（素材の固定と話題の選択 / 本文キーからの選択と
1 文字語 / 道具の宣言 / 往復の順序と報告 / 反復検知 / 上限 / 修復 1 周 / 直らなくても差し替え /
キャンセル）+ app 5 本（一覧外の拒否と #39 文言・バッファとディスクの読み分け・grep / sd の
状態機械と全置換と diff / 診断が返りに載る / spec 道具 / 配線で辿る表）+ live 1 本（ignored）。
harness 153、app backend 87、workspace 395。
**Phase D 実装済（同日、GUI 目視はユーザー実測待ち）**: エディタヘッダに ✨（開いているファイルが
あるときだけ・開いていれば熾火・実行中は spinner）→ `EditAssistPanel`（`FloatingPanel` の器・
動的 import = 3.6 KB の別 chunk）: 指示欄（揮発・Ctrl+Enter）/ 実行・取り消し / 進行ログ
（`edit-assist-progress` を 1 行ずつ）/ 報告・差し替えの有無・打ち切り理由・残った診断・
モデルと周回とトークン。実行中は CodeMirror が読み取り専用（`readonly` prop = Compartment）。
差し替えは store が `editor.text` へ入れる → `CodeEditor` の watch が **1 回の dispatch** で
全置換 = undo 1 回で戻る（E 節の要件は既存の watch が満たしていた）。実行中にファイルを
切り替えていたら結果は捨てる（別ファイルの本文を上書きしない）。vue-tsc / vitest 53 / build 緑。
Phase E（3 モデル × 3 依頼の実測）は未着手。

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

## 用語（rev2 で分離 — 本 spec 内で一貫して使う）

- **ディスク**: 対象ファイルの保存済み本文。
- **初期バッファ**: AI 編集を開始した瞬間の**エディタの本文**（作者の未保存変更を含む）。
  frontend が command に渡す。ディスクから作らない（作ると作者の未保存変更が先祖返りする）。
- **作業バッファ**: backend が持つ対象ファイルの本文。初期値 = 初期バッファ。`sd apply` の
  書き込み先。ループ終了時に frontend へ返る。
- **対象**: 今回書き換えてよい唯一のファイル。**他ファイル** = 同じパッケージの一覧に在る他の YAML。

## 決定（設計の核）

1. **編集のみ**。作成・削除・リネームは道具に無い（ユーザー決定）。対象は編集モードの
   ファイル一覧（各フォルダ直下の YAML）の**閉集合**で、system に一覧を載せる。
   Fuseforks failures #39（作れない道具に作らせようとして 12 周空転）の教訓を先取りし、
   道具の失敗文言は「できない理由 + 代わりの手段（作者が作る）」を必ず書く。
2. **1 リクエスト = 対象 1 つ + 作者の指示 1 つ**。書き換えられるのは対象だけ。
   **読む・探す**は同じ閉集合（一覧の YAML）の中で対象以外にも許す（参照先の id を確かめる
   ため。誤差の源は「一気に書く」ことであって「読む」ことではない）。読める集合と
   編集できる集合は**同じ一覧**で、違いは「書けるのが対象だけ」という一点。
3. **道具は 5 本**: `read` / `grep` / `sd` / `diff` / `spec`。前 4 本は Fuseforks の写経、
   `spec` は仕様断片を LLM が自分で引く読み取り専用の道具（rev2、査読 2 の 4）。
   `sd` は **preview を先に通さない `apply` をコードで拒否**する（決定 3 の「強制」は
   既定値ではなく状態機械。査読 1 の 2 / 査読 2 の 3）。
4. **LLM はディスクに書き込まない**（読むのは `read`/`grep` がディスクを読む）。`sd apply` の
   書き込み先は作業バッファで、ループが終わったら本文を frontend へ返し、CodeMirror の
   バッファへ**未保存（●）として**入れる。保存するのは作者で、書庫版ならフォーク確認が
   そのまま効く（spec 28 B）。取り消しはエディタの undo 1 回（差し替えは `dispatch` 1 回の
   単一トランザクション。E 節に実装を明記）。
5. **対象の `read`/`grep` は作業バッファを読む**。他ファイルはディスク。`sd apply` の直後に
   `read` で結果を確かめる経路が成立する（査読 1 の 1 / 査読 2 の 2）。
6. **置換のたびに診断を返す**。`sd apply` の結果には `lint_editor_text(kind, 作業バッファ)` の
   診断（parse エラーの行 / 未知キーのパス + 近い既知キー）を添える。モデルは自分の置換が
   壊したものをその場で見る = ターンループの self-repair と同じ輪。
7. **渡す仕様はファイル種別ごとの断片** + **型から導出した既知キー表と語彙**。断片は手書き
   （意味と例）、キー名と語彙の**列挙**は機械導出だけが持つ（断片に列挙表を置かない =
   乖離したとき正は型。査読 1 の 10 / 査読 2 の 5）。断片の正本は **Kataribe リポジトリ**に
   置き、配布サイトの `package_spec.md` は断片 + 生成した列挙の連結として作る（C 節）。
8. **停止性**: LLM 呼び出しの周回上限（既定 **20** — 起草時 12、実測で改訂 = 下の「CLI」節）。上限の最後の 1 周は道具なしの**まとめ**。**上限の外に修復 1 周**（20 + 1 と定義。
   査読 1 の 3）。反復検知 = **(道具, 引数全体, 結果) が直前と同一**なら停止（preview→apply は
   引数が違うので誤検知しない。査読 2 の 3）。道具結果の大きさ上限。キャンセル。
9. **診断 error が残って終わっても本文は差し替える**（未保存● + 赤線）。`changed: true`・
   `diagnostics` 添付・報告文に「直せなかった」を書く。作業を捨てるより、作者が直せる
   状態に置く方が安い（査読 1 の 7 / 査読 2 の 6 で確定。旧・未決 2 は撤去）。
10. **モデルは既定で GM の設定**（`LLM_*`）。**`EDITOR_LLM_*`（base_url / api_key / model /
    provider）を v1 に入れる** — あらすじの `SUMMARY_LLM_*`（`summary_from_env`）と同じ機構の
    再利用で、未指定フィールドは GM 設定を継承。GM が `ToolMode::Off`（tools を送れない
    サーバ）のとき AI 編集は disable し、文言で `EDITOR_LLM_*` を案内する（査読 1 の 5）。

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

JSON フォールバックで道具の往復を偽装する経路は v1 では作らない — 往復のたびに構文解析の
失敗が重なり、誤差を減らす目的と逆。Meta / Perplexity の Responses 口は `tool_choice` を
強制できないが、ここで要るのは `Auto` なので問題ない（#80 の癖はこの用途では踏まない）。

## 何を作るか

### A. 道具 5 本（app backend）

| 道具 | 引数 | 読む先 | 返り | 制約 |
|---|---|---|---|---|
| `read` | `path?`（省略 = 対象） | 対象 = 作業バッファ / 他 = ディスク | 行番号つき全文 | 一覧の YAML のみ。大きさ上限 |
| `grep` | `pattern`, `path?`, `case_insensitive?`, `context?(0-3)`, `count_only?` | 同上（対象だけバッファ） | `パス:行: 内容` | 同上 |
| `sd` | `pattern`, `replacement`, `apply?`, `case_insensitive?` | 作業バッファ | preview = unified diff + 一致数 / apply = 差分 + **診断** | **対象だけ**（`path` を受けない）。**apply は直前の同引数 preview 成功が前提** |
| `diff` | なし | 初期バッファ vs 作業バッファ | unified diff | AI の累積差分だけが出る（作者の未保存分は初期バッファに含まれるので混ざらない） |
| `spec` | `topic` | 仕様断片（同梱） | 断片の本文 | system に載る話題一覧の id のみ。ファイルシステムに触れない |

Fuseforks からの差分（rev2 で書き直し）: `sd` の `paths`（複数 preview）は持ち込まない /
`case_insensitive` は `grep` と対称に持つ / `diff` は 2 ファイル比較でなく「初期 vs 作業」/
`file` は `read` だけ（write・append・mkdir・move・copy・remove は**持ち込まない** = 決定 1）/
パスの containment は Kataribe の編集ルート（spec 28 A の相対パス + canonicalize）を再利用し
Fuseforks の `resolve_in_work_dir` は写さない。読める対象は一覧の YAML だけ
（`.lorekeel_source.json`・`.env`・画像・`docs/` は読めない。仕様断片は `spec` で引く）。

**`sd` の正規表現**（rev2、査読 1 の 11）: Rust `regex` crate（lookaround・後方参照なし）。
置換文字列は `$1` / `$name` でキャプチャ参照、リテラル `$` は `$$`。**一致は全部置換**
（件数を返す）。`(?m)` で `^`/`$` を行頭行末に、`(?s)` で `.` を改行に当てられる = 複数行の
ブロック置換はこの 2 つで書く。preview の返りに一致数を必ず載せる（0 件 = 「一致しません。
`read` で現在の本文を確かめる」、意図より多い件数に気づける）。

**`sd` の状態機械**: backend がセッション内に `last_preview: Option<(pattern, replacement,
case_insensitive)>` を持ち、`apply: true` は `last_preview` と一致するときだけ通す。不一致は
「先に同じ引数で preview を通してから apply する」と返し、書き込まない。preview 成功で
`last_preview` を更新、apply 成功で消す。

**失敗文言**（#39）: 一覧に無いパスは「`{path}` は編集対象の一覧にありません。AI 編集は
既存ファイルの書き換え専用で、新しいファイルは作れません（作るのは作者です）」。
一覧そのものは system に載せているので失敗文言には繰り返さない（査読 1 軽微 3）。

### B. ループ（harness に純関数、app に薄い IO）

- `harness::edit_assist::build_request(kind, target_rel, initial_text, instruction, vocabulary,
  topics)` → `system`（役割 + 規律 + 種別の基本断片 + 既知キー表 + 語彙列挙 + 他ファイルの
  id 一覧 + 編集対象一覧 + `spec` で引ける話題一覧と 1 行要旨）と `user`（対象の本文 + 指示）を
  組む純関数。**素材に何が入るかを PoC で固定**（harness 側は「渡した引数以外の文字列が
  出ない」、containment は app 側で別に固定 — 査読 1 軽微 2）。
- app の command `edit_assist_run(target_rel, initial_text, instruction)`（**初期バッファは
  frontend が渡す** — 査読 2 の 1）が LLM 往復を回す: `ToolChoice::Auto` + 道具 5 本 →
  応答の `tool_calls` を実行 → `tool_result` を積む → 繰り返し。`tool_calls` が空になったら
  終了（本文の assistant メッセージ = 作者への報告）。
- 終了時に `lint_editor_text` をもう一度走らせ、error が残っていれば**上限の外の修復 1 周**で
  「まだ壊れている: …」を投げる。それでも残れば診断つきで返す（決定 9）。
- 返り: `{ text: 作業バッファ, changed: bool（初期バッファと異なるか）, summary: 報告文,
  calls: [{tool, args_brief, ok}], diagnostics: [...], usage: {prompt, completion, cache_read},
  stopped: null | "limit" | "repeat" | "cancel" }`。
- 進行は `edit-assist-progress` イベント（1 呼び出し 1 行）。**backend の emit 名 ⊆ frontend
  `GAME_EVENTS`** は既存テスト（failures #98）が固定する。

### C. 仕様断片（Kataribe リポジトリを正本に）

`docs/package_spec/` に断片を置く（ファイル種別 × 話題）。現行 `package_spec.md` を
節で割った文字数（2026-09-06 実測）:

| 断片 | 文字数 | 渡し方 |
|---|---|---|
| 大原則 + 構造 | 1,802 | 常に |
| package.yaml | 1,734 | kind = manifest で常に |
| ┗ facts_policy / image_style | 2,275 / 1,772 | `spec` で引く（manifest の話題） |
| scenario 本体（場所・出口・アイテム・フラグ・ゴール・エピローグ・トリガー・challenge 基本） | 7,059 | kind = scenario で常に |
| ┗ Gate の意味と例 / effects の op の意味と例 | 1,802 / 1,334 → **列挙を抜いて縮む** | scenario で常に（列挙は D が出す） |
| ┗ image_hold / max_per_turn / entity+threshold / percentile / 確定行動 / expr / プッシュ / contests / `"*"` / presence_is / party / volatile / トリガー→判定 / トリガー→移動 / 可視性 / 人狼 | 1,138〜3,472（計 約 31,000） | `spec` で引く（scenario の話題） |
| characters | 1,017 | kind = character で常に |
| memoria | 451 | kind = memoria で常に |
| campaign | 747 | kind = campaign で常に |
| 作法 / ロード時エラー | 4,973 / 3,925 | 渡さない（診断が機械で返す。作法は作者の領分） |

**常時量**（rev2 で訂正、査読 1 の 6）: scenario = 1,802 + 7,059 + Gate/op の意味と例
（列挙を抜くので 3,136 未満）≒ **12 KB 弱**（41 KB から 3 分の 1 以下）。manifest ≒ 3.5 KB、
character ≒ 2.8 KB。

**話題断片は 2 経路で届く**（rev2、査読 2 の 4）:
1. **辞書による先出し** — `(kind, 指示の語 or 本文のキー) → 話題` の表を手で持つ
   （「SAN」「判定」「d100」→ percentile、「対決」「戦闘」→ contests、「登場」「同行」→
   volatile …）。当たった話題は最初から system に載せる。表は意味の話で型から導けない。
2. **`spec` 道具** — system に「引ける話題の一覧と 1 行要旨」を載せ、モデルが自分で引く。
   辞書に無い言い回し（「幸運を消費する仕組み」）や、**本文にまだキーが無い追加依頼**で
   効く。辞書の偽陰性が「知らずに書けない」で止まらず、モデルが取りに行ける。
辞書は最適化、`spec` が正しさの経路。辞書に当たらず `spec` も引かずに未知キーを書いた
ときは診断が拾い、修復周で `spec` を引く動機になる（診断文言に「関連する話題: …」を添える）。

**サイトの `package_spec.md` は連結で作る**。`docs/package_spec/index` の順で断片を連結し、
Gate/op の**列挙**は `play spec-vocab`（新設、`editor_vocabulary` と同じ型導出から Markdown
表を出す）の出力を所定位置に差し込む。連結済み `docs/package_spec.md` を Kataribe に置き、
outcast へ写す（写しは今までどおりユーザー作業）。**連結済み = 連結(断片, 生成列挙)** は
Kataribe 側のテストで固定。これで「アプリ同梱 / サイト / 手元」の三写しにならず、断片が
唯一の正本になる。本 spec の実装で最初にやるのは**現行 66 KB をこの表に沿って割ること**
（列挙を抜く以外、内容は 1 字も変えない）。

### D. 既知キーと語彙（機械導出、常に）

`editor_vocabulary` が補完のために持つ `contexts`（kind ごとの既知キー + doc）と `ids`
（他ファイルから集めた場所・人物・フラグ・アイテム・アセット）を、system に表として載せる:
「このファイルで書けるキー（型から導出。**これ以外は未知キーとして警告される**）」
「参照できる id（**これ以外は死んだ参照**）」。Gate の 13 種と op の一覧は
`gate_variant_keys` / `op_variant_keys` + 各バリアントの doc comment から出す（**列挙の正本は
ここだけ**。C の断片は意味と例）。**手書きの表を LLM 向けに新設しない**。

**id の鮮度**（rev2、査読 1 の 9）: 対象の id は作業バッファから、他ファイルはディスクから
集める。エディタは**単一バッファ**（ファイル切替時に未保存確認 = spec 28 A.7）なので、
他ファイルは常に保存済み = overlay は要らない。「package.yaml → characters → scenario」の順に
依頼する運用は、各ファイルを保存してから次へ進む形になる（保存はどのみち作者の手）。

### E. UI（frontend）

- エディタヘッダに「AI に直させる」（対象ファイルを開いているとき。卓中・ゲスト・
  `ToolMode::Off` は disable + 理由。Off のとき「GM の設定はツール呼び出しに対応して
  いません。`EDITOR_LLM_*` で別のモデルを指定してください」）。
- 浮遊パネル（`FloatingPanel`、プロンプト工房と同じ流儀）: 指示欄（自由文、揮発）/
  実行 / キャンセル / 進行ログ（`read scenarios/main.yaml` `grep flag_is (3)` `sd preview (2 hits)`
  `sd apply ✓ 診断 0` `spec percentile` … の 1 行ずつ。差分全文は行をクリックで展開 —
  旧・未決 4 の決着）/ 終了後は報告文 + 「本文を差し替えました（未保存）」+ 残った診断。
- 実行中はエディタを読み取り専用（初期バッファを渡した後に作者が編集すると、差し替えで
  その編集が消える。**同時に書く者を一人にする**ことで Fuseforks の read-modify-write の穴を
  塞ぐ）。キャンセルは backend の周回境界で効く（飛行中の 1 呼び出しは完走）。
- **差し替えは `view.dispatch({ changes: { from: 0, to: doc.length, insert: text },
  userEvent: "ai.replace" })` の 1 回** = undo 1 回で戻る（rev2、査読 1 の 12）。

## 素材と秘密

送るのは「対象の本文 + 指示 + 仕様断片 + 既知キー表 + 語彙列挙 + 他 YAML の id + 道具で読んだ
本文」だけ。**作者のパッケージは作者のもの**なので `hidden_*` の区別は無いが、
編集ルートの外（`.env`・他パッケージ・セーブ・参照画像）に道具が届かないことを
containment のテスト（app 側）で固定する。

## スコープ外（v1）

- 新規ファイル作成・削除・リネーム・メディア（ユーザー決定。作るのは作者）
- 複数ファイルを 1 依頼で直す（ファイルをまたぐ整合は作者が順に依頼する。順の目安は
  package.yaml → characters → scenario、パネルに 1 行で示すだけ）
- 層 2 診断（`inspect_package` = ファイル間の死んだ参照）のループ内反映。ディスクに無い
  本文を検分するには overlay が要る。v1 は保存時に従来どおり報告する
- 既刊モジュールの取り込み（spec 28「次の一手」3 の用途 2。本 spec のループが土台になるが
  入力が PDF/Markdown で別の話）
- ストリーミング表示 / 会話の継続（毎回 1 依頼で完結。前回のやり取りは積まない — 積むと
  ファイルをまたぐ「一気に」へ戻る）
- **第 6 の道具「行番号 + 挿入」は v2**（Phase E で `sd` だけでは弱いモデルが書けないと
  分かったら。決定 3 の「5 本」は v1 の数。査読 1 軽微 4）

## Phase 分割

- ✅ **Phase A（llm_client: tool の往復、2026-09-06 実装済）**: canonical に `tool_calls` / `tool_call_id` / `tool_name` +
  4 adapter の encode/decode（Fuseforks 写経）+ `EDITOR_LLM_*`（`summary_from_env` の一般化）。
  PoC: 既存経路の encode が byte 一致 / 各 adapter で assistant(tool_calls) → tool(result) の対が
  wire に出る / Gemini の id 合成と `functionResponse` / Responses の `function_call_output` /
  `EDITOR_LLM_*` の継承。
- ✅ **Phase B（仕様断片、2026-09-06 実装済）**: 66 KB を `docs/package_spec/` へ割る（列挙を抜く以外は内容不変）+
  `play spec-vocab` + 連結テスト + 連結済み `docs/package_spec.md` + 辞書の初期値。
  outcast への写しはユーザー作業。
- ✅ **Phase C（道具 + ループ、2026-09-06 実装済）**: 道具 5 本（app backend、containment は編集ルート再利用）+
  `harness::edit_assist`（純関数）+ command + 進行イベント。PoC（app）: 一覧外パスの拒否と
  文言 / `sd` の対象限定・preview 前提・全置換と件数・regex 方言 / 対象の `read` がバッファを
  返す / `diff` が初期 vs 作業 / 診断が結果に載る / 周回上限 + 修復 1 周 / 反復検知が
  preview→apply を止めない / containment。PoC（harness）: 素材の byte 固定 / fake クライアントで
  「read → spec → sd preview → sd apply → 終了」の往復。
- ✅ **Phase D（UI、2026-09-06 実装済・目視はユーザー実測待ち）**: ヘッダのボタン + 浮遊パネル + 進行ログ + 読み取り専用 + 単一
  トランザクションの差し替え。目視はユーザー実測（提示層は構造的にユーザーが検出器になる）。
- **Phase E（実測）**: 同梱 4 パッケージに対して「主人公の hp を 12 にして」「湖畔に SAN 判定の
  challenge を 1 つ足して」「モカの profile に趣味を足して」の 3 依頼を 3 モデル（Claude /
  Gemini / Meta）で回し、**未知キー 0・死んだ参照 0・周回数・トークン・`spec` を引いた回数**を
  測る。核心的未知 = 弱いモデルが `sd` の正規表現を書けるか（書けなければ v2 の第 6 の道具）。

## PoC 方針

- 検査規則は増やさない。増えるのは「道具の境界」「素材の形」「停止性」「wire の形」の 4 つ。
- fake クライアント（`DeltaProposer` の ScriptedProposer と同型）でループ全体を実 API なしで固定。
- live は `--ignored` 1 本（Phase E の 3 依頼のうち 1 つ）。

## CLI — `play edit`（2026-09-06、ユーザー要望）

GUI と**同じ道具・同じループ・同じ素材**を `play edit <パッケージ> <相対パス> "指示…" [--write]` で回す。
既定は**ディスクに書かない**（差分・報告・診断を表示 = GUI の「未保存で差し替える」に相当）、
`--write` で原子書き込み（改行コードは元のまま）。終了コードは 0 = 通った / 1 = 診断 error が
残った・打ち切られた / 2 = 使い方。モデルは `EDITOR_LLM_*` > `LLM_*`。そのために
`editor` / `editor_lint` / `editor_vocab` / 道具の実体（`edit_tools`）を app から harness へ移した
（依存は std + serde だけで、app は `pub use` の包み。テストも移動）。Phase E の実測はこの
CLI で回す（GUI の手作業より測りやすい）。

**初回の実測（Meta muse-spark、湖畔の scenario に「聞き耳の percentile challenge を足す」）**:
中身は正しかった（`allowed_flags` / `flag_titles` / `listen_hall` = resolution percentile・stat 聞き耳・
requires location_is・on_success flag・結末文 2 つ、診断ゼロ、`--write` 通過）のに **12 周の
上限で「打ち切り」と報告された**（置換 3 箇所 × preview + apply = 6 周 + read + spec + 失敗 1 回）。
83.8 秒、入力 312,678 tok のうち 284,251 がキャッシュ読み（91%）。ここから決定 8 を改訂:
- 上限 12 → **20**（1 周の入力は履歴の再送で ~27K tok だが 9 割がキャッシュなので代償は小さい）。
- 上限の最後の 1 周は**道具なしのまとめ**（途中までの変更を捨てず、やり遂げた分を報告させる。
  定型文の「打ち切り」が報告に差し替わって失敗に見えていた）。
- `sd` の preview の記憶を**集合**に（独立した置換を 1 周でまとめて preview → 次の周でまとめて
  apply。直前 1 件だけだと 2 本目の preview が 1 本目を消し、置換ごとに 2 周ずつ要った）。
  apply で本文が変わったら他の preview も消す（古い差分で書かない）。

改訂後に同じ依頼を再実測: **10 周・63.1 秒・exit 0**、報告は事実どおり（`listen_hall` を追加し
`allowed_flags` と `flag_titles` に宣言、診断なし）。入力 281,465 tok のうち 248,825 がキャッシュ読み
（88%）。muse-spark は preview をまとめず 1 周 1 呼び出しのままだった（まとめるかはモデル次第）。

## 未決

1. 辞書の初期値（`(kind, 語) → 話題`）。実 content 4 本と Phase E の 3 依頼で偽陰性を数え、
   `spec` を引いた回数が多い話題から辞書へ足す（辞書は最適化なので空でも正しく動く）。
2. `EDITOR_LLM_*` の設定 UI。v1 は `.env`（あらすじの初期形と同じ）。GUI 化は要望が出てから。

## 査読の反映（2026-09-06、2 本）

- **作業バッファの定義**（査読 1 の 1 / 査読 2 の 1・2）: 初期値 = エディタの現在本文（未保存
  込み、frontend が command に渡す）/ 対象の `read`・`grep` はバッファ、他ファイルはディスク /
  `diff` は初期 vs 作業（作者の未保存分は初期に含まれるので AI の累積差分だけが出る）。
  用語節を新設し、決定 4・5 と A・B に凍結。
- **preview 強制をコードで**（査読 1 の 2 / 査読 2 の 3）: `last_preview` の状態機械。
- **停止条件**（査読 1 の 3）: 上限 12 + 修復 1（修復は上限の外）。
- **反復検知の鍵**（査読 2 の 3）: (道具, 引数全体, 結果) の一致。preview→apply は引数が違う。
- **`sd` の引数と文**（査読 1 の 4 / 査読 2 の 7）: `case_insensitive` を持つ・`paths` は持ち
  込まない、と書き直し。
- **モデルの可用性**（査読 1 の 5）: `EDITOR_LLM_*` を v1 へ。Off のときの案内文言。
- **常時量の計算**（査読 1 の 6）: 大原則を含めて ≒12 KB 弱（列挙を抜く分は減る）。
- **診断残存時**（査読 1 の 7 / 査読 2 の 6）: 差し替える、で確定。旧・未決 2 を撤去。
- **読める範囲**（査読 1 の 8）: 読める集合 = 編集対象の一覧と同じ、断片は `spec` で引く、と明記。
- **id の鮮度**（査読 1 の 9）: 単一バッファゆえ他ファイルは常に保存済み。overlay 不要。
- **Gate/op の二重管理**（査読 1 の 10 / 査読 2 の 5）: 列挙は D（型導出）だけ、断片は意味と例。
  サイト版の列挙は `play spec-vocab` で生成して連結。
- **regex 方言**（査読 1 の 11）: Rust regex・`$1`/`$name`/`$$`・全置換・`(?m)`/`(?s)`・一致数。
- **undo 1 回の実装**（査読 1 の 12）: `dispatch` 1 回、E に明記。
- **話題断片の偽陰性**（査読 2 の 4）: `spec` 道具を新設（道具は 5 本へ）+ 辞書は最適化。
- 軽微: 「ディスクに触らない」→「書き込まない」/ PoC を harness と app に分離 / 失敗文言に
  一覧を繰り返さない / 第 6 の道具は v2 と明記。

## 波及

- `CLAUDE.md` 現状節 / `data_contract.yaml`（`ai_edit_assistant` 節を新設: 道具 5 本の契約・
  作業バッファ・停止性・`EDITOR_LLM_*` / `UnifiedToolLayer` に tool 往復の翻訳行を追記）
- `specs/12_unified_tool_layer.md`: canonical の拡張（tool 往復）を追記
- `specs/28_package_editor.md`: 「次の一手」3 → 本 spec へ
- `specs/10_synopsis.md`: `summary_from_env` の一般化（`EDITOR_LLM_*` が同じ機構を使う）
- outcast `package_spec.md`: **形式は不変**。ただし**正本の置き場が Kataribe の断片へ移る**
  （Phase B 以降、サイト側は連結済みの写し）
- `docs/`（Qiita 用）とは別に `docs/package_spec/` を新設 / CLI `play spec-vocab` 新設
