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
