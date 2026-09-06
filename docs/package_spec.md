# Lorekeel パッケージ作成仕様 (LLM 向け)

あなたはこの仕様に**厳密に**従って、Lorekeel (LLM をナレーターにした TRPG エンジン) のシナリオパッケージ一式の YAML ファイルを生成します。
この仕様は Lorekeel エンジンの data_contract (正本) から作者に必要な部分を抜粋したものです。

## 大原則 (これを破るとエンジンがロード時に拒否します)

1. **閉世界**: フラグ・stat・スキル・アイテム・属性・チャレンジは**宣言したものだけが存在**します。宣言していない名前を triggers や gate から参照してはいけません。
2. **正本はエンジン**: 数値・フラグ・位置・所持品の真実はエンジンが握ります。YAML は「宣言と拘束」を書く場所です。
3. **自己完結**: パッケージフォルダの外を参照してはいけません。ファイル参照はすべてフォルダ相対です。
4. **エンジンが検証しない欄** (profile / world / narration / description / hint / epilogue_prompt 等) は語りの素材です。ここに可変の世界状態 (HP や進行フラグ) を書いてはいけません。

## パッケージの構造

```
<パッケージ名>/
  package.yaml        # 必須。世界をまとめる 1 ファイル
  scenarios/          # 必須。シナリオ本体 (entry が指すファイルを含む)
    main.yaml
  characters/         # 任意。1 キャラ 1 ファイル (ファイル名 = EntityId)
    alice.yaml
  memoria/            # 任意。伏線・lore (1 断片 1 ファイル)
    promise.yaml
  campaign.yaml       # 任意。複数シナリオを繋ぐ場合のみ
  images/             # 任意。背景・イベント CG (ID = ファイル名)
  audios/             # 任意。BGM・SE (ID = ファイル名)
```

- フォルダごと zip して配布します (`<パッケージ名>/package.yaml` の形)。
- アセット ID は `^[A-Za-z0-9._-]{1,64}$` のファイル名のみ (日本語ファイル名不可)。
- 実行ファイル・スクリプト (exe / dll / bat / sh / js / wasm 等) は同梱禁止。

### アセットのフォーマットとサイズ (配布容易性のために)

- **画像は WebP を推奨**します。PNG から品質 80 で変換すると 1/10〜1/30 になります (顔アイコンで実測 480KB→約 13KB)。背景・イベント CG・顔アイコンすべて WebP で構いません。
- **音声は Ogg (Vorbis) を推奨**します。BGM は 1〜2MB、SE は数十 KB が目安。WAV は無圧縮で配布に向きません。
- Lorekeel は**拡張子を解釈しません** — `images/`/`audios/` に置いたファイル名をそのまま ID に書けば、WebP/Ogg も PNG/WAV と同じように扱われます。
- **パッケージ全体は数十 MB を超えないこと**。ダウンロードをためらわせるサイズは、それだけで遊ばれる機会を減らします。10MB の画像 1 枚は、ほぼ確実に減量できます。
- 変換例: `magick input.png -quality 80 output.webp` / `ffmpeg -i input.wav -c:a libvorbis -q:a 4 output.ogg`

## package.yaml (PackageManifest)

```yaml
title: 放課後の教室          # 必須。表示名
description: 夕暮れの教室で始まる小さな物語   # 任意。一覧表示用
author: あなたの名前          # 任意
version: "1.0"               # 任意
entry: scenarios/main.yaml   # 必須。開始点 (campaign.yaml か scenarios/xxx.yaml)
world: |                     # 任意。世界観 lore (語りの素材。可変状態は書かない)
  現代日本の高校。放課後の校舎には夕日が差し込む。
player:                      # 任意。主人公を一度だけ宣言 → 全シナリオへ注入される
  name: 主人公
  profile: 二年生。人当たりは柔らかいが芯は強い。   # 語りの素材
  stats: { hp: 10, 度胸: 3 }        # 初期数値 (シナリオ側より package が優先)。境界つき
                                     # { initial, min, max } も可 (initial_stats と同形)
  skills: [観察眼]                   # 初期スキル (union)
  items: [生徒手帳]                  # 初期所持品 (union)
  attributes: { 職業: 生徒 }         # 初期文字列属性 (クラス/職業/種族など)
globals:
  flags: [met_moka]          # パッケージ横断で生きる世界フラグの宣言 (一元宣言)
facts_policy: locked         # 任意。既定 locked。プレイヤーが「既成事実」を宣言できるか
                             # (open にする前に下の「既成事実」節を必ず読むこと)
image_style: 水彩画・淡い色・キャラはデフォルメ   # 任意。挿絵 (プレイヤーが押す画像生成) の画風指針
                             # (下の「挿絵の画風」節を参照)
```

## scenarios/*.yaml (Scenario)

```yaml
title: 放課後の教室
start: classroom                    # 開始 LocationId

# --- フラグの閉世界宣言 ---
allowed_flags: [door_open, met_moka, confession_done]   # 使ってよいフラグ全部
global_flags: [met_moka]            # うちシナリオを跨いで生き残る分 (allowed_flags の部分集合)
persistent_flags: [door_open]       # うち「この場所に戻った時だけ」蘇る分 (再訪もの向け・任意)
flag_rules:                         # set_flag(true) を許す条件 (書かなければ常に可)
  confession_done: { kind: stat_at_least, entity: moka, key: 好感度, value: 30 }
flag_hints:                         # GM への「立てる条件」ヒント (任意・語り素材)
  met_moka: モカと言葉を交わしたら立てる
flag_titles:                        # 人間向け表示名 (任意)
  met_moka: モカとの出会い
hidden_flags: []                    # プレイヤーには隠すが GM は見る「秘密のフラグ」(裏の真相・隠し進行・任意)
internal_flags: []                  # GM もプレイヤーも見ない engine 帳簿フラグ (タイマーの armed フラグ等・任意)

# --- 主人公の初期宣言 (package.yaml の player と union/merge される) ---
initial_stats:
  hp: 10                            # 素の数値 = 下限 0・上限なし (従来どおり)
  SAN: { initial: 60, min: 0, max: 99 }   # 境界つき宣言も可 (上限 99 で頭打ち・CoC の SAN 等)
  所持金: { initial: 100, min: -500 }      # 負の min で「借金」も書ける
initial_skills: []
initial_inventory: []
initial_attributes: {}
hidden_stats: []                    # プレイヤーには隠すが GM は見る「秘密の数値」(隠し好感度・裏の腐敗値・任意)
internal_stats: []                  # GM もプレイヤーも見ない engine 帳簿 stat (タイマー/カウンタ・任意)

# --- 登場キャラ ---
cast: [moka]                        # characters/*.yaml から注入する外部キャラの宣言。
                                    # 空なら外部注入なし。inline (下の characters:) も併用可
characters: {}                      # inline 定義する場合 (CharacterDef 形式、下記参照)
party: []                           # 冒険を共にする仲間 (下記「パーティ」参照・任意)

# --- 場所グラフ ---
locations:
  classroom:
    title: 放課後の教室              # 任意。GUI の現在地に出る表示名 (省略時は id を表示)
    description: 夕日の差し込む教室。机が整然と並ぶ。
    image: classroom.webp           # 任意。images/ 配下のファイル名 (WebP 推奨)
    bgm: evening.ogg                # 任意。audios/ 配下 (Ogg 推奨)
    present: [moka]                 # この場にいる NPC (明示宣言。空/未記載なら誰もいない)
    items:
      # アイテムの書き方は二形式あり、**混ぜてはいけない**:
      #   A) Gate 直書き — 取得条件だけ書く形。take は書けない (常に once 扱い)
      #   B) { when: <Gate>, take: once|infinite|fixed } — take を指定するなら必ずこの形。
      #      when は省略可 (省略 = 常に取得可)
      鍵: { kind: flag_is, key: met_moka, value: true }     # A) Gate 直書き (= take: once)
      ジュース: { when: { kind: always }, take: infinite }   # B) 何度でも取れる
      リモコン: { take: fixed }                              # B) when 省略。備え付け (持ち運べない)
      # ⚠ よくある誤り: Gate のキー (kind 等) と take を同じ階層に並べる。
      #    Gate 形式として読まれ、take は「不明なフィールド」として黙って無視される
      #    (fixed のつもりが once になる)。take を書くときは Gate を必ず when: で包むこと。
      #   ❌ 誤り: 天体写真の束: { kind: always, take: fixed }
      #   ✓ 正解: 天体写真の束: { when: { kind: always }, take: fixed }
      #   ✓ 正解: 天体写真の束: { take: fixed }              # 条件が「常に」なら when ごと省略が最短
    exits:
      - to: hallway
        gate: { kind: flag_is, key: door_open, value: true }   # 省略時は常に通れる
  hallway:
    description: 静まり返った廊下。
    present: []
    exits:
      - to: classroom

# --- ゴール (エンディング)。goal か goals のどちらか必須 ---
goals:
  - id: confession_ending           # 機械用 id (スペース禁止)
    when: { kind: flag_is, key: confession_done, value: true }
    title: 夕暮れの告白              # 人間向け表示名
    hint: モカと仲良くなろう          # プレイヤー向けの道しるべ (ネタバレしない範囲で)
    narration: |                    # 到達時の結末ナレーション
      夕日が二人の影を長く伸ばした。
    visible: true                   # false = 到達まで隠すシークレットエンド
    epilogue_prompt: |              # 任意。エピローグの生成指示 (本文ではない)
      二人のその後を、季節の移ろいとともに短く。

# エピローグ (epilogue_prompt): 書くと、その goal でゲームが終わる時に GM が
# 「このプレイスルー固有のエピローグ」を旅路の記録 (あらすじ・経緯) から自動で書き、
# 結末ナレーション → 到達バナー → エピローグ、の順で幕が下りる。
# - 中身は「何をどう語ってほしいか」の指示文。エンディングごとに違う余韻を演出できる。
# - GM は「起きたことは記録のとおりに・その後 (後日談) は自由に」の規律で書く。
# - narration (結末文) は必ず書くこと — エピローグ生成が失敗した時の幕であり、
#   「封印か討伐か死か」という結末の意味を GM に伝える土台 (無いと警告が出る)。
# - キャンペーンの途中ゴール (次の章へ遷移する goal) では発火しない。終端のみ。

# --- 反応ビート (トリガー)。条件成立でエンジンが確実に起こす筋書き ---
triggers:
  - id: moka_notices
    when: { kind: stat_at_least, entity: moka, key: 好感度, value: 10 }
    effects:                        # 機械的効果 (authored 専権。下記「effects で使える op」参照)
      - { op: set_flag, key: met_moka, value: true }
    narration: モカがふと顔を上げ、こちらを見た。
    repeatable: false               # 既定 false = 一度きり。true = 条件が再成立したら再発火
    recall: promise                 # 任意。memoria/*.yaml の伏線を想起させる cue
    image: event_moka.webp          # 任意。発火時のイベント CG (WebP 推奨)
    image_hold: show                # 任意。show=消すまで残す / hide=消す (省略=そのターンだけ)
    sound: chime.ogg                # 任意。発火時の SE (Ogg 推奨)

# --- 技能判定 (チャレンジ)。判定の素性と帰結は作者が握る ---
challenges:
  lockpick:
    description: 錆びた鍵穴をこじ開ける          # GM への提示文
    requires: { kind: has_item, item: 鍵 }      # 任意。挑む前提条件 (未達なら挑戦不可)
    max_per_turn: 1                              # 任意。1 ターンに挑める回数の上限 (省略 = 無制限)
    stat: 度胸                                   # 任意。省略なら純粋な運試し
    sides: 20                                    # 1d20
    count: 1                                     # 任意。ダイス個数 (3 なら 3d20 の合計)
    times: 1                                     # 任意。出目の乗数 (3D6×5 なら count:3 sides:6 times:5)
    dc: 12                                       # (合計×times)+修正 >= dc で成功
    modifiers:                                   # 任意。条件付き有利/不利
      - { when: { kind: has_skill, skill: 観察眼 }, bonus: 3 }
    on_success:
      flag: door_open                            # 帰結フラグ (allowed_flags 宣言必須)
      narration: 重い音を立てて錠が外れた。
      sound: unlock.ogg                          # 任意。結末の効果音 (audios/ 配下・毎回・同ターンに再生)
    on_failure:
      narration: 鍵穴はびくともしない。
    tiers:                                       # 極 (大成功/大失敗)。素の出目で判定
      crit_fail:
        natural: min                             # d20 なら 1
        flag: alarm_triggered
        narration: 手が滑り、大きな音が廊下に響いた。
        sound: alarm.ogg                         # 任意。極の効果音 (tier の sound は通常成否より優先)
```

#### イベント CG を出しっぱなしにする (`image_hold`)

`image` だけ書いた CG は**そのターンだけ**出て、次にプレイヤーが何か打つと場所の背景へ戻ります
(一瞬の演出向け)。**シーンとして出しっぱなしにしたい**ときは `image_hold` を使います。

```yaml
triggers:
  - id: 告白
    when: { kind: stat_at_least, entity: モカ, key: 好感度, value: 60 }
    narration: 夕日が二人の影を長く伸ばした。
    image: 告白シーン.webp
    image_hold: show            # 消すまで出し続ける

  - id: 教室を出る
    when: { kind: flag_is, key: 下校, value: true }
    narration: 鞄を取って、二人は教室を出た。
    image: 告白シーン.webp      # 任意。書くとこの CG のときだけ消す
    image_hold: hide            # 場所の背景へ戻す
```

- **消すのは作者の責任です。** `show` した CG は場所を移動しても残ります (だから移動を挟む
  長いシーンが書けます)。**どこかで `hide` を書き忘れると、ずっとその絵のままになります。**
- **別の CG を `show` すると差し替わります。** 前の CG を先に `hide` する必要はありません。
- `hide` に `image` を併記すると、**その CG が出ているときだけ**消します。すでに別の絵に
  差し替わっていれば何も起きません (古い取り消しが後のシーンを壊しません)。
- セーブに含まれるので、続きから始めても出しっぱなしの絵はそのままです。章 (モジュール) が
  変わるときはリセットされます。

#### 回数の上限 (`max_per_turn`) — 同じ挑戦を 1 ターンに何度も選ばせない

- **`max_per_turn: 1`** を書くと、その challenge は**同じ人物が 1 ターンに 1 回まで**しか挑めません
  (省略 = 無制限。既存のパッケージは何も変わりません)。
- **なぜ要るか**: `attempt_challenge` は GM (AI) が選べる op のうち**唯一「作者の効果を持つ」もの**です。
  `on_success` に `adjust_stat hp +5` のような効果があると、GM が 1 ターンに同じ挑戦を 5 回並べれば
  効果は 5 倍になります (実測で hp 10 → 25 が受理されました)。GM は効果の**量**を書けませんが、
  **回数**で実質的に量を決められる隙があり、しかも却下されないので気づけません。
- **`requires` では塞げません。** `requires: { kind: not, of: { kind: flag_is, key: rested } }` のように
  帰結フラグを前提に書いても、ダイスの帰結は同じターンの中では確定しないので、同じターン内の
  2 回目・3 回目は素通りし、止まるのは次のターンからです。ターンの内側の回数は `max_per_turn` だけが縛ります。
- **数えるのは (挑戦, 挑む人物) の組**です。多人数卓で仲間 2 人が同じ挑戦に 1 回ずつ挑むのは正当なので、
  挑戦単位では数えません。
- 上限を越えた提案は却下され、GM には「次のターンなら挑める」と伝わります。上限は GM への提示にも
  【1 ターンに N 回まで】と**先に**出るので、越えてから叱る形にはなりません。
- **`0` は書かないでください。** 提示には出るのに必ず却下される「死んだ挑戦」になり、検分で警告が出ます。

#### 判定主体の固定 (`entity`) と、幅のある大失敗/大成功 (`threshold`)

- **`entity: hina`** を challenge に書くと、その判定は**必ずその人物の stat で振られます**
  (GM が判定を選ぶとき entity を省略・誤指定しても、作者の指定が勝ちます)。
  「主人公が会社にいる間、裏でヒナの様子を判定する」のような **NPC の裏判定**にはこれを必ず書いてください
  — 書かないと GM は既定で主人公の stat を引こうとし、未宣言 stat の却下を繰り返します。
  指定した人物がその stat を宣言していない場合はロード時エラーで名指しされます。
- tier の `natural` は `min` (ダイスが全部 1 = 素の合計が最小。1d20 なら出目 1、3d6 なら合計 3) /
  `max` (全部が最大 = 合計 `count×sides`) に加えて、
  **`at_most` / `at_least` + `threshold`** で幅を持たせられます (d100 のように sides が大きい盤面では
  `min` = 1% でほぼ発火しないため)。判定は**素の出目** (stat 修正や modifiers が乗る前) で、
  `threshold` は `1〜count×sides` の範囲必須 (範囲外・欠落はロード時エラー)。
  なお `count` が 2 以上のときは合計の下限が `count` になるので、`at_most` に `count` 未満の値を
  書くと**一度も発火しません** (ロード時エラーにはならないので、作者側で気をつけてください)。
  複数の tier に該当した場合は tier 名の昇順で最初の 1 つだけが発火します。

```yaml
  hina_work:                       # 裏で NPC を判定する例
    entity: hina                   # 判定主体を hina に固定
    stat: 主人公❤                  # hina 側の characters/hina.yaml で宣言しておくこと
    sides: 100
    dc: 50
    on_failure:
      effects:
        - { op: adjust_stat, entity: hina, key: コウジ❤, delta: 5 }
      narration: ヒナはコンビニでコウジと一緒に働いていた。
    tiers:
      crit_fail:
        natural: at_most           # 「threshold 以下」で発火 (at_least なら「以上」)
        threshold: 10              # d100 の 10 以下 = 下位 10%
        narration: ヒナはコウジと、とても親しくなっていた……
```

#### d100 ロールアンダー判定 (`resolution: percentile` — CoC 系の判定様式・任意)

challenge に `resolution: percentile` を書くと、`1d100 ≤ 目標値 (stat 現在値 + modifiers)` の
**ロールアンダー判定** (低いほど良い) になります。成功度 (degree) はエンジンが計算します:
**クリティカル** (出目 01) / **イクストリーム成功** (≤ 値÷5) / **ハード成功** (≤ 値÷2) /
**成功** (≤ 値) / 失敗 / **ファンブル** (目標値 50 未満なら 96–100、50 以上なら 100)。

```yaml
  spot_hidden:
    resolution: percentile
    description: 部屋を注意深く調べる目星ロール
    stat: 目星                        # percentile では必須 (sides / dc は書かない)
    on_success: { flag: found_diary, narration: 抽斗の奥に日記を見つけた。 }
    on_hard:    { flag: found_diary, narration: 机上の癖から持ち主の利き手まで見抜いた。 }
    on_failure: { narration: 埃が舞うだけで、何も見つからない。 }
    on_fumble:
      narration: 書架を倒してしまった。崩れた本の下から、見てはならない図版が覗く。
      effects:
        - { op: roll_stat, key: SAN, count: 1, sides: 3, negate: true }   # SAN 1d3 減
```

- degree 別スロット `on_critical` / `on_extreme` / `on_hard` / `on_fumble` は任意。書かなかった
  degree は critical→extreme→hard→`on_success`、fumble→`on_failure` の順でフォールバックします。
  **適用されるのは該当した 1 スロットだけ**なので、degree 別スロットを書くならフラグは
  各スロットに重ねて書いてください (hard だけに書くと regular 成功でフラグが立ちません)。
- `tiers` とは併用不可 (ロード時エラー)。percentile の `modifiers` は**目標値に加算**されます。
- シナリオ先頭に **`check_style: percentile`** を書くと、GM の**即興判定**も d100 ロールアンダーに
  なります (「目星 60 で振る」の様式に盤面ごと統一)。書かなければ従来の加算式のまま。
- 可変量ダイス **`roll_stat`** (上の例) は effects 専用の op で、`count`d`sides`+`bonus` を振って
  stat に適用します (`negate: true` で減算)。SAN 減少・変動ダメージに使います。

#### 確定行動 (`resolution: none`) — ダイスを振らない「装備する」「使う」

**装備・使用・切替のような、失敗のない行動**は `resolution: none` で書きます。エンジンは
ダイスを振らず `on_success` を必ず適用します (判定していないので 🎯 の判定行も、ダイスを
開くカードも出ません)。

```yaml
challenges:
  equip_hanakanmuri:
    resolution: none
    description: 【装備する】地縛りの花冠      # GM への提示文 (行動の選択肢として出る)
    requires:                                  # 前提。満たすまで選べない
      kind: all
      of:
        - { kind: has_item, item: 地縛りの花冠 }
        - { kind: flag_is, key: eq_hanakanmuri, value: false }   # 二重装備を防ぐ
    on_success:
      flag: eq_hanakanmuri
      narration: コンの頭に花冠を乗せた。
      effects:
        - { op: adjust_stat, entity: kon, key: 好感度, delta: 3 }
```

- **`sides` / `dc` / `stat` / `expr` / `modifiers` / `tiers` / degree スロット / `on_failure` /
  `pushable` は書けません** (判定しないので無意味 — 書くとロード時エラー)。書けるのは
  `description` / `requires` / `on_success` だけです。
- **`sides: 1` / `dc: 1` で「必ず成功する判定」を作らないでください。** 意味のない判定行が
  会話ログに出て、ダイス演出も誤って発火します。`resolution: percentile` の盤面では
  そもそもロードできません。確定行動は `resolution: none` で書きます。
- 確定行動は完全に決定論なので、**同じターンに続けて別の行動を束ねられます**
  (「装備する → 移動する」が 1 ターンで通る)。ダイスを振る判定は結果が出るまで先へ進めないので、
  次のターンに分かれます。

**なぜトリガーではなく確定行動なのか (重要)**

「フラグを立てる → トリガーが効果を出す」で書きたくなりますが、**トリガーがそのフラグを
書き戻す (リセットする) と、GM はそのフラグを二度と立てられなくなります**。トリガーや
チャレンジが書き込むフラグは「作者専用」になり、GM の操作対象から外れる仕組みだからです
(筋書きの先取りを防ぐため)。

```yaml
# ✗ これは動きません (GM が eq を立てられなくなる)
triggers:
  - id: wear
    when: { kind: flag_is, key: eq, value: true }
    effects:
      - { op: adjust_stat, key: 魅力, delta: 3 }
      - { op: set_flag, key: eq, value: false }   # ← この書き戻しで eq が作者専用になる
```

繰り返せる行動の起点は**確定行動**にしてください。効果の続きをトリガーで書くのは問題
ありません (確定行動が立てたフラグをトリガーが読む形にする)。

#### 式修正 (`expr`) — 判定の修正値/目標値を stat の式で書く (任意)

`stat` の代わりに **`expr`** を書くと、`(CON + SIZ) / 2` のような整数式が
修正値 (通常判定) / 目標値 (percentile) になります。式は**判定のたびに現在値で評価**される
ので、CON が削られれば補正も自動で落ちます (手書きの派生値と違う「生きたシート」)。

```yaml
challenges:
  club:
    description: 棍棒で殴る
    expr: "(CON + SIZ) / 2"      # 修正値 = 式 (stat とどちらか一方だけ)
    sides: 20
    dc: 15
  dodge:
    resolution: percentile
    expr: "DEX * 2"              # 目標値 = 式
contests:
  grapple:
    opponent: ghoul
    player_roll: { expr: "(STR + SIZ) / 4", sides: 20 }   # 対決の振り方にも書ける
    opponent_roll: 組みつき
```

- 使えるのは **整数・stat 名 (日本語可)・`+ - * /`・括弧** だけ。除算は端数切り捨て (CoC 準拠)。
- ダイス側の演算は `count`/`times` (challenge/振り方の欄) で書く — `count: 3, sides: 6, times: 5` =
  **3D6×5** (乗算は素の合計だけに掛かり、stat/modifiers の修正は後から加算)。大失敗/大成功
  (`tiers`) は素の合計で判定され、`threshold` の範囲は `1〜count×sides` になる。
  percentile は 1d100 固定で `count`/`times` は書けない。
- **`count` を使うのは「合計の分布そのもの」を難易度設計に使う系だけ**にしてください
  (2d6+修正、3d6 ロールアンダー等)。`count: 3, sides: 6` を `sides: 18` で代用してはいけません —
  合計は 3〜18 の釣鐘型ですが 1d18 は 1〜18 の一様分布で、最低値も平均も散らばりも別物になります。
  エンジンは書かれたとおりに振るだけなので、これはロード時エラーにもプレイ中の却下にもなりません。
- **d100 は `sides: 100`(1d100) と書いてください。** CoC 系の卓で 10 面体を 2 個振るのは
  十の位と一の位の**位取り**であって合計ではないため、結果は 1〜100 の一様分布 = 1d100 と同じです
  (`count: 2, sides: 10` は 2〜20 の合計になり、まったく別の判定になります)。
- **GM の即興判定 (`check` / `check_under`) は常に 1 個のダイス**です (`count` は書けません)。
  合計ダイスを使う判定を出したい場面には、その式を書いた `challenge` を必ず用意してください
  — 挑戦が無いと GM は 1 個のダイスで代用しようとします。
- 式が参照する stat は**判定主体の宣言済みキー**であること (未宣言はロード時エラー)。
- `stat` と `expr` の併記はロード時エラー (どちらか一方)。
- ダメージ・ボーナスのような**帯テーブル** (STR+SIZ 125 以上で +1d4 等) は式では書けません —
  `modifiers` の条件付き bonus (`when: { kind: stat_at_least, ... }`) で帯を書いてください。

#### 失敗への決断 — プッシュと差分買い (任意)

失敗した判定に、プレイヤー自身が「**受け入れる / 押して振り直す / 支払って成功に変える**」を
選ぶ決断パネルが出せます (AI は関与しません)。CoC のプッシュロールと幸運消費に相当しますが、
**支払い元の stat は自由**です (幸運でも、所持金でも、霊力でも)。

```yaml
# シナリオ直下 (差分買いの opt-in。書かなければ「買う」は出ない)
spend_rules: { from: 幸運, rate: 1 }   # from = player の宣言済み stat / rate = 差分 1 あたりの支払い
push_cost: { from: hp, amount: 1 }     # 任意。押す代償の上乗せ (書かなければ無償 = CoC 原典どおり)

challenges:
  lockpick:
    # ... (通常の定義に追記する)
    pushable: true                     # 既定 false。true でこの判定の失敗が「押せる」
    on_push_failure:                   # 押して再失敗した時の帰結 (無ければ on_failure へ)
      flag: alarm_triggered
      narration: 無理にこじった針が折れ、鍵穴の奥で高い音が鳴り響いた。
    spendable: true                    # 既定 true (spend_rules のある盤面で個別に禁じる時だけ false)
```

- **押した失敗はより悪く書く** (`on_push_failure`) のが作法です。振り直しは 1 度だけで、
  結果は成否に依らず確定します。
- ファンブルと tier 該当 (大失敗等) は押せず・買えず、そのまま確定します (逃れられない)。
- percentile ではハード/イクストリーム成功まで**買い上げ**られます (費用 = 出目との差分 × rate)。
  支払える範囲の選択肢だけが提示されます。
- **一発勝負にしたい判定 (SAN ロール・対決の決着) には `pushable` を付けない**でください。
- 支払いは stat の宣言 `min` まで。`from: hp` + `min: 0` + 敗北 goal を組めば
  「命を削って成功を買う」盤面も書けます。
- 注意: `pushable`/`spendable` な challenge は、作法 1 の例外 (両帰結に共通する効果の同ターン束ね)
  の**対象外**になります (帰結が決断まで確定しないため。束ねたい日次フラグ等の challenge には
  `pushable` を付けないのが無難です)。

#### 対決 (`contests`) — 決着まで AI を介さないラウンド戦 (任意)

雑魚戦・対抗ロールのための**一括型**の戦い方です。GM が対決を「開く」と、以後は
プレイヤーとエンジンがラウンド制で直接振り合い (⚔ ボタン)、決着まで **AI を一切呼びません**
(何交換あってもトークン消費ゼロ。決着の要約 1 行だけが次の GM ターンに渡ります)。
ボス戦のように**毎交換を GM に語らせたい**場面は、従来の challenge (1 判定 = 1 ターン) で
書いてください — **どちらの刻みで戦うかは作者が決めます** (プレイヤーにも AI にも選ばせない)。

```yaml
characters:
  gravel:
    name: 石くれの群れ
    stats:
      HP: { initial: 6, min: 0 }
      腕力: { initial: 8 }
    rolls:                             # このキャラの「振り方」テンプレート (使い回せる)
      体当たり: { stat: 腕力, sides: 20 }

contests:
  gravel_brawl:
    description: 石くれの群れを蹴散らす   # GM への提示文
    opponent: gravel                     # 相手 (既知のキャラ必須)
    player_roll: { stat: STR, sides: 20 }  # player 側はインラインで書く
    opponent_roll: 体当たり               # 相手側はテンプレート名でもインラインでも
    requires: { kind: flag_is, key: battle_open, value: true }   # 任意。解禁条件
    on_win:                              # 1 交換の帰結 (player 視点)。毎交換適用される
      narration: 拳が群れの芯を捉えた。
      effects:
        - { op: roll_stat, entity: gravel, key: HP, count: 1, sides: 6, negate: true }
    on_lose:
      narration: 石の礫が脛を打った。
      effects:
        - { op: roll_stat, key: HP, count: 1, sides: 3, negate: true }
    until:                               # 決着条件 (毎交換後に評価)。省略 = 1 交換で終わる
      kind: any
      of:
        - { kind: stat_at_most, entity: gravel, key: HP, value: 0 }
        - { kind: stat_at_most, entity: player, key: HP, value: 0 }
    max_rounds: 20                       # 交換回数の上限 (必ず書く。無限の対決を作らない)
```

- 勝敗は additive なら**合計の比較**、`resolution: percentile` なら**成功度の比較**
  (同成功度は技能値の高い側が勝つ = CoC の対抗ロール準拠)。様式は contest 単位で
  双方に適用されます。`on_tie` (引き分けの帰結) も書けます。
- 帰結フラグは `allowed_flags` 宣言必須で、**筋書きの専権** (GM は set_flag できません)。
- `until` を書き漏らしても、goal (HP0 の敗北条件など) に達すれば対決は必ず閉じます —
  ただし **`until` と `max_rounds` は必ず書く**のが作法です。
- 対決の帰結 effects に `attempt_challenge` / `attempt_contest` は書けません (ロード時エラー)。
- プッシュ/差分買い (前節) は対決のラウンドには**効きません** (対抗ロールは押せない)。

### Gate (条件) の語彙

14 種類 (エンジンの型から機械生成 — **これ以外の `kind` は存在しない**)。`entity` は省略時に主人公 (`player`)。

| kind | 欄 | 意味 |
|---|---|---|
| `all` | `of` | すべての子条件が通る (AND)。 |
| `always` | — | 常に通る。 |
| `any` | `of` | いずれかの子条件が通る (OR)。 |
| `attribute_is` | `entity`, `key`, `value` | 指定キャラの文字列属性が value と一致する (クラス/職業条件)。未設定は空文字扱い。 「魔法剣士なら〜」のように転職後の状態を縛れる。entity 省略時は主人公。 |
| `flag_is` | `key`, `value` | 指定フラグが指定値である。 |
| `has_item` | `entity`, `item` | 指定キャラが指定アイテムを所持している。entity 省略時は主人公。 |
| `has_skill` | `entity`, `skill` | 指定キャラが能力を獲得済みである (能力条件)。閉世界: 宣言/開花した能力のみ true。entity 省略時は主人公。 |
| `has_voted` | `entity` | 指定キャラの票が現在の票箱に入っている (spec 06 / #38)。entity 省略時は主人公。 「プレイヤーが投票したら開票」をイベント駆動で書く述語 — resolve_vote が票を リセットするので開票後は自然に偽へ戻り、repeatable トリガーは次サイクルで再武装する。 タイマー (turns_since) と any で束ねれば「票が入るか N ターンで強制開票」も書ける。 |
| `location_is` | `at` | 指定の場所にいる。 |
| `not` | `of` | 子条件の否定 (NOT)。all(AND)/any(OR) に対して否定を与え、ブール代数を閉じる。 「鍵を持っていない」= { kind: not, of: { kind: has_item, item: 鍵 } }。has_item/has_skill/ attribute_is/location_is など任意の gate を否定でき、フラグでの二重管理が要らない。 |
| `presence_is` | `entity`, `present` | 指定キャラがこの場にいる (spec 04 の実効 presence = Location.present ± overrides)。 present: false で不在を直接問える (not で包まなくてよい)。  用途の核心は筋書きのビートを不在のキャラに出させないこと — 「〇〇が帰っていった」の ような退場の語りを、その場にいない相手に対して発火させない。譲渡や会話の前提にも使える。  実効 presence は scenario 側の土台 (Location.present) と state 側の override の合成なので、 この gate だけは Gate::eval が scenario を要る (state に写す案は、セーブに複製が残って パッケージ更新で作者が present を直しても古いセーブが古い顔ぶれのままになるため採らない)。 entity は必須 (主人公は常に居るので既定に意味がない)。 |
| `stat_at_least` | `entity`, `key`, `value` | 指定キャラの stat が value 以上である (数値条件)。未設定は 0 扱い。entity 省略時は主人公。 |
| `stat_at_most` | `entity`, `key`, `value` | 指定キャラの stat が value 以下である (Gate::StatAtLeast の双対)。未設定は 0 扱い。 hp は 0 クランプなので stat_at_most hp 0 が「気絶/死」を表せる (HP0 End を goal に書く経路)。 entity 省略時は主人公。 |
| `turns_since` | `entity`, `key`, `turns` | 指定キャラの stat に刻まれたターンから turns ターン以上経過している (現在turn - stat >= turns)。StateOp::RecordTurn と対で 「〇〇から N ターン後に発火」を組む。stat 未設定 (=0) だと turn>=turns で誤発火しうるので、 「〇〇が起きた」フラグと all で束ねるのが定石。entity 省略時は主人公。 |

`not` は**どの条件でも否定できます**。「〜を持っていない」「〜がまだ立っていない」「その場所に
いない」を、フラグでの二重管理なしに直接書けます。

```yaml
# お守りを持っていなければ襲われる
when:
  kind: all
  of:
    - { kind: location_is, at: 森 }
    - { kind: not, of: { kind: has_item, item: お守り } }
```

`flag_is` の `value: false` や `stat_at_most` でも部分的に否定は書けますが、あれは個別の条件に
生えた反対語であって、`has_item` や `has_skill` には対応物がありません。`not` はその一般形です。

#### キャラファイルを足すだけで動かす (`"*"`)

「場所と雰囲気だけ書いておいて、あとはキャラを差し替えて遊ぶ」——そういう**環境シナリオ**を
作るときは、`cast` と `present` に `"*"` を書きます。

```yaml
cast: ["*"]                              # characters/ に置いた yaml 全員が登場人物になる
locations:
  居間:
    description: 陽の差す居間。
    present: ["*"]                       # この場に全員いる
```

こう書いておけば、遊ぶ人は `characters/` に yaml を一つ置くだけで済みます (シナリオ側を
編集しなくてよい)。

- `present: ["*"]` は**土台**なので、`set_presence` の上書きはその上に重なります
  (「全員いる。ただし今日は一人出かけている」が書けます)。
- `"*"` を書かない場所は**今までどおり無人**です。全員いるのは、そう書いた場所だけです。
- `cast` には明示 id と混ぜて書けます。混ぜた場合、書き間違えた id は従来どおり
  ロード時エラーになります。
- `"*"` という名前のキャラは作れません (予約語・ロード時エラー)。

環境シナリオには**終わり方**も入れておくと収まりが良くなります。ゴールが無いとエピローグも
出ないので、「プレイヤーが終わりにしたいと言ったら幕」を一つ書いておく形です。

```yaml
allowed_flags: [終幕]
flag_hints:
  終幕: プレイヤーが物語を終える意思を示したとき（「そろそろ終わりにしよう」等）
goals:
  - id: 幕引き
    title: 幕引き
    when: { kind: flag_is, key: 終幕, value: true }
    narration: 灯りが落ちる。今日の話はここまで。
    epilogue_prompt: この時間に何が積み重なったかを、静かに振り返って締めくくれ。
```

早すぎる幕引きが心配なら `flag_rules` で下限を掛けられます。下の書き方は
「`開始` を一度も `record_turn` しない」ことを利用して、実質「10 ターン目以降でないと
立てられない」になります。

```yaml
flag_rules:
  終幕: { kind: turns_since, entity: player, key: 開始, turns: 10 }
```

#### いるかどうかで縛る (`presence_is`)

**その相手がこの場にいるときだけ**、という条件です。いちばん効くのは**筋書きのビートを
不在の相手に出させない**用途 — 「〇〇が手を振って帰っていった」を、そもそもその場にいない
相手に対して出さないようにできます。

```yaml
triggers:
  - id: 見送り
    when: { kind: presence_is, entity: モカ }        # 居るときだけ発火
    narration: モカは手を振って帰っていった。
```

- `present: false` で「**いない**」を直接書けます (`not` で包む必要はありません)。
- `entity` は省略できません (主人公は常にいるので、既定にする意味がないためです)。
- 見ているのは**実効 presence** です。場所の `present` に書いた顔ぶれに `set_presence` を
  重ねた結果で、**来訪者 (`volatile: true`) が場所を離れて消えれば、この条件も偽に戻ります**。
- ⚠ `characters` に無い相手を指すと、この条件は**永久に成立しません**
  (`present: false` なら逆に常に成立します)。ロード時に警告が出ます。

譲渡や会話の前提にも使えます。「その場にいる相手にしか渡せない」を条件で縛れます。

#### パーティ (`party`) — 「仲間の誰かが持っているか」で問う (任意)

`has_item` / `has_skill` の `entity` は既定で主人公だけを見ます。仲間が鍵を持っていると、
条件は**却下すら起きずただ真になりません** (静かに詰みます)。シナリオ直下に `party` を
宣言すると、`entity: party` で「主人公 + 仲間の誰か一人でも」を問えます。

```yaml
party: [レイ, ミナ]                  # characters に宣言済みの id だけ。主人公は書かない

# 「パーティの誰かが鍵を持っていれば開く」
exits:
  - to: 宝物庫
    gate: { kind: has_item, entity: party, item: 錆びた鍵 }

# 「誰か一人でも解錠を使えれば挑める」
challenges:
  pick_lock:
    requires: { kind: has_skill, entity: party, skill: 解錠 }
```

- **所持品はプールされません。**「誰が持っているか」はそのままで、**問い方だけ**が集団になります。
  共有インベントリにすると「〇〇がない → 私が持ってる → 渡して → もう一度」という持ち替え作業が
  発生し、冒険が物流管理になるためです。
- `party` に書けるのは `characters` に宣言済みの id だけです (幻の仲間はロード時エラー)。
- `party` という名前のキャラは作れません (予約語・ロード時エラー)。
- 複数人プレイでは、この `party` がプレイヤーに割り当て可能なキャラの候補にもなります
  (主人公はホストが操作します)。人間が足りずに埋まらない席は、GM が声を当てる NPC 仲間として
  そのまま party に残ります。
- 宣言しなければ従来どおり (`entity: party` は主人公だけを見る形に縮退) です。

### triggers / challenges の effects で使える op

表の「作者のみ」の op は trigger / challenge の effects からだけ使えます (GM が提案しても却下されます)。
それ以外の op は GM (AI) の提案でも effects でも使えます。

19 種類 (エンジンの型から機械生成 — **これ以外の `op` は存在しない**)。「作者のみ」は trigger / challenge の effects からだけ使え、GM (AI) が提案すると却下される。

| op | 欄 | 誰が使えるか | 意味 |
|---|---|---|---|
| `add_item` | `item` | GM の提案も effects も可 | player が現在地からアイテムを拾う (世界 → player)。 |
| `adjust_stat` | `delta`, `entity`, `key` | GM の提案も effects も可 | stat への加減 (+/−)。エンジンが clamp(current + delta) を計算する。 LLM は変化量(意図)だけを提案し、結果の値は持てない。entity 省略時は主人公。 |
| `attempt_challenge` | `challenge`, `entity` | GM の提案も effects も可 | authored challenge への挑戦。LLM は challenge を「選ぶ」だけ — 判定の stat/sides/dc も、 大失敗/大成功(tier)とその帰結フラグも、すべて crate::Scenario の authored 定義側にある (LLM は帰結を持てない＝閉世界)。engine が 1d{sides} + stat修正 vs dc を振り、natural 値が tier に該当すれば authored な帰結フラグを直書きする (resolution: percentile なら d100 ロールアンダー + degree 別帰結、spec 16)。未宣言 challenge は却下。entity 省略時は主人公。 |
| `attempt_contest` | `contest` | GM の提案も effects も可 | authored contest (対決) の開始 (spec 18 Phase C)。LLM は対決を「開く」だけ — 双方の振り方 (RollSpec)・帰結・決着条件はすべて authored 定義側にある。開始後の ラウンドは LLM を介さず engine とプレイヤーが直接回す  = 雑魚戦のトークンを消す一括型 (cadence はこの機構、逐次型は従来の challenge)。 |
| `cast_vote` | `target`, `voter` | GM の提案も effects も可 | 投票の意図 (spec 06 Phase C)。voter が target の処刑/襲撃に票を入れる。 LLM 提案可 — ただし受理は「voter/target 生存 + vote_rules のいずれかに合致 (デフォルト拒否)」をエンジンが裁く。一人一票 (GameState::votes は voter キーの map = 再投票は上書き)。開票は StateOp::ResolveVote の専権。voter 省略時は主人公。 |
| `check` | `dc`, `entity`, `sides`, `stat` | GM の提案も effects も可 | 技能判定。エンジンが 1d{sides} + entity の stat 修正 を振り、total >= dc で成否を裁く。 LLM は出目も合計も主張できない (op 構造上不可能)。stat 未宣言は却下。entity 省略時は主人公。 |
| `check_under` | `entity`, `key` | GM の提案も effects も可 | d100 ロールアンダー即興判定 (spec 16)。エンジンが 1d100 を振り、目標値 = その entity の stat 現在値、roll <= 目標値 で成功。成功度 (degree: critical/extreme/hard/regular/ failure/fumble) もエンジンが決定論で計算する — LLM は出目も成功度も持てない。 帰結は持たない (成否+degree の surface のみ。機械的帰結は authored challenge で書く)。 key (技能 stat) 未宣言は却下。entity 省略時は主人公。 |
| `give_item` | `from`, `item`, `to` | GM の提案も effects も可 | アイテムを譲渡する。from が所持していなければ却下 (持っていない物は渡せない)。 to は既知の entity でなければ却下。from 省略時は主人公。 |
| `grant_skill` | `entity`, `skill` | 作者のみ (effects) | 能力の付与 (開花)。authored トリガーの専権 — LLM が提案すると adjudicate が却下する (メアリー・スー遮断)。trigger effects は apply_ops 直行なので付与できる。entity 省略時は主人公。 |
| `move` | `to` | GM の提案も effects も可 | 現在地から to へ移動する。LLM の提案は現在地の exits に在り gate を満たす行き先だけ 受理 (それ以外は却下)。トリガー効果からは出口も gate も見ずに運ぶ (authored 専権の一貫 — 落とし穴・転移・場面転換)。移動で揮発 presence (来訪者) は破棄される。 |
| `record_turn` | `entity`, `key` | 作者のみ (effects) | 現在ターンを stat に刻む (タイムスタンプ)。authored トリガーの専権 — LLM が提案すると adjudicate が却下する (タイマー詐称遮断、GrantSkill/SetAttribute と同型)。trigger effects は apply_ops 直行なので刻める。Gate::TurnsSince と対で「〇〇から N ターン後に発火」を組む。 値は GameState.turn の生値 (stat 境界で clamp しない)。entity 省略時は主人公。 |
| `remove_item` | `item` | GM の提案も effects も可 | player がアイテムを手放す。 |
| `request_roll` | `dc`, `sides` | GM の提案も effects も可 | ダイスを振る要求。結果は含めない — エンジンが振って裁く。 |
| `resolve_vote` | — | 作者のみ (effects) | 開票 (spec 06 Phase C)。authored トリガーの専権 (効果 op 第5例) — LLM が提案 すると adjudicate が却下する (開票結果の捏造遮断)。エンジンが一箇所で原子適用: 集計 → 最多得票 (同数は seed 派生 VOTE_RNG で抽選 = 決定論) → 死亡 (生存=0 + presence false) → 役職カウンタ/優位 stat 再計算 → 票リセット。 |
| `roll_stat` | `bonus`, `count`, `entity`, `key`, `negate`, `sides` | 作者のみ (effects) | 可変量ダイス (spec 16)。エンジンが count × d(sides) + bonus を振り、negate に 応じて ± を stat へ clamp 適用する (SAN 1d6 減少・1d8 ダメージ)。authored 専権 — LLM が提案すると adjudicate が却下する (ダメージ量の捏造遮断、GrantSkill と同型)。 trigger/challenge の effects は apply_ops 直行なので使える。出目は crate::StatRollOutcome として surface (「SAN -4 (1d6=4)」)。entity 省略時は主人公。 |
| `scale_stat` | `den`, `entity`, `key`, `num` | GM の提案も effects も可 | stat への乗除 (×/÷)。エンジンが clamp(current * num / den) を計算する。 den == 0 (ゼロ除算) はエンジンが却下するので、LLM は /0 で壊せない。entity 省略時は主人公。 |
| `set_attribute` | `entity`, `key`, `value` | 作者のみ (effects) | 文字列属性の書き換え (クラス転職 等)。authored トリガーの専権 — LLM が提案すると adjudicate が却下する (クラス捏造 = メアリー・スー遮断、GrantSkill と同型)。trigger effects は apply_ops 直行なので書き換えられる。未宣言キーは load 時 validate で弾く。entity 省略時は主人公。 |
| `set_flag` | `key`, `value` | GM の提案も effects も可 | フラグを立てる/下ろす。flag_rules の gate 未達、および authored 専権フラグ (トリガー/ challenge の帰結が書くフラグ) への LLM 提案は真偽どちらの向きも却下 (#50)。 |
| `set_presence` | `entity`, `present`, `volatile` | 作者のみ (effects) | 画面上の登場/退場 (presence のオーバーライド)。authored トリガーの専権 — LLM が提案すると adjudicate が却下する (キャラ勝手登場の捏造遮断、GrantSkill/SetAttribute と同型)。trigger effects は apply_ops 直行なので登場/退場させられる。present=true で強制登場・false で強制退場。 entity 省略時は主人公。  volatile (既定 false) が様式を選ぶ (PresenceOverride, 2026-07-25): - false = 同行者。モジュール全域に効き transition で持ち越す (従来どおり)。 - true = 来訪者。適用時の現在地に紐づき、そこを離れた瞬間に破棄される。 「使者が駆け込んでくる」のようなその場限りの登場を、退場トリガーを書かずに済ませる。 |

#### 登場のさせ方 — 同行者と来訪者 (`set_presence` の `volatile`)

`set_presence` には二つの様式があります。**どちらを使うかで「いつ消えるか」が変わります。**

```yaml
# 同行者 — 仲間になった。以後どこへ行っても一緒。章を跨いでも同行する
- { op: set_presence, entity: レイ, present: true }

# 来訪者 — この場に現れただけ。プレイヤーがその場所を離れた瞬間に自動で消える
- { op: set_presence, entity: 早馬の使者, present: true, volatile: true }
```

`volatile: true` を書かなければ従来どおり**同行者**です。「その場面だけ登場させたい」相手に
同行者を使うと、**退場させるトリガーを別に書かない限りずっと付いてきます**。

来訪者が消えたあとの顔ぶれは、その場所の `present` に書いた面々へ戻ります。**戻ってきても
来訪者は復活しません** (「その訪問限り」の登場です)。

`present: false` の側も揮発にできます。「この訪問の間だけ店主が席を外している」— 出直せば
また居ます。

- **challenge の帰結 (`on_success` 等) の effects には `attempt_challenge` を書けません**
  (判定 A の帰結で判定 A を呼ぶ無限再帰の芽。判定の連鎖はフラグ→トリガー経由で。ロード時エラー)。

#### トリガーから判定を起こす

**トリガーの effects には `attempt_challenge` / `attempt_contest` を書けます。** 筋書きの側から
ダイスを振らせる経路で、「罠が発動して回避判定」「時間経過で襲撃判定」のように、GM の提案を
待たずに判定を起こせます。

```yaml
triggers:
  - id: 落とし穴
    when: { kind: all, of: [{ kind: location_is, at: 回廊 }, { kind: flag_is, key: 床板を踏んだ, value: true }] }
    narration: 足元の床板が、鈍い音を立てて沈んだ。
    effects:
      - { op: attempt_challenge, entity: player, challenge: 回避 }
```

判定の中身 (ダイス・成功度・帰結スロット・結末文・効果音) は通常どおり全部効きます。

- ⚠ **`requires` は評価されません。** トリガーの効果は作者が書いたものとして信頼され、検証を
  通らないためです。GM が同じ判定を選ぼうとすれば `requires` で止まりますが、トリガーからは
  通ります。前提で縛りたいなら、トリガーの `when` の側に条件を書いてください。
- ⚠ **id を書き間違えると何も起きません。** トリガーは発火して narration だけ出て、判定が
  起きません。`attempt_contest` の場合はもっと厄介で、存在しない対決が「進行中」のまま居座り、
  以後の対決が全部拒否されます。**ロード時に警告が出ます**ので、開幕の ⚠ を必ず読んでください。

#### トリガーから場所を動かす

**トリガーの effects には `move` を書けます。** プレイヤーの意思と関係なく場所が変わる出来事 —
落とし穴、転移、気絶して運ばれる、場面転換 — を筋書きの側から起こせます。

```yaml
triggers:
  - id: 落とし穴
    when: { kind: flag_is, key: 床板を踏んだ, value: true }
    narration: 床が抜けた。落ちていく。
    effects:
      - { op: move, to: 地下水路 }
```

**この move は `exits` も `gate` も見ません。** 出口を繋いでいない場所へも運べます (だから
「落ちる」が書けます)。GM が同じ移動を提案した場合は従来どおり出口と gate で裁かれるので、
**行かせたくない場所へ出口を作る必要はありません**。

- 移動すると、その場所で登場させた**来訪者** (`volatile: true`) は消えます。誰が動かしたかに
  関わらず同じです。
- ⚠ **行き先を書き間違えると詰みます。** 主人公は `locations` に無い場所に立ち、出口も説明文も
  無いまま先へ進めなくなります。**ロード時に警告が出ます**ので、開幕の ⚠ を必ず読んでください。

## 状態の可視性 (誰に見せるか)

フラグ・数値・属性は、**誰に見せるか**で 3 段階に分けられます。用途で正しく選んでください。

| 宣言 | GM (語り手) | プレイヤー (UI) | 使いどころ |
|---|---|---|---|
| (通常) | 見える | 見える | 公開された状態 |
| `hidden_flags` / `hidden_stats` / `hidden_attributes` | **見える** (〔秘匿〕付き・明かさない) | 見えない | プレイヤーに伏せる**秘密**。裏の真相・隠し進行・隠し好感度・自覚のない正体 |
| `internal_flags` / `internal_stats` | 見えない | 見えない | **engine の内部変数**。タイマーの起点・repeatable カウンタ・armed フラグ |

- **`hidden_*`** は「プレイヤーには見せたくないが、GM には追わせたい秘密」です。GM は値を〔秘匿〕付きで知り、**その値・真偽を地の文で明かさず、引き起こす現象だけを描きます** (原因は伏せる)。裏で進む好感度や、まだ明かされない真相に使います。
- **`internal_*`** は「GM にも見せる必要がない engine の帳簿」です。タイマー (`record_turn` + `turns_since`) の刻みや、repeatable トリガーのカウンタなど、**語りに一切関係しない変数**をここに入れ、GM のプロンプトを汚しません。
- どちらも `gate` / トリガーの評価には**通常どおり効きます** (隠すのは表示だけ)。キーの宣言は通常フラグ/stat と同じ (`hidden_flags`/`internal_flags` は `allowed_flags` 必須、`hidden_stats`/`internal_stats` は宣言不要)。
- 迷ったら: **プレイヤーに隠したいが物語に関わる → `hidden_`** / **タイマー・カウンタで語りに無関係 → `internal_`**。

## 既成事実 (`facts_policy`) — プレイヤーが設定を宣言する欄

プレイヤーが「これは事実だ」と 1 行ずつ書き、**GM が毎ターン読んで以後の語りで守る**リストです
(最大 20 件 × 60 字)。呼称・関係・来歴・目標のような、忘れられたくない設定を固定します。

`package.yaml` の `facts_policy` で、**プレイヤーが書けるかどうかを作者が決めます**。

| 値 | 挙動 | 向いている盤面 |
|---|---|---|
| `locked` (**既定**) | 欄そのものを出さない (タブごと非表示) | ストーリーもの全般。**書かなければこれ** |
| `open` | プレイヤーが追加・編集・削除できる | キャラクターとの自由な RP が主目的の盤面 |

**`open` にする前に理解しておくこと。** ここに書かれた行は GM への指示として毎ターン渡され、
GM はそれに従います。つまりプレイヤーは**語りの中の事実を一方的に足せます**。

- **正本は壊れません。** 数値・所持品・フラグ・現在地は engine が握っており、既成事実を根拠に
  「鍵を持っている」ことにはできません (state と矛盾したら state が勝つ規律を GM に課しています)。
- **しかし物語の順序は壊せます。** プレイヤーが「私は犯人を知っている」「あの扉の先を見たことがある」
  と書けば GM はそれに沿って語り、**作者が設計した発見の段取り (謎・段階開示・伏線) が飛ばされます。**
- したがって **`open` は「薄いシナリオ + キャラとの掛け合いが主役」の盤面のためのもの**です。
  重厚なストーリーもの・推理もの・段階開示のある盤面では `locked` のままにしてください。

効き方の実測 (claude-opus-4-8): 「〇〇したら怒る」と宣言すると、実際にその行動の後に GM が怒り、
以後も長く覚えていました。単なる呼称の固定にとどまらず、**条件つきの規則**まで守られます。
その強さの裏返しとして、和解しても怒り続けることがあります (忘れさせたい行はプレイヤーが削除します)。

## 読み上げ — 音声で聴かせる盤面を書くなら

Lorekeel は GM の語りを**音声で読み上げ**られます。読み上げるかどうかは**プレイヤー側の設定**
(設定 → サウンド →「読み上げ機能を使う」、既定 OFF) で決まり、**パッケージ側に宣言はありません**。
どのパッケージでも、有効化したプレイヤーには会話ペインに読み上げ操作 (ON/OFF・スキップ) が出ます。
挿絵と同じ扱いです — 語りに触れないプレイヤー側の鑑賞物で、押すのもプレイヤーです。

> かつて `package.yaml` に `use_tts: true` と書く作者宣言がありましたが、**廃止されました**。
> 書いてあっても読み込みは通りますが (未知キーとして警告が出るだけ)、何にも効きません。
> 既存のパッケージから消しておくと警告が出なくなります。

**文体は読み上げ設定では決まりません。** 音声前提の盤面にしたいなら、`world` に文体の指示を書いてください
(例:「会話中心で語る。地の文は短く」)。読み上げの ON/OFF で語りの書かれ方が変わってはいけません —
変わると、あらすじや経緯に残る記録まで再生設定によって食い違います。**文体は作者が決め、
読み上げはその上の再生手段**、と分けてください。

**受領者の環境に依存します。** 既定はブラウザ内蔵の音声で、導入は不要ですが音質は控えめです。
プレイヤーが VOICEVOX や AivisSpeech を別途起動していれば、設定でそちらへ切り替えて聴けます。
つまり読み上げは**任意の上乗せ**であって、音声がないと成立しない設計にはしないでください。

## 挿絵の画風 (`image_style`) — プレイヤーが生成する一枚絵への指針

Lorekeel には、プレイヤーが**その場面の挿絵を生成する**ボタンがあります (設定で画像生成の
プロバイダを入れた人だけに出ます)。生成のプロンプトは GM の LLM が「直前の語り・現在地の説明・
その場にいる人物の profile・world」から書きます。`image_style` は、そこに足される**作者の画風指針**です。

```yaml
image_style: 水彩画・淡い色・キャラはデフォルメ・背景は簡略に
```

- **任意・既定は空**です。書かなければプレイヤーの設定 (スタイル接頭辞) だけで生成されます。
- **500 字まで**。超えると検分で警告が出て、先頭 500 字だけが使われます。複数行でも構いません。
- **語りにも正本にも影響しません。** 背景画像やイベント CG (`images/`) はそのままで、挿絵はそれらの上・
  文字の下に重なる別の層に出ます。読み上げと同じく、作者側に ON/OFF の宣言はありません —
  挿絵はプレイヤーが自分の判断で押す鑑賞物だからです。
- **秘密は渡りません。** プロンプトの素材はプレイヤーが既に見ているものだけです (`hidden_*` /
  `secret_*` の属性や GM だけが知る情報は入りません)。逆に言えば、`profile` に書いた容姿・服装は
  挿絵に反映されるので、見た目の決め手は `profile` に書いてください。
- ローカルの弱いモデルが GM のときは、ここに「タグ形式で 50 語以内」のように**書き方の指示**を
  足すとプロンプトの質が安定します。

## characters/*.yaml (CharacterDef)

ファイル名が EntityId になります (`characters/moka.yaml` → `moka`)。シナリオの `cast` に挙げたキャラだけが注入されます。

```yaml
name: モカ
profile: |                          # 設定・背景・性格・性向 (語りの素材。可変状態は書かない)
  人見知りだが心を開いた相手にはよく喋る。甘いものに目がない。
stats:
  好感度: { initial: 0, min: 0, max: 100 }    # min 省略時 0 / max 省略時 上限なし
  hp: { initial: 8 }
skills: []                          # 初期能力の閉世界宣言
inventory: [文庫本]                  # 初期所持品
attributes: { 役割: 同級生 }          # 初期文字列属性
icon: moka.webp                     # 任意。images/ 配下の顔アイコン (WebP 推奨)
taboos:                             # 硬い禁忌。これが真になる変化をエンジンが却下する
  - { kind: flag_is, key: 豚肉を食べた, value: true }
```

## memoria/*.yaml (伏線・lore の断片)

ファイル名が recall の主キー (id) になります。trigger の `recall: <id>` や語りの文脈で想起されます。

```yaml
tags: [幼馴染の約束, 桜の木]          # 別名キー
text: |                             # 語りに注入される伏線本文 (可変状態は書かない)
  十年前、桜の木の下で「大人になったらまた会おう」と約束した。
```

## campaign.yaml (任意。複数シナリオを繋ぐ)

```yaml
title: 逃亡行
start: study                        # 開始モジュール id
modules:
  study: scenarios/study.yaml       # id → シナリオファイル (パッケージ相対)
  cellar: scenarios/cellar.yaml
  forest: scenarios/forest.yaml
edges:                              # (現モジュール, 到達ゴール) → 次モジュール
  - { from: study, on_goal: jammed_ending, to: cellar }
  - { from: study, on_goal: opened_ending, to: forest }
```

- 数値 (entities)・所持品・能力・`global_flags` 宣言分のフラグは遷移を生き残ります。局所フラグは捨てられます。
- `package.yaml` の `entry: campaign.yaml` で開始します。

## 人狼型の秘匿役職盤面 (上級・任意)

```yaml
role_assignment:                    # 役職のランダム割り当て (エンジンが決定論 shuffle)
  key: 役職
  pool: { 人狼: 2, 占い師: 1, 村人: 3 }
  among: [player, mira, gen, sayo, tokio, yuren]   # 人数は pool 合計と一致必須
secret_attributes: [役職]            # 宛先別秘匿 (GM は全員分見えるが演じ分ける。本人は自分の分を見える)
hidden_attributes: [真の正体]         # 本人にも見えない属性 (自覚のない正体・呪い等)。UI は本人分ごと隠し、GM は「当人にも明かさない」規律で扱う。キーの宣言必須は secret と同じ
vote_rules:                         # 投票権の宣言 (書かなければ誰も投票できない)
  - { when: { kind: flag_is, key: 投票中, value: true } }
```

生存管理 (`生存` stat・`生存{役職}数`・`{役職}優位`) は role_assignment が自動生成し、開票 (`resolve_vote`) だけが更新します。フェーズ進行は repeatable トリガー + タイマー (record_turn / turns_since) で書きます。

## 作法 (品質の高いパッケージのために)

1. **山場を 1 回の判定で潰させない**: 決定的な決着は必ず flag / goal に接地し、大敵の challenge には `requires` で前段ビートを積む (弱体化 → 儀式 → 討伐、と数珠つなぎ)。
   なお GM は「拾ってから使う」のような段取りを 1 ターンに束ねられる (op は書いた順に処理される) が、**ダイス判定 (challenge) の帰結に依存する手は次のターンに割れる** — 駆け抜けてほしくない箇所には challenge を置くのがペース制御の基本。
   例外が 1 つ: challenge の効果のうち **on_success と on_failure の両方に共通するもの** (= どの出目でも必ず起きるもの) は判定と同じターンの後続手の前提にできる。日次ループのフラグ (「今日は働いた」等) を両帰結に書けば「働いて帰る」が 1 ターンで畳め、成功側にしか無いフラグを前提にした手は従来どおりターンが割れる — **割りたい連鎖は帰結を成功/失敗で変えて書く**。
2. **`location_is` の `at` には `locations:` に宣言した場所名しか書けない**。所持品や状態を場所のように書くとその条件は**永久に成立しません** (挑戦なら一度も選べず、出口なら通れないまま)。
   ❌ `{ kind: location_is, at: inventory }` — 所持品は場所ではない → ✓ `{ kind: has_item, item: 花冠 }`
   宣言に無い場所を指していたら開幕に警告が出ます。
3. **会話で立つ知識フラグ**は `flag_hints` (促し) + `flag_rules` (守り) のペアで。**行動・場所に紐づく学び**は trigger (`when: location_is` → `set_flag`) で。
   **1 つのフラグの書き手は 1 人に決める**: トリガー/challenge の効果が書くフラグは「筋書きの専権」となり、GM は set_flag できない (true にも false にも倒せずエンジンが却下する)。専権フラグに `flag_hints` を付けても GM には届かない (開幕に警告が出る)。GM に立てさせたいフラグは、トリガー/challenge の効果に書かないこと。
4. **NPC を出す場所には必ず `present` を書く** (未記載 = 無人)。
5. **トリガーの `narration` に台詞・所作で登場させるキャラは、その場に居ることを保証する**。
   `when` に場所条件 (`location_is`) が無いトリガーは**どの場所でも発火し得ます**。フラグ条件だけで書くと、
   そのキャラの居ない場所で「彼女が首をかしげる」のような narration が確定イベントとして提示され、
   GM は以後そのキャラが居るものとして語り続けます (presence 宣言との矛盾が作者の文から生まれる)。
   守り方は 3 つのどれか: (a) `when` に `location_is` でそのキャラの居る場所を足す /
   (b) 発火し得る場所すべての `present` にそのキャラを含める / (c) `effects` の `set_presence` で正規に登場させる。
   (c) を「その場面だけ」のつもりで使うなら `volatile: true` を付けること — 付けないと以後ずっと同行します。
6. **下流 gate の前提になる進行フラグは、単一の発見経路で立てる**。
   同じ手掛かりを複数経路で得られるようにしないでください。例: ある写真を「場所アイテムの拾得」と
   「challenge 成功」の両方で発見できるようにし、フラグ (found_photo 等) を立てるのが challenge 側だけだと、
   アイテムを拾っただけのプレイヤーはフラグが立たず、そのフラグを要求する下流 (日記の取得 gate 等) で詰みます。
   「見たのに進めない」というプレイヤー体感になり、特定の遊び方でだけ再現する発見しにくいバグです。
   手掛かりの発見と、それが立てる進行フラグは 1 対 1 に保ってください。
7. **タイマー・カウンタ等の engine 内部変数**は `internal_stats` / `internal_flags` で GM からもプレイヤーからも隠す。
   **プレイヤーには伏せたいが GM には追わせたい秘密** (裏の好感度・隠し進行・自覚のない正体) は `hidden_stats` / `hidden_flags` / `hidden_attributes` に置く — GM は〔秘匿〕付きで見て、値を直接明かさず現象だけを描く (詳細は「状態の可視性」)。
8. **画像は WebP・音声は Ogg** に変換し、パッケージ全体を数十 MB 以内に抑える (「アセットのフォーマットとサイズ」)。
9. **プレイ時間 2〜3 時間までの短編を推奨**。goal 2〜4 個・場所 3〜8 個・キャラ 1〜5 人が目安。
10. 迷ったら小さく作って動かす。宣言漏れはエンジンがロード時に具体的なエラーで教えてくれます。

## よくあるロード時エラー (自己チェックリスト)

- challenge / trigger の帰結フラグが `allowed_flags` に無い → 宣言を追加
- `global_flags` / `persistent_flags` / `flag_hints` / `flag_titles` / `hidden_flags` / `internal_flags` のキーが `allowed_flags` に無い → 宣言を追加
- trigger の `set_attribute` が未宣言の属性キー → `initial_attributes` (player) / `attributes` (NPC) に宣言
- goal も goals も無い → 勝利条件を必ず 1 つ以上
- `secret_attributes` / `hidden_attributes` のキーがどこにも宣言されていない → `initial_attributes` / NPC の `attributes` / `role_assignment.key` のいずれかで宣言
- `role_assignment` の pool 合計と among 人数の不一致 / 幻キャラ / 重複
- `cast` に挙げたキャラの `characters/<id>.yaml` が無い
- `entry` の指すファイルが無い / `package.yaml` が無い
- challenge の `entity` で指定した人物が判定 `stat` を宣言していない → その人物の `stats` に宣言
- tier の `threshold` が `1〜sides` の範囲外、または `at_most`/`at_least` なのに `threshold` が無い
- `resolution: percentile` なのに `stat` が無い / `sides`・`dc` を書いた / `tiers` を併用した → percentile の形に直す (加算式は sides+dc、percentile は stat のみ)
- contest の `opponent` が居ない / 振り方テンプレートが無い / 帰結フラグ未宣言 → キャラ・rolls・allowed_flags を宣言
- `spend_rules.from` / `push_cost.from` が player の宣言済み stat でない → `initial_stats` (または package の `player.stats`) に宣言

**エラーにならない静かな罠**: この仕様に無いフィールドは**黙って無視されます**
(例: `locations` の場所直下に `gate:` を書いても効かない — 移動を縛る gate は `exits` の**各出口**に書く。
フィールド名の typo — `entity` を `entry` と書く等 — やインデントずれによる入れ子ミスも同類)。
最新の Lorekeel は新しいゲームの開幕にこれらを ⚠ で名指しします (「entity の誤り？」のような修正候補つき)。
「書いたのに効いていない」と感じたら、開幕の警告とこの仕様を突き合わせてください。
これは `characters/*.yaml` にも効きます (`stats` を `stat` と書くと、そのキャラは数値を 1 つも持たないまま静かに動きます)。

### 書けたら機械に検めさせる (推奨)

アプリを起動しなくても、**API キーなしで**パッケージ全体を静的検査できます。

```
cargo run -p harness --bin play -- lint packages/あなたのパッケージ
```

`package.yaml` / シナリオ / `characters/*.yaml` / キャンペーンの**全モジュール**を一度に見て、
`✗` (ロード拒否級) と `⚠` (書いたのに効かない類) を名指しします。エラーがあれば終了コード 1。
できあがったら**まずこれを通してください** — 上のチェックリストのほとんどは機械が答えます。

**ただし「通る」は「遊べる」ではありません。** この検査は各機構を*単独で*見るので、
**独立に正しい機構どうしの相互作用**は原理的に捕まりません。実例: 同じフラグに
`flag_hints` (GM に見せて立てさせる) と `hidden_flags` (GM の語彙から隠す) を併用すると、
GM がそのフラグを知り得ず**真エンドが到達不能**になりますが、検査は何も言いません。
検査を通したあと、**各ゴールへの到達経路を 1 本ずつ手でたどってください** —
「そのフラグを誰が立てるのか (GM か筋書きか)」「GM から見えているか」「先に別の goal が
成立してしまわないか」の 3 点が、到達不能の主な発生源です。
