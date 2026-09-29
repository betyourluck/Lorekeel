# spec 34 — API キーを OS の資格情報ストアへ

**Status**: 実装済み (2026-09-30 起草・同日実装)。残 = 実機での移行確認 (ユーザー) / macOS・Linux 実機は未確認。

---

## 動機

2 つの独立した起点が同じ穴を指した。

1. **winget-pkgs のレビュー** ([PR #427244](https://github.com/microsoft/winget-pkgs/pull/427244#issuecomment-5849381697)、
   2026-09-26 denelon): Lorekeel は LLM の API キーを受け取り、ゲームの素材をクラウドへ送るので、
   `PrivacyUrl` に「鍵をどこにどう保存し、どう守っているか」を書いた文書を求められた。
2. **ユーザーの以前からの懸念** (2026-09-30):「Fuseforks のように `.env` でなく OS の資格情報に入れたほうが安全」。

文書に正直に書くと答えは「**平文で 3 箇所**・守りは OS のユーザー権限だけ」になる。
先に動作を正し、文書はその後に書く (ユーザー決定 2026-09-30 = 「移行を先に」)。

### 実測 — 鍵は 3 箇所に平文で居る (2026-09-30 コード実読)

| 置き場 | 入っている鍵 |
|---|---|
| `app_data/.env` | `LLM_API_KEY` / `SUMMARY_LLM_API_KEY` / `EDITOR_LLM_API_KEY` / `IMAGE_API_KEY_OPENAI` / `IMAGE_API_KEY_GEMINI` / `JEV_API_TOKEN` |
| WebView の localStorage `kataribe.aiModelProfiles[].apiKey` | 登録モデルごとの鍵 |
| `app_data/settings.json` | 上の localStorage の写し (2026-08-31 の設定ミラー) |

設定ミラーの契約は「file も `.env` と同じ信頼水準」と書いていた。**言い換えると平文の置き場を 1 つ増やしていた**。

---

## 決定

### 1. 範囲 = 保存時 (at rest) の平文をやめる。実行時のメモリは従来どおり

Fuseforks は値を UI に一切返さない (`contains()` で「登録済み」だけを見せる)。Lorekeel で同じ形を取ると、
登録モデルの突き合わせ (`profileMatchesConfig` が鍵を比較する)・切り替え・あらすじ用と編集用の
プロファイル選択を全部作り替えることになり、**ここ数週間で不具合が集中した提示層**を大きく触る。

脅威の本体は「ディスクに平文で残る」ことなので、**保存先だけを差し替え、実行時の流れは変えない**。
鍵は起動時に資格情報ストアから読んでプロセス env と WebView のメモリへ載る (これまでと同じ)。
この線引きは文書に明記する (「実行中はアプリのメモリに載る」)。

### 2. 保存先 = OS の資格情報ストア

`keyring` 3 (Fuseforks と同じ features: `apple-native` / `windows-native` / `sync-secret-service`)。

- Windows: 資格情報マネージャー / macOS: キーチェーン / Linux: freedesktop Secret Service
- サービス名 = `jp.lorekeel.app` (bundle identifier と同じ)
- エントリ名 = env 名そのもの (`LLM_API_KEY` など 6 つ) / 登録モデルは `profile:<id>`
- `profile:` の id は `[A-Za-z0-9_-]{1,64}` だけ受ける (`newProfileId` の UUID とフォールバック形の両方が入る)

### 3. backend — 書き込みの出口を 1 つにする

`.env` を書く production の 7 箇所をすべて `persist_env` に通す。鍵かどうかの判定は
`SECRET_ENV_KEYS` (6 つ) の一箇所。**後から足した書き込みが鍵を `.env` に落とす経路を構造で塞ぐ**
(鍵を書かない呼び出しも通すのはそのため)。

- 鍵・値あり → 資格情報ストアへ `set` → `.env` からその行を**消す**
- 鍵・値が空 → ストアから `delete` → `.env` に `KEY=` の空行を**書く**
  (dev では main.rs が repo `.env` を先に読むので、空行が無いと「消したのに repo の鍵が生き返る」。
  failures #46 の「GUI の保存値がユーザーの最後の明示意思」を保つ。空行は秘密ではない)
- 鍵以外 → 従来どおり `upsert_env`
- ストアが失敗した鍵 → **`.env` へ平文で書く (退避)** + 警告。保存そのものは落とさない
  (Linux で Secret Service が無い環境を想定。設定できないアプリにするより、弱い保存でも動くほうを採る)

プロセス env への `set_var` は各 command がこれまでどおり行う (実行時の流れは不変)。

### 4. 起動時の順序

```
main.rs: repo .env (dev のみ実質)
setup:   app_data/.env を override で読む → 改名の移行 → **鍵の移行** → **ストアの鍵を env へ**
```

- **鍵の移行** (`migrate_env_secrets`): `app_data/.env` に値のある鍵行があれば、ストアへ書き、
  **読み戻して一致したものだけ** `.env` から消す。失敗した行は残す (次回起動で再試行)。
  2026-08-28 の改名で「コピーであって移動ではない」が退路になった前例と同じく、**消すのは確認の後**。
- **ストアの鍵を env へ** (`load_secrets_into_env`): ストアにある鍵だけ `set_var`。無い鍵は触らない
  (dev の repo `.env` の値がそのまま生きる = 従来の挙動)。

### 5. 登録モデル — localStorage に鍵を置かない

command 3 つ: `get_profile_secrets(ids) -> {id: key}` / `set_profile_secrets([{id, key}])`
(空は delete・書いた後に読み戻して検める) / `delete_profile_secrets(ids)`。

frontend:
- `saveAiProfiles` は **`apiKey` を空にしてから** localStorage へ書く。鍵は呼び出し側が先に
  `set_profile_secrets` で渡す (`persistProfiles`)。ストアが失敗したら**鍵ごと** localStorage に書く (退避・警告)。
- 設定ダイアログは開いたときに `get_profile_secrets` で鍵をメモリへ取り寄せる (`hydrate`)。
  以後の突き合わせ・切り替えは従来の関数のまま動く。
- **旧形式の移行は起動時** (App.vue の onMounted): localStorage の登録モデルに鍵が残っていれば
  ストアへ書き、成功したら鍵を抜いて書き直す。設定ダイアログを開くまで待たない
  (開かないユーザーの `settings.json` に平文が残り続けるため)。書き直しは設定ミラーの write-through を
  起こすので `settings.json` も鍵の無い内容で上書きされる。

### 6. 状態の表示と一括削除

- `secret_store_status()` → `{ fallback_keys: [env 名] }` = `.env` に値のある鍵行 (= 退避中)。
  設定「AIモデル」タブに「鍵は OS の資格情報ストアに保存しています」/「資格情報ストアが使えないため
  .env に保存しています: …」を出す。**真実は file から読む** (状態を別に持つと file とずれる)。
- 「保存した API キーをすべて削除」ボタン: `delete_all_secrets(profile_ids)` = 6 つの env 鍵 + 登録モデルの鍵を
  ストアから消し、プロセス env を空にし、`.env` に空行を書く。確認ダイアログを挟む (不可逆)。
  プライバシー文書の「鍵を消す方法」がこのボタンと OS の資格情報マネージャーの 2 つになる。

---

## スコープ外 / 既知の限界

- **実行中のメモリ**: 鍵はプロセス env と WebView のメモリに載る (決定 1)。
- **WebView の LevelDB の残骸**: localStorage から鍵を消しても、LevelDB のログファイルに旧値が
  コンパクションまで残りうる。消去を保証する API は無い。
- **旧フォルダ `jp.kataribe.app`**: 改名時の移行は「コピー」なので、Kataribe 時代から使っている人の
  旧フォルダには平文の `.env` と旧 localStorage が残っている。**自動では消さない** (改名の退路の方針)。
  文書に場所と消し方を書く。**2026-09-30 ユーザー決定 = 消さなくてよい** (旧フォルダの `.env` にも触らない)。
- **CLI `play`**: 開発者向けで repo の `.env` を読む。配布物に含まれないので対象外。
- **アンインストール**: 資格情報ストアのエントリはアンインストールで消えない (`app_data` と同じ)。文書に書く。

---

## PoC

app backend (InMemory のストアで全経路):
1. `persist_env` の振り分け: 鍵はストアへ行き `.env` から消える / 空の鍵はストアから消え `KEY=` が書かれる /
   鍵以外は `.env` / ストアが失敗した鍵は `.env` へ退避し警告を返す
2. 起動時の移行: 値のある鍵行はストアへ移り `.env` から消える / 読み戻しが一致しない (失敗するストア) なら行を残す /
   ストアの鍵が env に載る
3. 登録モデルの鍵: 往復・空は削除・不正な id は拒否
4. `secret_store_status` が `.env` の退避行を数える

frontend (vitest): `saveAiProfiles` が鍵を書かない / 旧形式の判定 (鍵の残った登録を拾う)。

live (ignored): 実 keyring で set → get → delete (Windows 実機)。
