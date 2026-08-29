/**
 * spec 28 v2 追補: エディタのフッタと上書きモードの**純粋な芯**。
 *
 * どちらも CodeMirror に依らない (DOM も要らない) ので、提示層のうちここだけはテストできる。
 * 特に上書きは**黙って文字を壊しうる**経路 — サロゲートペアや結合文字を半分だけ消すと、
 * エラーも警告も出ないまま本文が化ける。
 */

/**
 * 上書きで消す幅（文字数）。`0` なら消さない = 挿入と同じ振る舞いにする。
 *
 * - **行末では 0**（次の行を食わない = 一般的なエディタの流儀）。
 * - 消す単位は**書記素クラスタ**。呼び出し側は `findClusterBreak` の結果を渡す想定で、
 *   ここはその結果が前に戻る/動かない異常値を弾く番人を兼ねる。
 */
export function overwriteSpan(lineLength: number, offset: number, clusterEnd: number): number {
  if (offset >= lineLength) return 0; // 行末
  if (clusterEnd <= offset) return 0; // 進まないなら触らない
  return Math.min(clusterEnd, lineLength) - offset;
}

/**
 * フッタに出す文字数。**コードポイント数**で数える。
 *
 * JS の `.length` は UTF-16 の符号単位なので、絵文字が 2 と数えられる。
 * 「何文字書いたか」の表示としては嘘になるので、こちらを使う。
 */
export function countChars(text: string): number {
  let n = 0;
  for (const _ of text) n++;
  return n;
}
