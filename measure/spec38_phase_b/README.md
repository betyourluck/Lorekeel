# spec 38 Phase B — `play build` の実測の道具

AI でパッケージを組む (spec 38) を、CLI の `play build` で複数のモデルに回し、`--report` の JSON を集める。
測るもの・結果は [specs/38_ai_package_builder.md](../../specs/38_ai_package_builder.md) の「Phase B 実測」節が正。

## 中身

| ファイル | 役割 |
|---|---|
| `run.ps1` | 1 モデルで依頼 2 つを回す。モデルの設定は Lorekeel の登録モデルと同じ値 (spec 35 の `run.ps1` と同じ) |
| `report.py` | 出力フォルダの JSON を集計する (計画の却下・ファイルごとの周回・雛形のまま・層 1/層 2 の残り・利用量) |
| `requests/*.txt` | 依頼の本文 |
| `out/` | 実行結果 (パッケージの本文を含むので **git には入れない**。`.gitignore` 済み) |

| 依頼 | 出発点 |
|---|---|
| `r1_skeleton` | ウィザードが作る骨格 (package.yaml + scenarios/main.yaml) — 灯台の短い盤面を組む |
| `r2_lakeside` | 同梱の `packages/lakeside_manor` の写し — 庭師のキャラと温室の場所を足す |

どちらも出力フォルダの中の**写し**に対して回し、`--write` は付けない (元のパッケージには書かない)。

## 回し方

PowerShell で repo の直下から。CLI は OS の資格情報ストアを読まないので、鍵は実行のたびに伏せ字で入力する
(grok だけは repo の .env をそのまま使う)。

```powershell
./measure/spec38_phase_b/run.ps1 -Preset dryrun   # 課金なしの結線確認 (計画の呼び出しで接続に失敗する)
./measure/spec38_phase_b/run.ps1 -Preset muse
./measure/spec38_phase_b/run.ps1 -Preset gemini
python -I measure/spec38_phase_b/report.py measure/spec38_phase_b/out/<preset>-<日時>
```

差分は `<依頼>.stdout.txt`、進行ログ (道具の呼び出しと手がかり) は `<依頼>.stderr.txt` にある。
