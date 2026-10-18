# spec 35 Phase E — E-2 本走の道具

台詞と地の文の分離 (spec 35) を複数のモデルで台本プレイし、計器 (`play --observe`) の記録を集める。
指標の定義・判断基準・E-1 の結果は [specs/35_dialogue_segments.md](../../specs/35_dialogue_segments.md) の
「Phase E の実測計画」節が正。

## 中身

| ファイル | 役割 |
|---|---|
| `run.ps1` | 1 モデルで台本 3 本を回す。モデルの設定は Lorekeel の登録モデルと同じ値 |
| `report.py` | 出力フォルダを集計し、手で分類するための読み物 (`<package>.read.txt`) を書く |
| `scripts/*.txt` | 台本 (1 行 1 行動。行頭の `[P1]`〜`[P4]` は誘いの行動の印) |
| `out/` | 実行結果 (語りの本文を含むので **git には入れない**。`.gitignore` 済み) |

台本とパッケージ:

| 台本 | パッケージ | 誘いの行動 |
|---|---|---|
| `friday_lemmon` | repo 同梱 `packages/friday_lemmon` | P1 電話 / P3 八百屋 / P4 店に入る / P2 主人公の台詞 |
| `ui_and_ruka` | `%APPDATA%\jp.lorekeel.app\packages\ui_and_ruka` | P3 コンビニの店員 / P1 電話 / P4 帰宅 / P2 主人公の台詞 |
| `station_of_sand_girl_en` | `D:\語り部\station_of_sand_girl_en` (英語) | P1 待合室から叫ぶ / P3 駅員 / P4 ホームへ戻る / P2 主人公の台詞 |

計画では 2 本目を yuminaristars にしていたが、**CLI の `--package` はキャンペーン形式 (入口が `campaign.yaml`) を
読めない** (GUI は読める) ので、マスターがよく遊んでいる単発の ui_and_ruka に替えた。
この欠けは 2026-10-16 に直した (`689afbe`) ので、CLI の `play --package` は yuminaristars を読める。
ただし `run.ps1` の `-Package` は 3 本の固定の組なので、回すには台本と `$packages` への追加が要る。

## 回し方

PowerShell で repo の直下から。1 モデル 30 ターン (3 本 × 10 行)。前回の試走では 10 ターンに 4〜5 分かかった。

```powershell
./measure/spec35_phase_e/run.ps1 -Preset dryrun -MaxLines 1   # 課金なしの結線確認 (接続先に繋がらないことを確かめる)
./measure/spec35_phase_e/run.ps1 -Preset sakura               # さくら AI Engine Qwen3.6 (弱いモデル枠)
./measure/spec35_phase_e/run.ps1 -Preset muse                 # Meta muse-spark-1.3
./measure/spec35_phase_e/run.ps1 -Preset gpt6                 # OpenAI gpt-6-luna
./measure/spec35_phase_e/run.ps1 -Preset grok                 # xAI grok-4.3 (repo の .env の鍵を使う。E-1 で 2 周済み)
./measure/spec35_phase_e/run.ps1 -Preset gemini               # Google gemini-4-argon
./measure/spec35_phase_e/run.ps1 -Preset opus                 # Anthropic claude-opus-5-5 (最も高いので最後)
```

- **API キーは実行のたびに伏せ字で入力**する (CLI は OS の資格情報ストアを読まない)。このプロセスの環境変数にだけ渡し、
  終わったら元に戻す。ファイルには書かない。grok だけは repo の `.env` の鍵を使う。
- 1 本だけ回すときは `-Package friday_lemmon` のように指定する。
- 思考の深さと出力上限は登録モデルと同じ (muse = medium / 16384、gpt6 = medium / 160000、opus と gemini = 未設定 / 16000、
  sakura = 未設定 / `.env` の 4096)。

回し終えたら:

```powershell
python -I measure/spec35_phase_e/report.py (Get-ChildItem measure/spec35_phase_e/out -Directory).FullName
```

## 費用の目安 (上限)

E-1 の grok-4.3 では 1 ターンあたり入力約 1 万トークン・出力数百トークンだった。1 モデル 30 ターンで入力約 33 万トークン。
キャッシュ読みを数えない上限で、opus ≈ $2〜3、gemini ≈ $1、muse と gpt6 ≈ $0.1 未満。思考するモデルは出力が増える。
`report.py` が登録モデルの単価で上限を出す (キャッシュ読みは jsonl に無いので割り引かない)。

## 注意

- **ui_and_ruka は成人向けの設定を含む**。台本の行動は日常の場面だけにしてあるが、system に載る設定だけで
  生成を断るプロバイダーがありうる。断られたターンは jsonl に `malformed` / `error` として残るので、そのまま記録する。
- 出力 (`out/`) には語りの本文が入る。共有するときは本文を含むことに注意する。
- 途中で止めたいときは Ctrl+C。そのパッケージの jsonl は書けた行までが残る。

## 手で分類する (E-3)

`report.py` を回すと各出力フォルダに `<package>.read.txt` ができる。ターンごとに語りの本文・提案の形・
やり直しの理由・⑦ の漏れ・「却下後に消えた」が並ぶので、末尾の `分類:` 行に③⑤⑥と副を書き込む。
定義は specs/35 の指標の表 (③は「」の有無に関わらず、地の文に入った台詞を全部数える)。
