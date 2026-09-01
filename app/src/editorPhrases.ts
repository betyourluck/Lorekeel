/**
 * CodeMirror の組み込み UI 文言（検索・置換パネル / 行へ移動）の和訳（2026-09-01）。
 *
 * `@codemirror/search` のパネルは自前の英語ラベルを `state.phrase("Find")` のように引く。
 * CodeMirror はこれを `EditorState.phrases` ファセットで差し替えられるようにしてあるので、
 * **キーはライブラリ側の英語文字列そのもの**になる。
 *
 * ゆえにこの表は「UI 文言」ではなく**第三者コンポーネントの引き当て表**なので、
 * `messages.ts`（アプリ自身の文言）には置かない — 置くとキーと値が 2 ファイルに割れ、
 * 追加のたび両方を触ることになる。加えてここは `localStorage` に触れないので、
 * 純粋関数としてテストできる（`i18n.ts` は起動時にロケールを localStorage から読む）。
 *
 * **英語は空表を返す** — キー自身が英語なので、上書きしないのが最も正しい
 * （英語を書き写すと、ライブラリが原文を直したときにこちらだけ古いまま残る）。
 */
export type EditorLocale = "ja" | "en";

/**
 * 和訳表。**キーは `@codemirror/search` が `phrase()` に渡す文字列と 1 字も違ってはいけない**
 * （違えば黙って英語のまま出る = 沈黙する失敗）。網羅は `editorPhrases.test.ts` が
 * ライブラリの dist から実際のキーを抽出して照合する（手書きの一覧を信じない）。
 *
 * `$` はライブラリ側の差し込み位置（件数・行番号）なので**必ず残す**。
 */
const JA: Record<string, string> = {
  // 検索・置換パネル
  Find: "検索",
  Replace: "置換",
  next: "次へ",
  previous: "前へ",
  all: "すべて選択",
  "match case": "大文字小文字を区別",
  regexp: "正規表現",
  "by word": "単語単位",
  replace: "置換",
  "replace all": "すべて置換",
  close: "閉じる",
  // 読み上げ用のアナウンス（画面には出ないがスクリーンリーダーが読む）
  "current match": "現在の一致",
  "on line": "行",
  "replaced $ matches": "$ 件を置換しました",
  "replaced match on line $": "$ 行目の一致を置換しました",
  // 行へ移動（Alt-G）
  "Go to line": "行へ移動",
  go: "移動",
};

/** そのロケールで CodeMirror に渡す差し替え表（英語は上書きしない = 空）。 */
export function searchPhrases(locale: EditorLocale): Record<string, string> {
  return locale === "ja" ? { ...JA } : {};
}
