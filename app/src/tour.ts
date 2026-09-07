/**
 * 初回起動のナビゲーション (2026-09-07、ユーザー要望) の**純粋な芯**。
 *
 * 「ゲームの始め方」を 4 歩で示す。①設定で AI モデルを登録 ②パッケージ一覧で TRPG シナリオを
 * 取得 ③パッケージを選んで開始 ④入力欄に行動を打つ。実物の UI 要素をスポットライトで照らし、
 * その横に番号つきのカードを出す (コーチマーク)。演出は CSS だけで依存を足さない。
 *
 * ここに置くのは DOM に依らない判断だけ = 提示層で唯一テストできる部分:
 * - 出すかどうか (`shouldShowTour`) — 「初回」の判定
 * - スポットライトの矩形 (`spotlightBox`) — 対象を少し広めに囲む
 * - カードの置き場 (`placeCard`) — 対象の下・上・右・左の順に、画面からはみ出さない側へ
 *
 * 描画そのもの (要素の測定・トランジション) は FirstRunTour.vue。
 */

/** 見たかどうかの永続キー。`kataribe.` 接頭辞なので設定ミラー (settings.json) の射程に入る。 */
export const TOUR_DONE_KEY = "kataribe.tourDone";

/** 4 歩の対象。`data-tour` 属性の値で、実物の要素に付いている。順序が案内の順序。 */
export const TOUR_STEPS = ["settings", "packages", "start", "input"] as const;
export type TourStep = (typeof TOUR_STEPS)[number];

/**
 * 出すかどうか。**初回だけ** = 見た印が無く、かつ既に使っている痕跡も無いとき。
 *
 * 既存ユーザー (更新で初めてこの機構が入った人) は AI モデルかパッケージを既に登録している
 * ので、印が無くても「初回」ではない。その人に案内を出すと「知っている手順を教えられる」
 * 形になるので、痕跡があれば出さずに印だけ立てる (呼び出し側の責務)。
 */
export function shouldShowTour(input: { done: boolean; profileCount: number; packageCount: number }): boolean {
  if (input.done) return false;
  return input.profileCount === 0 && input.packageCount === 0;
}

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** 対象の矩形を `pad` だけ広げる (負の座標には出さない = 画面外へ滲まない)。 */
export function spotlightBox(target: Box, pad: number): Box {
  const left = Math.max(0, target.left - pad);
  const top = Math.max(0, target.top - pad);
  return {
    left,
    top,
    width: target.width + (target.left - left) + pad,
    height: target.height + (target.top - top) + pad,
  };
}

export type CardSide = "below" | "above" | "right" | "left";

export interface CardPlacement {
  left: number;
  top: number;
  side: CardSide;
}

/**
 * カードの置き場。対象の**下 → 上 → 右 → 左**の順に、カードが画面に収まる最初の側を選ぶ。
 * どの側にも収まらなければ下に置いて画面内へ押し戻す (小さい窓でも消えない)。
 *
 * - 下/上: 対象の左端に揃え、右端が画面を越えるぶんだけ左へ寄せる
 * - 右/左: 対象の上端に揃え、下端が画面を越えるぶんだけ上へ寄せる
 */
export function placeCard(
  spot: Box,
  viewport: { width: number; height: number },
  card: { width: number; height: number },
  gap: number,
): CardPlacement {
  const clampX = (x: number) => Math.min(Math.max(8, x), Math.max(8, viewport.width - card.width - 8));
  const clampY = (y: number) => Math.min(Math.max(8, y), Math.max(8, viewport.height - card.height - 8));

  const belowTop = spot.top + spot.height + gap;
  if (belowTop + card.height <= viewport.height) {
    return { left: clampX(spot.left), top: belowTop, side: "below" };
  }
  const aboveTop = spot.top - gap - card.height;
  if (aboveTop >= 0) {
    return { left: clampX(spot.left), top: aboveTop, side: "above" };
  }
  const rightLeft = spot.left + spot.width + gap;
  if (rightLeft + card.width <= viewport.width) {
    return { left: rightLeft, top: clampY(spot.top), side: "right" };
  }
  const leftLeft = spot.left - gap - card.width;
  if (leftLeft >= 0) {
    return { left: leftLeft, top: clampY(spot.top), side: "left" };
  }
  return { left: clampX(spot.left), top: clampY(belowTop), side: "below" };
}
