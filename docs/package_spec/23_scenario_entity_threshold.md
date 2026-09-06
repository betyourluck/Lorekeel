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
