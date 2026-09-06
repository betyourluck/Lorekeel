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
