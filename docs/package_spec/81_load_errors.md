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
