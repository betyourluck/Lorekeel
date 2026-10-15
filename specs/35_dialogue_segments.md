# spec 35 — 台詞と地の文の分離 (話者の照合・吹き出し・キャラ別の声)

**Status**: rev2 → **Phase 0〜D 実装済** (2026-10-14 起草 → 同日査読 2 本〔矛盾 3・不整合 2・懸念 2・誤記 1 / CRITICAL 4・MAJOR 3〕を反映 → 同日 Phase 0〜B → 2026-10-15 Phase C・D)。残 = Phase E (実測)。
契約は data_contract `DialogueSegments` 節。未決 7 点は査読 #2 の承認 (条件は rev2 で充足) で確定。

---

## 動機

1. **見せ方の主流がキャラチャットになっている** (ユーザー観察 2026-10-14、tikita 等)。地の文と、
   顔アイコンつきの吹き出しの台詞を分けて並べる形。Lorekeel は語りを 1 本の文字列で返すので、
   この形にできない。
2. **矛盾しない GM に直接効く**。`narration` は検証の外にあり (#23)、「その場にいないキャラが喋る」
   (#47・#49) はプロンプトの規律と一貫性検査 (spec 32 の `absent_speaker`、Jev の観測) でしか
   守れていない。台詞が登場人物の id つきで出てくれば、エンジンの事実 (`present_at`) と照合して
   **却下できる**。主人公の台詞を GM が書くことも欄で禁じられる。キャラチャット化は流行への
   追従である以前に、**語りの一部を裁定の内側へ入れる変更**になる。
3. **キャラ別の声が解ける**。読み上げ (TTS 節) は「narration が 1 本の文字列」を理由に多声を
   保留しており、案 B (話者つき台詞 = LLM 契約の変更) がこの spec そのもの。

### 位置づけ (2026-07-20 の判断との関係)

7/20 に「RP のフロントエンド競争には乗らない」と決めた。この spec はそれを覆さない —
鍵が要らず無料枠がありスマホで動くサービスと、BYO キーのデスクトップアプリが使い勝手で競うのは
不利なので、**勝負どころは「忘れない・矛盾しない」のまま**、キャラチャットは**見せ方として**取り込む
(ユーザー確認 2026-10-14)。

### スコープ外

- 表情の差分 (次の spec。キャラごとの閉じた表情集合を台詞が選ぶ形を想定)
- Live2D 風の立ち絵 (後回し。本物の Live2D は使わず、外部の「立ち絵から動く絵」ライブラリに乗る = ユーザー決定)
- 一人プレイでの「空のまま送信 = キャラが続ける」(多人数の PASS と同型だが別件)
- 作者がキャラに声の目安を書く欄 (声の割り当てはプレイヤー側。エンジンごとに話者 ID が違うので作者は ID を知り得ない)
- 再開時の「前回までの語り」の吹き出し化 (セーブに段を持たせる別 spec)

---

## 接地 (2026-10-14 コード実読)

| 事実 | 場所 |
|---|---|
| `StateDelta.narration: String` (doc comment 無し = schema の description も空) | `crates/gm_core/src/state.rs:430` |
| 契約は `narration: string # 情景/NPC台詞。検証しない` | `data_contract.yaml:26` |
| 主人公の entity は定数 `PLAYER = "player"` で、`Scenario.characters` のキーではない。主人公の表示名 `Protagonist.name` は空でありうる | `crates/gm_core/src/state.rs:28` / `crates/gm_core/src/spine.rs:1086` |
| `validate` は表示名の重複を検査しない (予約 id は `party` と `*` だけ) | `crates/gm_core/src/spine.rs:1588-1599` |
| 編集距離は `lint.rs` に `levenshtein` が 1 本ある (非公開) | `crates/gm_core/src/lint.rs:691` |
| 語りの消費者: harness 10 ファイル (経緯・あらすじ・直前の語り・エピローグ・一貫性検査・挿絵プロンプト・編集・Memoria 等) / app backend `lib.rs` 73 箇所 / frontend 9 ファイル (会話ログ・読み上げ・ログ書き出し等) | grep |
| `run_turn` は `adjudicate` → `apply(state)` を直に行う | `crates/harness/src/turn.rs:483-503` |
| `apply` は内部で `adjudicate` → ops 適用 (RNG 消費) → `turn += 1` → `fire_triggers` (トリガー内のダイスでも RNG 消費・登場もここ) | `crates/gm_core/src/engine.rs:754-783` |
| 移動したターンの顔ぶれは**出発地 ∪ 到着地 (適用後)** でないと、移動先で迎えた人の台詞が誤検出になる (実例 0.94 → 0.16) | spec 32 / `crates/harness/src/consistency.rs::widen_present_for_move` |
| 読み上げは自前の直列鎖で 1 文ずつ流す。ライブラリに `updateOptions({ speaker })` があり、鎖の中で文ごとに話者を変えられる | `app/src/tts.ts:316` / `@aituber-onair/voice` の `VoiceEngineAdapter.d.ts:55` |
| `kataribe.tts.voices` は**既に声一覧のキャッシュ** (`LS_VOICES` = `{engine, url, voices}`) | `app/src/tts.ts:76` |
| ~~ワイヤの golden 10 本のうち 6 本が `StateDelta` の schema を埋め込んでいる~~ **誤り (実装時に訂正)**: golden は手書きの小さな schema (`narration: string`) を使っており、実際の `StateDelta` の schema は埋め込んでいない (`narration` で grep した早合点) | `crates/llm_client/src/golden.rs:28` |
| `sanitize_narration` (漏れたマークアップの掃除) は narration 1 本に掛かっている | `crates/llm_client/src/lib.rs:180` |

---

## 決定

### D1. 形 — `narration` を「順に並んだ段」の配列にする

```json
"narration": [
  { "text": "教室の空気が一瞬凍りついた。" },
  { "speaker": "rinka", "text": "挨拶のついでに愛想を振りまくなんて、随分と余裕があるのね。" },
  { "text": "雪菜は何も言わず、静かに視線を向けている。" }
]
```

- `speaker` が無い (空) 段 = 地の文。ある段 = 台詞。`kind` は置かない (欄が 1 つ少ないほど弱いモデルが崩しにくい)。
- **キーは `narration` のまま** (査読 #2 承認)。GM_SYSTEM・自己修復の文言・各モデルの慣れが `narration` を前提にしており、改名すると波及が広い。
- **順序が意味を持つ** (地の文 → 台詞 → 地の文)。台詞だけを別配列にする案は並びを失うので採らない。

### D2. 処理の順序と、文字列の救済の置き場

```
llm_client  parse      … narration を「段の配列」か「生の文字列」のどちらかで受ける (scenario を知らない)
llm_client  sanitize   … 漏れたマークアップの掃除を段ごと (生の文字列ならその 1 本) に掛ける
gm_core     resolve    … 生の文字列を段へ割り、全段の話者を解決する (scenario が要る = ここで初めてできる)
gm_core     adjudicate … ops の裁定 (従来どおり純粋)
gm_core     apply_checked … 写しへ適用 → 話者の照合 → 通れば確定 (D3)
```

- **受け側の型**: `StateDelta.narration: Narration` = `Segments(Vec<Segment>)` | `Raw(String)`。
  手書きの Visitor で受け (`visit_seq` / `visit_str`)、**untagged enum は使わない** — #93 と同じ理由で、
  全形に失敗したとき内側の理由 (段の欄の誤り等) が総称文言に潰れる。
- **schema は配列だけ**を示す (ツール呼び出しの文法でも配列へ寄せる)。文字列は受け側の救済であって案内しない。
- **文字列の救済** (`resolve` の中): 行頭の `名前「…」` で、`名前` が D3 の手順 0〜3 で**登場人物か主人公に
  解決できたときだけ**台詞の段にし、残りは地の文の段にする。
  - 査読 #1-② の非対称 (配列なら `店員` が吹き出し、文字列なら地の文) は**意図して残す**。配列では
    話者が欄で宣言されているが、文字列では行頭の語が話者か語りの一部か判別できない
    (`そして「…」`・`彼女は小さく「…」` が架空の話者になる)。文字列は劣化した経路なので、
    吹き出しを捏造しない側に倒す。

### D3. 話者の解決と照合

**解決 (`resolve`、scenario を見る純関数)**。話者ラベルは前後の空白を落として比べる。空なら地の文。

| 手順 | 条件 | 結果 |
|---|---|---|
| 0 | `player`・主人公を指す語 (私 / 俺 / 僕 / わたし / あたし / 主人公 / あなた / プレイヤー / I / me / you / protagonist) | 主人公 (`PLAYER`) → 照合で却下 |
| 1 | 登場人物の id に完全一致 | その entity |
| 2 | 表示名に完全一致。照合の相手は**登場人物の `CharacterDef.name` と主人公の `Protagonist.name`** (どちらも空でないものだけ) | その entity (主人公なら `PLAYER` → 照合で却下)。**2 人以上に一致すれば却下** `AmbiguousSpeaker { label, candidates }` (id で書き直せ) |
| 3 | 登場人物の id・表示名・主人公の表示名に**近い** (編集距離: 3 字以下なら 1、それより長ければ 2) か、**含む** (2 字以上の名前を部分文字列に持つ: `凛花さん`) | 却下 `UnknownSpeaker { label, candidates }` (候補の id で書き直せ) |
| 4 | どれでもない | **名もない人物** (店員・衛兵など)。ラベルを表示し、照合しない |

- 主人公の表示名を手順 0 でなく**手順 2 の照合に含める**のは、NPC が主人公と同名のとき曖昧さとして検出するため
  (手順 0 で先に引き当てると、同名の NPC の台詞が黙って主人公扱いになる)。査読 #2 CRITICAL-1 の
  「主人公の表示名 → `PLAYER`」は手順 2 で満たし、`player` と主人公を指す語だけを手順 0 に置く。

- 手順 3 は査読 2 本の指摘 (誤字 `aliec`・敬称つき `凛花さん` が名もない人物へ逃げて不在照合を素通りする) への対策。
  **誤検出の代償は再生成 1 回**で、取り違え (別人の台詞として確定) より安い側に倒す。既知の誤検出:
  `凛花の母` のような「既知の名前を含む別人」も却下される → GM は `母親` 等の別の呼び名で書き直す。
- 主人公の表示名・登場人物の表示名が空なら、その名前では一致させない (空文字・部分一致の誤爆を防ぐ)。

**照合 (`apply_checked`、gm_core)** — 適用後の顔ぶれはトリガーの発火で変わるので、`adjudicate`
(射影はトリガーを含まない) の中では判定できない。

```rust
pub fn apply_checked(
    state: &mut GameState, scenario: &Scenario, delta: &ResolvedDelta, controlled: &[EntityId],
) -> Result<ApplyOutcome, Vec<RejectReason>>
```

1. `state` の写しへ `apply` する (ops の裁定もここで通る)
2. 解決済みの全段について:
   - 主人公、または `controlled` (多人数で人が操作している仲間) → `SpeakerControlled { speaker }`
   - **ターン開始時の顔ぶれ ∪ 適用後の顔ぶれ**に居ない → `SpeakerNotPresent { speaker, present }` (今いる人物を列挙)
3. 却下が無ければ `*state = 写し` とし、**写しの `ApplyOutcome` をそのまま返す** (再適用しない。出目・判定・発火ビートが試し適用と同一 = 査読 #2 MAJOR-2)。
   却下なら写しを捨てる — 正本も RNG も動かない。

- **`apply` 本体は変えない** (査読 #1-④ の「apply に組み込む」は採らない)。`apply` は決断の確定・対決のラウンド・
  テストからも呼ばれ、どれも LLM の語りを持たない。照合が要るのは LLM のデルタだけなので、包む関数を
  gm_core に足してトランザクションをエンジンの内側に置く (harness が clone を手で管理しない)。
  `controlled` は gm_core が知らない多人数の情報なので引数で受ける。
- 却下理由は**誰ならいるか / 何で書き直せば通るか**を必ず載せる (#42)。
- **試行上限に達したら**従来どおりターン失敗 (`TurnOutcome::Rejected`) として提示する (降格して通さない)。

**塞げない穴は明記する**: GM が地の文の中に「」で台詞を書けば照合を素通りする。GM_SYSTEM で
禁じ (D5)、一貫性検査の `absent_speaker` は地の文の側の網として残す (質問文は変えない = #104)。

### D4. 型の境界と、語りを受け取る各所

| 層 | 型 |
|---|---|
| `StateDelta.narration` | `Narration` (段の配列 or 生の文字列) — **型が変わるのはここだけ** |
| 解決後 | `ResolvedSegment { speaker: Option<Speaker>, text }` / `Speaker = Entity(id) | Extra(label)` |
| `TurnOutcome::Accepted` | `narration: String` (= `plain`) は**型も意味も据え置き** + `gm_narration: String` (= `plain_for_gm`、harness で 1 回だけ作る) + `segments: Vec<ResolvedSegment>` を追加 |
| `TurnView` (app → frontend) | `narration: String` (据え置き) + `segments: Vec<SegmentView { speaker, name, icon, text }>` を追加 |

平らにする関数は 2 本 (査読 #2 CRITICAL-3 / #1-⑤)。`plain` 本体は id を含めない。

- `plain(scenario)` — 地の文はそのまま、台詞は `名前「text」` (名もない人物はラベル、表示名が空なら id)
- `plain_for_gm(scenario)` — 台詞は `名前 (id):「text」` (名もない人物は `ラベル:「text」`)。GM が自分の過去の語りを
  **段の形で思い出す**ための書式 (旧来の「地の文に「」」を真似させない)

| 消費者 | 使う形 |
|---|---|
| 直前の語りの還流 `recent_narrations` (セーブにも入る) | `plain_for_gm` |
| 発火ビート・判定の結末を継続文脈へ繋ぐ `carryover_narration` | `plain_for_gm` (上と同じ鎖) |
| `last_narration` (スロット一覧・再開の「前回までの語り」・エピローグの素材) | `plain` |
| 経緯の fallback (summary が空のとき語りの冒頭) | `plain` |
| 一貫性検査 (Jev) | 当ターンの語りは `plain`。**過去の語り (スナップショットの `recent_narrations`) は `plain_for_gm` のまま渡る** (平らな文字列を別に持たないため。質問文は変えない = #104、材料の書式が変わった分の影響は Phase E で見る) |
| 挿絵のプロンプト書き | `plain` |
| ログ書き出し | `plain` |
| 会話ログの表示・読み上げ・多人数のゲスト | `segments` |

- **セーブ形式は不変** (欄の型は文字列のまま)。中身の書式が `recent_narrations` = `plain_for_gm` /
  `last_narration` = `plain` に分かれるので、契約の「`last_narration` は末尾の写し」を「末尾の表示用」へ書き換える。
  旧セーブは `last_narration` から 1 本 seed する従来の経路のまま (平らな書式が混ざるだけで壊れない)。
- `sanitize_narration` は段ごとに掛ける (D2)。

### D5. GM_SYSTEM の規律 (静的 system = キャッシュの安定プレフィックスが 1 回だけ動く)

- 台詞は必ず `speaker` つきの段に分ける。**地の文に「」で台詞を書かない**
- `speaker` は「この場にいる」に挙がった人物の **id** (名もない人物は `店員` のような短い呼び名。登場人物の名前を含めない)
- 主人公 (多人数では人が操作する仲間も) の台詞は書かない
- **心の声・独白は speaker の段にしない** (地の文で書く) — 査読 #2 の条件
- 字数や地の文の量の規律は変えない (朗読として効くという実測があるので地の文を削らせない)

### D6. 表示 — 吹き出し

- 台詞は顔アイコン + 名前 + 吹き出し、地の文は従来の段落。プレイヤーの入力は従来どおり右寄せ。
- 名前とアイコンは **backend が scenario から解決して DTO に載せる** (`SegmentView`)。その場にいる人の一覧から
  引かない (適用後に退場した人の台詞もありうる)。名もない人物はアイコン無し。
- 多人数プレイのゲストにも同じ DTO が届く。

### D7. キャラ別の声

- **割り当てはプレイヤー側の設定 `kataribe.tts.characterVoices`** (査読 #2 CRITICAL-4。`kataribe.tts.voices` は
  声一覧のキャッシュのまま据え置く。キー名はリポジトリの camelCase に揃える)。形は
  `{ [エンジン + サーバー URL]: { [パッケージ]: { [entity]: 話者 ID } } }` (話者 ID はエンジンごとに違う)。
  設定ミラー (settings.json) の射程に入る。
- 未割り当ての人物と名もない人物は**語り手の声** (いまの設定の話者)。「その他の人物の声」枠は要望が出てから。
- **読み上げる範囲**: `すべて` (地の文 + 台詞、既定 = 今と同じ) / `台詞だけ` (ユーザー要望)。
  **割り切り**: `台詞だけ` では、GM が地の文に紛れ込ませた台詞 (D3 の塞げない穴) は読まれない (査読 #1-⑥)。
  Phase E で地の文の「」の率を測り、高ければ「台詞だけでも地の文の「」は語り手の声で読む」を足す。
- 実装は直列鎖の中で段ごとに `updateOptions({ speaker })` → `speakText`。鎖が直列なので切り替えが前の文に混ざらない。
- 設定の場所は ~~サウンドタブの「キャラクターの声」~~ **会話ペイン右下の読み上げの操作列にあるアイコンから開く浮遊パネル** (2026-10-15 ユーザーFB で移設。いま遊んでいるパッケージの登場人物を一覧。
  そのために GameView に `cast: [{ id, name, icon }]` を足す)。

### D8. 波及

- ~~ワイヤの golden 6 本は schema の変更で変わる~~ **golden は変わらない** (手書きの小さな schema を使っている。接地表の訂正を参照)。キャッシュの安定プレフィックスは schema と GM_SYSTEM の変更で 1 回動く。
- 未知キー lint・作者向け仕様・編集の語彙は `StateDelta` を扱わないので無関係 (作者の YAML は変わらない)。
- 撤去は無い。型が変わるのは `StateDelta.narration` だけで (D4)、その読み手はコンパイラが名指しする
  (`TurnOutcome` / `TurnView` の `narration: String` は据え置きなので、その先の消費者は書式の割当表 D4 で追う)。
- `levenshtein` は `lint.rs` から `pub(crate)` で共有する (手順 3)。

---

## Phase

- **Phase 0 — 契約凍結**: data_contract の `StateDelta.narration` / `Segment` / `ResolvedSegment` / 解決の手順表 /
  却下理由 4 種 / `apply_checked` / `plain`・`plain_for_gm` と消費者の割当表 / `SessionSave` の書式の書き換え /
  `kataribe.tts.characterVoices` の形。
- **Phase A — gm_core**: `Narration` の手書き Visitor、`resolve` (救済の分割を含む)、`apply_checked`、
  `RejectReason` 4 種 (`SpeakerControlled` / `SpeakerNotPresent` / `AmbiguousSpeaker` / `UnknownSpeaker`、ja/en)、
  `plain` / `plain_for_gm`、schema の description (doc comment)。
- **Phase B — harness / llm_client**: `run_turn` を `resolve` → `adjudicate` → `apply_checked` へ (resolve と adjudicate の
  却下理由は合わせて 1 回で返す)、GM_SYSTEM (D5)、消費者を割当表どおりに、`sanitize_narration` を段ごとに、
  golden の作り直し、多人数の操作中の仲間を `controlled` へ。
- **Phase C — app / frontend**: `SegmentView` と `cast`、会話ログの吹き出し、ログ書き出し、多人数の配信。
- **Phase D — 読み上げ**: 声の割り当て UI と保存、読み上げる範囲、段ごとの話者切り替え。
- **Phase E — 実測**: 複数モデル (muse-spark / gpt-6-luna / Opus / Gemini / Grok + 弱いモデル 1 つ) で
  ①配列で返す率 (文字列の救済に落ちる率) ②話者の却下率 (理由の内訳) ③地の文に「」が残る率 ④ターンあたりの
  再生成の増加。ユーザーの実プレイで吹き出しと声の体感。②が高ければ D3 の手順 3 の閾値と未決 2 を見直す。

## PoC (Red → Green)

gm_core
- `narration` を配列でも文字列でも受ける / 段の欄の誤りは総称文言に潰れず理由が残る (#93)
- 文字列の救済: 解決できた名前だけ台詞にし、`そして「…」` は地の文のまま
- 手順 0 / 2: `speaker: "player"`・主人公の表示名・`私` がすべて `SpeakerControlled` (査読 #2 の指定)
- 手順 0 / 2: 主人公の表示名が空のとき、空ラベルや部分一致で主人公に誤爆しない
- 手順 2: 表示名が 2 人に一致すれば `AmbiguousSpeaker` (NPC と主人公が同名のときも)
- 手順 3: `aliec` (alice の誤字) と `凛花さん` が `UnknownSpeaker` で候補つき / `店員` は名もない人物として通る
- 照合: 不在の話者は `SpeakerNotPresent` で今いる人物つき / 操作中の仲間は `SpeakerControlled`
- 照合: **到着時のトリガーで現れた人物の台詞は通る** (開始時 ∪ 適用後)
- `apply_checked`: 却下で正本と RNG が無傷 / **受理時の出目・発火ビートが試し適用と一致** (査読 #2 MAJOR-2)
- `plain` は id を含まず、`plain_for_gm` は `名前 (id):「…」`

harness
- fake proposer で「不在の話者 → 却下 → 理由に今いる人物 → 再提出で受理」
- resolve と adjudicate の却下理由が 1 回の自己修復に合わせて載る
- `recent_narrations` に `plain_for_gm`、`last_narration` に `plain` が入る
- GM_SYSTEM に D5 の規律 (独白を含む) が載る

frontend (mount)
- 台詞が吹き出しで出る・名もない人物はアイコン無し
- 声の割り当てが段ごとに効く (`updateOptions` の呼び順をスパイで見る) / `台詞だけ` で地の文を読まない
- `kataribe.tts.voices` (キャッシュ) を壊さない

---

## 査読の反映 (rev2)

| 指摘 | 対応 |
|---|---|
| #1-① / #2 MAJOR-3: 誤字・敬称・一人称が名もない人物へ逃げて照合を素通り | D3 の手順 0 (主人公を指す語) と手順 3 (近い・含む → 却下) |
| #2 CRITICAL-1: `speaker: "player"` の解決先が無い | D3 の手順 0 (`player`・主人公を指す語) と手順 2 (主人公の表示名) で `PLAYER` に正規化 → 照合で却下。表示名を手順 2 に置くのは同名の NPC を曖昧さとして検出するため |
| #1-⑦: 主人公の表示名が空 | 空の名前では一致させない (D3) + PoC |
| #2 MAJOR-1: 表示名の重複 | 手順 2 で `AmbiguousSpeaker` |
| #2 CRITICAL-2: 救済の置き場と順序 | D2 に処理順序。救済は gm_core `resolve` (scenario が要る)、deserializer は手書き Visitor (#93) |
| #1-②: 文字列と配列で名もない人物の扱いが割れる | 意図して残す (D2 に理由)。文字列は話者か語りか判別できないので吹き出しを捏造しない側に倒す |
| #1-③: 型の境界が曖昧・D4 と D8 の矛盾 | D4 に型の表。型が変わるのは `StateDelta.narration` だけ、`TurnOutcome` / `TurnView` は `segments` を追加 |
| #1-④ / #2 MAJOR-2: 試し適用の置き場と outcome | gm_core に `apply_checked` を新設 (apply 本体は不変)。写しの outcome を返し再適用しない + PoC |
| #1-⑤ / #2 CRITICAL-3: `plain` と還流の書式 | `plain_for_gm` を分離し、消費者の割当表 (D4) |
| #2 CRITICAL-4: `kataribe.tts.voices` の衝突 | `kataribe.tts.characterVoices` へ |
| #1-⑥: `台詞だけ` で地の文の台詞が無音 | 割り切りとして明記 + Phase E の測定で緩和策を判断 |
| #1 誤記: 接地表のパス | `crates/` を補った |
| #2 未決 2 の条件: 試行上限での提示 | D3 末尾 (従来どおりターン失敗) |
| #2 未決 6 の条件: 独白 | D5 に 1 行 |

## 未決 → 裁定 (査読 #2 の承認・条件は rev2 で充足。ユーザーの最終確認待ち)

1. キーは `narration` のまま — 承認
2. 不正な話者はデルタ全体を却下 — 承認 (Phase E で却下率を見て再判断)
3. 名もない人物を許す — 条件付き承認 → 手順 3 を入れた
4. 還流書式 `名前 (id):「…」` — 条件付き承認 → `plain_for_gm` を分離した
5. 再開時の語りは v1 平らな文字列 — 承認
6. 心の声・独白は v1 地の文 — 承認 (D5 に 1 行)
7. 名もない人物の声は語り手の声 — 承認

---

## 実装の記録 (Phase 0〜B、2026-10-14)

- **Phase 0**: data_contract `DialogueSegments` 節を凍結。`StateDelta.narration` の行・`SessionSave.carryovers`・`GmTurn.recent_narrations` を追従。
- **Phase A (gm_core `dialogue.rs`)**: `Segment` / `Narration` (手書き Visitor: 配列・文字列・単一オブジェクト・null を受け、段の欄の誤りは理由ごと返す) / `resolve_narration` (救済の分割を含む) / `check_speakers` / `apply_checked` / `plain`・`plain_for_gm`。`RejectReason` 4 種 (ja/en)。`levenshtein` を `pub(crate)` で共有。schema は段の配列だけ (`JsonSchema` を手で実装し `Vec<Segment>` に委ねる)。`StateDelta::new` は `impl Into<Narration>` を受けるので既存の呼び出しは無改修。
  - 実装時の追加: 手順 1 の id 一致は ASCII の大文字小文字を区別しない (`Alice` = `alice`)。手順 3 の照合相手に `player` 自身も入れた (`playr` を書き直させる)。
  - PoC 9 本。**壊して確かめた**: 照合を「開始時の顔ぶれだけ」にすると到着時のテストが落ちる / 確定後に再適用すると同じテストが落ちる (写しを捨てて正本へ適用し直す形は乱数が決定論なので結果が同一 = 検出されないが、バグではない)。
- **Phase B (harness / llm_client / app / CLI)**: `run_turn` を `resolve` → `adjudicate` → `apply_checked` に組み替え (両方の却下理由を 1 回の自己修復に載せる)。`TurnOutcome::Accepted` に `gm_narration` と `segments`。GM_SYSTEM に D5、多人数の接地に「参加者が操作する人物を speaker にしない」。app は `GameSession.last_display` (表示用) を `recent_narrations` (GM 向け) と分けて持ち、決断・対決の結末は両方へ継ぎ足す。CLI も同じ。PoC: harness 4 本 (多人数の照合を空にすると落ちることを確認) + llm_client 2 本 (schema の形 / generate_delta)。
- **副産物 = 潜在バグ 1 件 (failures #112)**: `generate_delta` が `StateDelta::new(narration, ops)` で組み直しており、**GM の `summary` が常に捨てられていた**。実セーブ 4,821 件中 GM の要約は 0 件 (経緯ログは語りの冒頭 80 字の切れ端だった)。delta をそのまま返す形に直した。
- **実 LLM (gpt-6-luna、friday_lemmon、2 ターン、n=1)**: 2 ターンとも却下ゼロで受理。台詞は `岬あかり「…」` (表示用) / `岬あかり (akari):「…」` (GM 向け) に分かれ、経緯ログに GM の要約が入った。
- **観測した既存の弱点 (spec 35 の外)**: 移動したターンに GM が「店内には源蔵の姿は見当たらない」と書いた — 到着先の喫茶店には源蔵がいる (直後に源蔵の登場ビートも発火)。移動ターンの語りを書く時点で、GM は到着先の顔ぶれを知らない (state_brief はターン開始時の場所の顔ぶれしか出さない)。n=1。台詞の照合は開始時 ∪ 適用後で正しく通すが、**地の文の「いない」という断言**は照合の外。

## 実装の記録 (Phase C、2026-10-15)

- **backend**: `SegmentView { speaker, name, icon, text }` を `TurnView.segments` に (地の文 = `name` 空 / 登場人物 = `speaker` が id・`icon` がアセット ID / 名もない人物 = `speaker` null・`name` がラベル)。名前とアイコンは scenario から引く (この場にいる人の一覧からは引かない)。本文には従来の語りと同じ `normalize` (#16)。却下ターンは空。`GameView.cast` (登場人物の一覧、id 順) を Phase D のために足した。
- **frontend**: `LogEntry` の語りに `segments?` を持たせ、`ingestTurn` で段の顔アイコンも先読みして URL へ解決。会話ログは段があれば地の文と吹き出しを順に描く (顔アイコン + 名前 + 吹き出し。アイコンは顔アイコン行と同じ CSS background、無ければ名前の頭 2 字)。段が無い語り (再開の「前回までの語り」・エピローグ) は従来どおり 1 段落。平らな `text` はログ書き出し用に残し、画面には重ねて出さない。多人数のゲストにも同じ DTO が届くので同じ描画になる。
- **PoC**: app 1 本 (段の view と cast) + frontend mount 3 本 (会話ログ 2 本 = 吹き出し・名もない人物の頭文字・平らな文字列を重ねない / 段の無い語りは従来どおり、ストア 1 本 = 段が語りの項目に届きアイコンが URL になる)。**壊して確かめた**: 会話ログの段の分岐を消す / 取り込みで段を渡さない / 先読みから段のアイコンを外す、の 3 つで落ちる。vue-tsc は私のテストの IPC の偽装の型で 1 度落ちた (vitest は緑のまま = 台帳の警告どおり)。
- **目視 (dev サーバ、ストアに段を直接入れて描画)**: 地の文・吹き出し・名前・名もない人物の頭文字が意図どおりに並ぶ。会話ペインはテーマから独立した暗い舞台なので、ライトテーマでも吹き出しは暗い配色 (プレイヤーの吹き出しと同じ色トークン = 舞台の配色に追従する)。**接地の限界**: 試験用の data URL に `'` と `(` を含めると引用符なしの `url()` が壊れてアイコンが出なかった — 顔アイコン行と同じ書き方で、本番のアセット ID は `^[A-Za-z0-9._-]+$` に制限されているので起きない想定 (記号の無い URL で表示を確認)。**✅実機 Green (2026-10-15 ユーザー実測、muse-spark-1.3・金曜日のレモンパイ、3 ターン)**: 実際のアセットの URL で顔アイコン・名前・吹き出しが出て、地の文と台詞が交互に並んだ (GM が台詞を段に分けて書いた = 規律が効いている、n=1)。

## 実装の記録 (Phase D、2026-10-15)

- **tts.ts**: 読み上げの設定に `scope` (`all` = 既定・従来どおり / `dialogue` = 台詞だけ)。キャラ別の声は別キー `kataribe.tts.characterVoices` (`{ [エンジン|URL]: { [パッケージ]: { [id]: 話者 ID } } }`。`kataribe.tts.voices` = 声一覧のキャッシュとは分けた)。段から「何をどの声で読むか」は純関数 `planSegments` (地の文 = 語り手、`dialogue` なら読まない / 登場人物 = 割り当てがあればその声 / 名もない人物 = 語り手)、割り当ての書き換えは純関数 `withCharacterVoice` (空の話者 ID で消し、空になった箱を畳む)。`speakSegments(segments, pkg)` を足し、`speak` も同じ内部の `speakParts` を通す。**声は直列の鎖の中で、変わるときだけ `updateOptions({ speaker })`** — ライブラリは合成のたびに `options.speaker` を読み (`VoiceEngineAdapter.js:138/167`)、`speaker` は共通の更新キーとして許されている (`COMMON_UPDATE_KEYS`) ことを実装で確かめた。鎖は 1 文ずつ合成と再生を待つので前の文に混ざらない。平らな文字列 (エピローグ・段の無い語り) は地の文として扱い、`dialogue` なら読まない。設定の試聴は `test(sample, voice?)` でキャラの声も試せる。
- **store**: `cast` を `GameView.cast` から持つ。受理ターンは段があれば `speakSegments(turn.segments, activePackagePath)`、無ければ従来の `speak`。
- **設定 > サウンド**: 「読み上げる範囲」(台詞だけのときは地の文の「」が読まれない旨を出す)。キャラクターの声は場所を案内する一文だけ。
- **キャラクターの声のパネル** (`CharacterVoicesPanel.vue`、**ユーザーFB 2026-10-15 で設定画面から移設**「サウンド設定にあるより、メッセージエリアのボイスのミュートがあるところにダイアログを呼び出すアイコンがある方がいい」): 会話ペイン右下の読み上げの操作列 (ON/OFF・スキップ) に人物のアイコンを足し、押すと浮遊パネル (参照ストックやプロンプト工房と同じ `FloatingPanel` = 幕を張らず会話を見ながら触れる) が開く。登場人物ごとに 語り手と同じ / 声の一覧 (一覧を持たないエンジン = OpenAI 互換は自由入力)・行ごとに試聴・一覧に無い保存済みの ID も選択肢に出す・声一覧の取得ボタン・どのエンジンの割り当てかを表示 (エンジンや速度は設定 > サウンドのまま)。**浮遊パネルの開閉は store の `floatingPanel` 1 つにまとめた** — 3 枚 (参照ストック・プロンプト工房・キャラクターの声) とも同じ位置に出るので、コンポーネントをまたいで排他にする (従来は ImageControls のローカル状態で 2 枚だけ排他だった)。操作列が 1 ボタン伸びたので画像の操作列を `right-28` → `right-36` へ (dev サーバで 36px 離れていることを実測)。パネルは動的 import の別チャンク (3.5 kB)。
- **PoC**: frontend unit 4 (`planSegments` 2 / 保存形 2) + mount 9 (鎖の中の声の切り替え順 3・パネル 3 (保存と読み戻し / ゲーム前 / 開閉と画像側との排他)・設定画面 1・取り込みの継ぎ目 2)。**壊して確かめた**: 鎖から `updateOptions` を消す / パネルで保存しない / 操作列のボタンをローカルの状態で開く (排他が壊れる) / 取り込みで `speakSegments` の代わりに `speak` を呼ぶ、の 4 通りで落ちる。
- **目視 (dev サーバ)**: 選んだ声が `voicevox|http://localhost:50021` → パッケージ → `akari` に保存され、声一覧のキャッシュは壊れない。移設後のパネルも、操作列のアイコンから開いて 3 人分の行・試聴・エンジン表示が並ぶことを確認。
- **✅実機 Green (2026-10-15 ユーザー実測)**: 複数のキャラに声を割り当て、台詞ごとに声が切り替わって鳴ることを確認 (どのエンジンで聴いたかは未記録)。テストは偽装したライブラリで呼び出しの順序を見ており、実際に鳴ることはこの実測が接地。
- **追補 (2026-10-15 ユーザー要望 3 件)**: ①声の「なし」= 番兵 `VOICE_NONE` を保存し、その人物の台詞を読まない (一覧のあるエンジンは選択肢、自由入力のエンジンはチェック) ②スキップは次のセリフへ = 鎖の文に段の通し番号を持たせ、読み中の段の残りだけを捨てる (`skip()`、世代は進めない。ライブラリの `stop()` は再生中の発話も reject するので鎖は止まらない — `VoiceEngineAdapter.stop()` を実読して確認) ③吹き出しにホバーで出る読み上げボタン = そのセリフを読み直す (範囲と「なし」は見ない)。PoC: unit +1・mount +7 (`tts.skip.mount.test.ts` 3 / パネル 2 / 吹き出し 2)。Red はスキップを世代を進める形に戻す・「なし」の選択肢を消す・ボタンの表示条件を外す、の 3 通りで確認。
- **既知の制限**: `cast` は `GameView` からしか来ないので、キャンペーンで後のモジュールに初めて出る人物は、再開 (またはロード) するまで設定の一覧に出ない (割り当て自体は id で効く)。
