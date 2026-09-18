# spec 31: マウントテスト — 提示層の部品を DOM ごと立てて、実機より先に不具合を出す

**Status**: rev2（2026-09-18 起草 → 同日査読 2 本を反映。決定 5 点はユーザー承認済み）→ **✅Phase 0（2026-09-19）** → **✅Phase A（同日）** → **✅Phase B（同日）**。
Phase C（規律）は以後の運用。
査読の反映は末尾「査読の反映」節に凍結。

## 動機

frontend の vitest（2026-08-29 新設、現在 115 本）は**純関数しか測っていない**。DOM を持たず
（`@vue/test-utils` / `happy-dom` 未導入）、Vue の部品をマウントしないので、**部品が内部状態を持ち、
イベントと props の流れでその状態が動く**種類の不具合には最初から届かない。台帳で「マウントテストが
無いので検出はユーザー実機だった」と書いた不具合は、ここまでで 5 件になる:

| 日付 | 症状 | 真因 | 番号 / failures.md の見出し |
|---|---|---|---|
| 2026-08-28 | 名前の入力欄が閉じない | `v-for` の中の文字列 ref が配列になり、フォーカスが当たらず blur が起きない | #92 |
| 2026-09-01 | 1 ターンに 2 個振ると 2 個目の開帳カードが押せない | `v-if` が真のまま props だけ替わり、Vue が同じインスタンスを使い回す | #94 / `app/src (2026-09-01 実プレイ …)` |
| 2026-09-13 | 「保存 + 登録モデルを更新」で単価が消える | 保存後の同期が、表示にしか無い入力を上書きした | #103 / `app/src (2026-09-13 実機 …単価が消える)` |
| 2026-09-14 | effort を変えて同じ操作をすると価格とコンテキスト長が空になる | 「一致しない」を「別の登録に移った」に数えていた | #103 の残り半分 / `app/src (2026-09-14 実機 …)` |
| 2026-09-14 | メディアをダブルクリックしても改名にならない | 1 回目のクリックが即座に全画面の幕を出し、2 回目がその幕に落ちる | `app/src (2026-09-14 ユーザー要望 …)` |

**番号の出どころ**: #92 までは failures.md の見出しに番号がある。2026-09-01 以降の節は見出しが
「`## 層 (日付 契機 — 引用)`」の形で番号を持たず、#94・#103 は CLAUDE.md 側での呼び名。本 spec では
両方を併記する（failures.md の書式を揃え直すのは本 spec の射程外）。

このうち 2 件（#103 とその残り半分）は、判定を**純関数へ切り出してから**テストした（`selectionAfterSave`）。
これは正しい対処だが、**切り出せる形に気づいた後でしか使えない** — 気づく前の不具合は、純関数の網には掛からない。

## 決定（2026-09-18 ユーザー承認。rev2 で査読を反映）

1. **DOM の偽装は happy-dom。** 選ぶ理由は**起動の速さと軽さ**。jsdom の強みは HTML/DOM 仕様・
   イベント・Web API への厳密な準拠だが、Kataribe の部品が触るのは localStorage・イベント・フォーカス・
   `document.activeElement` の範囲で、そこは両者とも実装している。**レイアウトは両者とも計算しない**
   （`getBoundingClientRect` / `offsetWidth` は 0）ので、選定の材料にならない。
   **メディア系 API（`HTMLAudioElement.play()` / `navigator.mediaDevices`）は最初から共通準備でスタブにする** —
   happy-dom の実装はスタブに近く、本物の挙動に頼るテストは書かない（Phase 0 で実際の挙動は記録する）。
2. **環境はファイル名で割る — vitest の `projects`。** `vite.config.ts` の `test.projects` に 2 つの
   project を置く: `unit`（`src/**/*.test.ts` から `*.mount.test.ts` を除く・environment `node`）と
   `mount`（`src/**/*.mount.test.ts`・environment `happy-dom`）。どちらも `extends: true` でルートの設定
   （`test.css = true` 等）を継ぐ。
   - **ファイル先頭の `// @vitest-environment` 注記は使わない。** 注記は書き忘れても黙って node で走り、
     `document is not defined` のような分かりにくい失敗になる。repo に ESLint は無いので規律で塞ぐ手段も無い。
     ファイル名で決まる形なら、**名前を付けた時点で環境が決まる**。
   - 既存 115 本は `unit` に入り、実行環境は 1 本も変わらない（`theme.test.ts` はビルド後の CSS を
     `?inline` で読むので、DOM が入ると挙動が変わりうる）。
   - 別 project ゆえ `window` / `document` がテストスイートをまたいで漏れない。所要時間が伸びた場合に
     `npm test` から外すのも project 単位でできる（未決 2）。
3. **Tauri の IPC は公式の `mockIPC` で偽装し、偽装していない command は例外で落とす**
   （`@tauri-apps/api/mocks`、`shouldMockEvents: true`）。`vi.mock("@tauri-apps/api/core")` で
   invoke ごと差し替える形は採らない — store の `invoke` 呼び出しと transport の `listen` を
   **実物のコードのまま**通したいから。`convertFileSrc` は `mockConvertFileSrc("windows")`。
   - **イベントの内部 command は落ちない（mocks.js の実装で確認）。** `shouldMockEvents: true` のとき、
     `plugin:event|listen` / `emit` / `unlisten` は**利用者のコールバックより前**に mockIPC 自身が処理し、
     コールバックには届かない。例外で落ちるのは**それ以外**の command だけ。
   - **ウィンドウ系は届く。** `getCurrentWindow().setTitle` 等（`stores/game.ts` / `App.vue` / `TitleBar.vue`）
     は `plugin:window|*` を発行し、利用者のコールバックへ来る。これを既定で黙って通す allowlist は**置かない** —
     置けば「偽装し忘れを黙って通さない」が崩れる。使うテストだけが明示的に偽装する。
   - `void` で投げっぱなしにした呼び出し（SettingsDialog の読み込み群など）が例外を出すと、vitest は
     未処理の rejection としてテストを失敗させる。**偽装し忘れがここでも見える**のは狙いどおり。
4. **store は本物の pinia を使う。** テストごとに `createPinia()` して `setActivePinia()` し、
   `mount` の `global.plugins` にも同じインスタンスを渡す（テスト間で状態を共有しない）。
   store を偽装すると、#94 のように「store は正しく 1 個ずつ進むのに部品が固まる」形を、
   store 側の偽物が隠してしまう。
5. **追加は devDependencies だけ**（`@vue/test-utils` と `happy-dom`）。`@tauri-apps/api/mocks` は既存の
   依存に含まれる。配布物の bundle は 1 バイトも変わらない（Phase 0 で `vite build` の前後を実測して証跡にする）。

## マウントテストが届く範囲と、届かない範囲

これを先に凍結する。「マウントテストを入れた」が「提示層の不具合は機械で捕まる」と読まれると、
2026-08-28 の方法論節（実機でしか出ない層がある）が上書きされてしまう。

| 種類 | 届くか | 理由 |
|---|---|---|
| Vue のインスタンス再利用（#94 型） | 届く | マウントして props を差し替えれば、内部状態が残るかを直接見られる |
| ref の形・フォーカス・blur（#92 型） | 届く | `document.activeElement` と blur イベントは happy-dom にある |
| 保存 → 同期の順序でフォームが消える（#103 型） | 届く | mockIPC で保存を成功させ、その後のフォームの値を読める |
| click と dblclick の取り合い（09-14 型） | **半分** | 取り消しの機構（遅延と `detail`）は偽のタイマーで測れる。**2 回目のクリックがどの要素に落ちるか（幕に当たるか）は測れない** — happy-dom は座標から要素を決める hit-testing をしない |
| `height: 100%` の連鎖切れ（スクロールしない） | 届かない | レイアウトを計算しない |
| canvas 汚染（#91）・カスタムプロトコル | 届かない | asset:// も CORS も無い |
| Tauri の起動・capabilities（#89 / #90） | 届かない | Rust 側と WebView の継ぎ目 |
| IME の確定（09-13 補完の二重） | 届かない | composition イベントの実挙動が無い（こちらは純関数側で固定済み） |
| 音の再生・マイク | 届かない | 決定 1 でスタブにする（鳴ったかではなく、呼んだかしか見ない） |

## 共通の準備と後始末（`src/test/mount.ts`）

順序を固定する。**偽装は mount より前**に置く — 部品は `setup` / `onMounted` の中で command を投げるので、
後から偽装しても間に合わない。
**←2026-09-19 Phase A で改訂**: IPC の偽装そのものは**import より前に 1 回だけ**張る（`transport.ts` が
モジュールの最上段で `listen` を呼ぶため）。以下の準備 1 は「偽装の**表**を置く」と読み替え、後始末 2 は
`clearMocks()` でなく「表を空に戻す」。経緯は「Phase A の実測」節。

**準備（各テストの先頭）**
1. `mockIPC(handler, { shouldMockEvents: true })` — 既定の handler は `throw new Error(\`unmocked command: ${cmd}\`)`。
   テストは必要な command だけを表で渡し、表に無いものは既定の handler へ落ちる。
2. `mockConvertFileSrc("windows")`。
3. メディア系のスタブ（`HTMLMediaElement.prototype.play` は解決済みの Promise を返す・`pause` は何もしない・
   `navigator.mediaDevices.enumerateDevices` は空配列）。
4. `const pinia = createPinia(); setActivePinia(pinia);`
5. `mount(部品, { global: { plugins: [pinia] } })`。

**後始末（`afterEach`）**
1. `wrapper.unmount()` — 部品の `onBeforeUnmount`（タイマーやリスナーの解除）を先に走らせる。
2. `clearMocks()` — IPC の偽装を外す（次のテストの準備 1 で張り直す）。
3. `localStorage.clear()`。
4. `vi.useRealTimers()` と `vi.restoreAllMocks()` — 偽のタイマーやスタブを使ったテストの後始末を、
   使わなかったテストでも無条件に行う（呼び忘れが次のテストへ漏れない）。
5. **←2026-09-19 Phase B で追加**: 表に無かった command が 1 件でも呼ばれていたら、ここで**そのテストを落とす**
   （残りの後始末を全部済ませてから投げる）。意図して表に無い command を呼ぶテストだけが `allowUnmocked()` で外す。
   理由は「Phase B の実測」節。

## Phase

### Phase 0 — 土台

- devDependencies に `@vue/test-utils` と `happy-dom`、`vite.config.ts` に `test.projects`（決定 2）。
- `src/test/mount.ts` に上の準備と後始末。
- 煙のテスト 1 本（`HelpNote` を立てて開閉する = store も IPC も使わない最小の部品）。
- **完了条件**（すべて実測で記録する）:
  - 既存 115 本が `unit` の側で同じ件数・同じ結果で通る（所要時間を前後で記録）。
  - `vite build` の main chunk が前後で同じバイト数（決定 5 の証跡）。
  - happy-dom の `HTMLAudioElement.play()` の素の挙動（Promise を返すか・例外を投げるか）を記録する。
    スタブにする判断は変わらないが、なぜスタブが要るかを台帳に残す。
  - 偽装していない command を呼んだら例外で落ちる / `listen` は落ちない、を 1 本ずつ確かめる
    （決定 3 の二つの主張をテストで固定する）。

#### ✅Phase 0 の実測（2026-09-19）

| 完了条件 | 変更前 | 変更後 |
|---|---|---|
| 既存テスト | 115 本・1.97 秒 | `unit` 115 本・1.92 秒（verbose の `|unit|` ラベルで 115 本を確認）+ `mount` 10 本・1.41 秒 = 計 125 本・2.77 秒 |
| main chunk（`index-*.js`） | 419,390 バイト | 419,390 バイト・**sha256 一致**（`95098a73…a85c`） |
| 型検査（`vue-tsc`） | 通る | 通る |

- **happy-dom の素の `HTMLAudioElement.play()`**: 例外を投げず、**解決済みの Promise を返す**（`paused` は false へ）。
  査読 2 の「スタブに近い実装」は当たっていた。一方で **`navigator.mediaDevices` は存在しない**。
  ゆえにスタブにする理由は「play が落ちるから」ではなく、①呼んだかをスパイで確かめるため
  ②マイク一覧を読む部品（`refreshMicDevices`）が `mediaDevices` の不在で落ちないため、の 2 つ。
  テスト `環境 > happy-dom の素の Audio.play()…` がこの観察を固定する（準備の前に測る）。
- **決定 3 の二つの主張はテストで固定した**（`src/test/harness.mount.test.ts`）: 表に無い command は
  `unmocked command: …` で reject される / `listen`・`emit` は表が空でも通り、`plugin:event|*` は
  利用者のコールバックに一度も届かない / `getCurrentWindow().setTitle` は `plugin:window|set_title` として
  コールバックへ届き、表に無ければ落ち、在れば通る。
- 共通準備は `src/test/mount.ts`（`prepare` / `mountWith` / `teardown`）、後始末は
  `src/test/mount.setup.ts` を mount project の `setupFiles` に置いて**無条件に** afterEach で掛けた。
- 依存は `@vue/test-utils` 2.5.1 と `happy-dom` 20.14.5（devDependencies）。`npm audit` の警告 5 件は
  すべて既存の依存由来（brace-expansion / browserslist / nanoid / postcss 系）で、今回の 2 つは含まれない。
- **煙のテストで分かったこと**: 偽装の返り値はそのまま呼び出し側へ返る（`setTitle` が `null` を返した）。
  最初の期待値（`undefined`）は私の書き間違いで、土台の主張ではない。

### Phase A — 既に直した不具合を固定する（後付けの Red→Green）

直した後に書くテストは、そのままでは Red を一度も見ない。**修正を一時的に戻して Red を確かめてから**
戻す（例: `ConversationLog.vue` の `:key` を消す → 落ちる → 戻す → 通る）。確かめた手順はコミット
メッセージに残す。

1. **#94**: `ConversationLog` に判定 2 件の行を積み、1 件目を開帳したあと 2 件目のカードがクリックを
   受けて出目を開くこと。スクランブルの演出（1.1 秒）は偽のタイマーで進める。
2. **#92**: `StatePanel` の編集モードで新規作成の入力欄を開き、**フォーカスが入力欄にある**こと、
   blur で確定して閉じること。
3. **09-14 dblclick**: `vi.useFakeTimers()` のもとで、メディア一覧の名前をクリックしたとき**300ms 経つまで**
   プレビューが開かないこと、その間に dblclick が来れば改名の入力欄になりプレビューは開かないこと
   （`advanceTimersByTime(300)` 後にも開いていないことまで見る）。届かない半分（幕への着地）は
   テストの冒頭コメントに書く。

#### ✅Phase A の実測（2026-09-19）

3 件とも、修正を一時的に戻して Red を確かめてから戻した。

| 対象 | テスト | Red の作り方 | Red の出方 |
|---|---|---|---|
| #94 | `ConversationLog.mount.test.ts` 2 本（別の判定 2 件 / 同じ出目 2 回） | checks 側 `DiceReveal` の `:key` を消す | 2 本とも `revealed` が **1 のまま**（実機のデッドロックと同じ形） |
| #92 | `StatePanel.mount.test.ts` 2 本（新規作成 / 改名で `document.activeElement` が入力欄） | `:ref="bindDraft"` を修正前の `ref="draftInput"` + `draftInput.value` へ | 2 本とも落ち、未処理 rejection に **`el.focus is not a function`**（配列に focus が無い = 実機の機序そのもの） |
| 09-14 dblclick | `StatePanel.mount.test.ts` 2 本（300ms 経つまで開かない / 間の dblclick で改名になり開かない） | 名前の `@click` を修正前の即時プレビューへ | 2 本とも落ちる。**ただし落ちた理由は「プレビューが即座に開いた」で、「幕が 2 回目を奪った」ではない** — happy-dom は hit-testing をしないので Red の状態でも dblclick は名前へ届く（表の「半分」のとおり） |

演出の時間は偽のタイマーで進めた（開帳は `setTimeout` + `performance.now()`、クリックの遅延は `setTimeout`）。

**土台の設計を 1 点変えた（Phase A 初回で判明）**: 共通準備を「テストの中で `mockIPC` を張り、後始末で
`clearMocks()`」の形で書いていたところ、ConversationLog を import しただけで未処理 rejection が 6 件出た
（`transformCallback` of undefined）。**`transport.ts` はモジュールの最上段で transport を作り `listen` を
呼ぶ**（アプリの生涯を通して生きる購読）ので、**「偽装は mount より前」では足りず「import より前」が要る**。
加えて `clearMocks()` は Tauri の内部ごと消すので、import 時に登録された購読を 2 本目以降のテストで壊す。
→ 偽装は `mount.setup.ts`（テストファイルの import より前に走る）で **1 回だけ**張り、テストごとには
**偽装の表だけ**を差し替える形にした。後始末は表を空に戻す（空の表ではどの command も従来どおり例外で落ちる
= 決定 3 の性質は不変、`harness.mount.test.ts` の後始末テストで固定）。
**一般化: 部品の外側にもモジュールの最上段で副作用を持つ層がある。偽装の張り時は「部品が動く前」でなく
「そのモジュールが評価される前」で決める。**

### Phase B — 保存経路（#103 の両半分）

`SettingsDialog` の AI モデルタブを立て、単価 3 欄とコンテキスト長を入れて「保存 + 登録モデルを更新」を
押したあと、**フォームと登録簿の両方に値が残る**こと。effort だけを変えた場合も同じ（残り半分）。

- `SettingsDialog` はマウント時に読み込みを約 11 本投げる（`loadLlm` / `loadImageKeys` /
  `loadSummaryTimeout` / `loadRecentTurns` / `loadUsage` / `loadEditorProfile` / `refreshSheets` /
  `refreshDevMode` / `refreshMicDevices` 他）。~~決定 3 により、どれか 1 本でも偽装し忘れると
  保存ボタンに辿り着く前に落ちる。~~ **←2026-09-19 訂正: 落ちない**（下の実測）。**テストの冒頭に
  「このテストが偽装する command」の表を置き**、部品が新しい読み込みを足したらテストが落ちる形にする
  （脆さを隠さず、仕様として見えるようにする）。
- 1866 行あり立てる費用が一番大きい部品なので、Phase A で準備の形が固まってから着手する。

#### Phase B の実測（2026-09-19）

`SettingsDialog.mount.test.ts` 2 本 + 検出器の性質テスト 1 本（mount 16 → 19、frontend 131 → 134）。

**起草時の予告は外れていた** — 「偽装し忘れると保存ボタンに辿り着く前に落ちる」と書いたが、
`SettingsDialog` のローダーはほぼ全部が失敗を `try/catch` で握り潰す（Tauri の外でも画面を出すため）。
表に無い command は reject されるが、catch に吸われて**欄が空になるだけ**で、テストは黙って通る。
`void` の投げっぱなしなら未処理の rejection として落ちる、という決定 3 の前提は**catch の無い呼び出しにしか
効かない**。そこで `mount.ts` に検出器を足した: 表に無い呼び出しを記録し、後始末で 1 件でもあれば落とす。
実際に表から `get_recent_turns` を 1 行外すと、2 本とも
「偽装の表に無い command が呼ばれた…: get_recent_turns」で落ちることを確かめた。Phase A の 6 本は
検出器を入れても**そのまま通った**（偽装の漏れは無かった）。落ちたのは偽装そのものの性質を見る
harness のテスト 3 本だけで、これらは `allowUnmocked()` を明示した。

偽装した command は 11 本（onMounted のローダー 9 + `set_llm_config` + `plugin:window|set_title`〔保存後の
`refreshLlmModel`〕）。`set_llm_config` は写しの `.env` を書き換え、`get_llm_config` はその写しを返す
（backend の往復を 1 つの状態で表す）。

**Red の出方**（修正を一時的に戻して確認し、`git checkout` で復元）:

| 戻した形 | 1 本目（単価だけ打つ） | 2 本目（思考の深さも変える） |
|---|---|---|
| 61513cd^（#103 前: 同期が常に上書き + 単価を .env の後に読む） | ✗ フォーム `'' ≠ '3'` | ✗ 登録簿の単価が `undefined` |
| 61513cd（一致なしを『変わった』とみなす同期 + 同期ありの保存） | ✓ | ✗ フォーム `'' ≠ '3'`（登録簿は通る） |

61513cd の形で落ちるのが 2 本目の**フォームだけ**であることは、09-14 の報告「登録簿には残るのに画面が空」の
機序そのもの。**純関数の vitest（selectionAfterSave）では「フォームに書かれる値」までしか見えなかった**が、
ここでは画面の入力欄の値と localStorage の両方を同じ操作の後に読んでいる。

### Phase C — 以後の規律

`app/src/components` の不具合を直すときは、上の表で「届く」に入る種類ならマウントテストを 1 本付ける。
「届かない」種類は、従来どおり接地の限界として申告する。**全部品を網羅することは目的にしない**
（壊れた所と、壊れ方の一族に当たる所だけ）。

## 未決

1. `FirstRunTour` のようにレイアウトの実寸（`getBoundingClientRect`）を読む部品は、happy-dom では
   全部 0 になる。この種の部品は Phase C の対象から外すか、純関数（`placeCard` 等、既にテスト済み）で
   足りるとするか。
2. 所要時間の上限。Phase 0 で `unit` / `mount` それぞれの時間を測り、`mount` が大きく伸びるなら
   `npm test` を `unit` だけにして `mount` を別スクリプトにするかを判断する（決定 2 の projects なら
   設定 1 行で切り替えられる）。

## 査読の反映（2026-09-18 rev2、査読 2 本）

| 指摘 | 判断 | 反映先 |
|---|---|---|
| 決定 1「jsdom の強みはレイアウト」は自己否定（両者ともレイアウトを計算しない）。jsdom の強みは仕様準拠 | 採用 | 決定 1 の選定理由を「速さと軽さ」へ |
| 決定 1 で Audio は足りると断定しつつ未決 1 で未確認と書いている | 採用。**スタブ前提へ倒す**（査読 2 案）+ 素の挙動は Phase 0 で記録（査読 1 案） | 決定 1・Phase 0・旧未決 1 を削除 |
| `@vitest-environment` の書き忘れ。ESLint かレビュー規律で防ぐ | **projects で構造的に塞ぐ**へ変更（repo に ESLint が無い。査読 2 の「別 config で分離」とも一致） | 決定 2 |
| 環境の混在でグローバルが漏れる | projects で解消 | 決定 2 |
| 「知らない command は例外」と `listen` を本物で通すことが衝突する（`plugin:event|*` が弾かれる）【査読 2 最重要】 | **部分的に棄却**。mocks.js の実装では `shouldMockEvents: true` のとき `plugin:event|*` は利用者のコールバックより前に処理され、例外にならない。ただし `plugin:window|*` は届くので、その扱いを明記した。**既定の allowlist は置かない**（偽装し忘れを黙って通すことになる）。二つの主張は Phase 0 でテストに固定する | 決定 3・Phase 0 |
| `mount.ts` の実装形（既定 handler が throw） | 採用 | 共通の準備 1 |
| Phase B は初期化の command が多く、例外方針と摩擦する。冒頭に列挙を | 採用（実数を数えた = 約 11 本、うち多くが `void` 投げっぱなし） | Phase B |
| `setActivePinia` を共通準備に | 採用 | 決定 4・共通の準備 4 |
| 後始末の順序（`clearMocks → mockClear → localStorage.clear → mockIPC 再設定`） | **趣旨は採用・順序は改めた**。要点は「偽装は mount より前」で、再設定は次のテストの準備で行う。`mockClear` は vitest のスパイ単位の API なので `vi.restoreAllMocks()` に置き換えた | 共通の準備と後始末 |
| Phase 0 に `vite build` のサイズ確認を | 採用 | Phase 0 完了条件 |
| #94 の表記不整合（表は `app/src (2026-09-01)`、本文は #94） | 採用。failures.md の見出しに番号が無い（09-01 以降）ことを実読で確認し、両方を併記 | 動機の表と注記 |
| Phase A 3 に偽のタイマーが必須・後始末に `useRealTimers` | 採用 | Phase A 3・後始末 4 |
