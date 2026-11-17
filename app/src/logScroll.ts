/**
 * 会話ログの「末尾が見えているか」(2026-11-17、ユーザー要望「最新までスクロールする FAB。
 * 一番下までいくと消える」)。写経元は Fuseforks の `lib/chatScroll.ts`。
 *
 * 読むのは**同じ要素のスクロール量だけ** (`scrollHeight` / `scrollTop` / `clientHeight`)。要素の
 * 画面上の座標は測らない — 3 つは同じ要素の同じ座標系で閉じているので、文字サイズの設定や
 * 表示倍率が変わっても差の意味が変わらない。
 *
 * 実寸を読むのは部品の側 (`ConversationLog.vue`) で、ここは読んだ数値から先だけを持つ
 * (spec 31 の規律 = 実寸を読む部品は、実寸から先を純関数へ出して unit で固定する)。
 */

/**
 * 末尾からこの距離 (CSS px) 未満なら「末尾を見ている」とする。
 *
 * Fuseforks は 80 だが、あちらは新しい行への追従と同じ判定を共有するための幅。Lorekeel の会話ログは
 * 追従しない (語りはターンごとに丸ごと届くので、末尾へ送ると書き出しが画面の外へ出る) ので、
 * 吸収したいのは 2 つだけ — 表示倍率で `scrollTop` が端数になり厳密な 0 に届かないことと、
 * ログの下の余白 (`py-5` = 20px)。広く取ると、まだ読んでいない行が下に残っているのにボタンが消える。
 */
export const NEAR_BOTTOM_PX = 24;

/** スクロールの量だけを持つ形。テストで要素を作らずに渡せるよう、要素の型に縛らない。 */
export interface ScrollMetrics {
  scrollHeight: number;
  scrollTop: number;
  clientHeight: number;
}

export function isNearBottom(m: ScrollMetrics): boolean {
  return m.scrollHeight - m.scrollTop - m.clientHeight < NEAR_BOTTOM_PX;
}
