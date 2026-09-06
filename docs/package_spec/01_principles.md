## 大原則 (これを破るとエンジンがロード時に拒否します)

1. **閉世界**: フラグ・stat・スキル・アイテム・属性・チャレンジは**宣言したものだけが存在**します。宣言していない名前を triggers や gate から参照してはいけません。
2. **正本はエンジン**: 数値・フラグ・位置・所持品の真実はエンジンが握ります。YAML は「宣言と拘束」を書く場所です。
3. **自己完結**: パッケージフォルダの外を参照してはいけません。ファイル参照はすべてフォルダ相対です。
4. **エンジンが検証しない欄** (profile / world / narration / description / hint / epilogue_prompt 等) は語りの素材です。ここに可変の世界状態 (HP や進行フラグ) を書いてはいけません。

## パッケージの構造

```
<パッケージ名>/
  package.yaml        # 必須。世界をまとめる 1 ファイル
  scenarios/          # 必須。シナリオ本体 (entry が指すファイルを含む)
    main.yaml
  characters/         # 任意。1 キャラ 1 ファイル (ファイル名 = EntityId)
    alice.yaml
  memoria/            # 任意。伏線・lore (1 断片 1 ファイル)
    promise.yaml
  campaign.yaml       # 任意。複数シナリオを繋ぐ場合のみ
  images/             # 任意。背景・イベント CG (ID = ファイル名)
  audios/             # 任意。BGM・SE (ID = ファイル名)
```

- フォルダごと zip して配布します (`<パッケージ名>/package.yaml` の形)。
- アセット ID は `^[A-Za-z0-9._-]{1,64}$` のファイル名のみ (日本語ファイル名不可)。
- 実行ファイル・スクリプト (exe / dll / bat / sh / js / wasm 等) は同梱禁止。

### アセットのフォーマットとサイズ (配布容易性のために)

- **画像は WebP を推奨**します。PNG から品質 80 で変換すると 1/10〜1/30 になります (顔アイコンで実測 480KB→約 13KB)。背景・イベント CG・顔アイコンすべて WebP で構いません。
- **音声は Ogg (Vorbis) を推奨**します。BGM は 1〜2MB、SE は数十 KB が目安。WAV は無圧縮で配布に向きません。
- Lorekeel は**拡張子を解釈しません** — `images/`/`audios/` に置いたファイル名をそのまま ID に書けば、WebP/Ogg も PNG/WAV と同じように扱われます。
- **パッケージ全体は数十 MB を超えないこと**。ダウンロードをためらわせるサイズは、それだけで遊ばれる機会を減らします。10MB の画像 1 枚は、ほぼ確実に減量できます。
- 変換例: `magick input.png -quality 80 output.webp` / `ffmpeg -i input.wav -c:a libvorbis -q:a 4 output.ogg`
