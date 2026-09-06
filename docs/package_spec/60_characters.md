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
