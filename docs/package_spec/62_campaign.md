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
