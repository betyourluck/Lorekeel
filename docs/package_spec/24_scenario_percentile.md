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
