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
