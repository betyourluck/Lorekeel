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
