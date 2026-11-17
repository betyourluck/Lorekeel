import { describe, expect, it } from "vitest";

import { isNearBottom, NEAR_BOTTOM_PX } from "./logScroll";

/** 高さ 2000 の中身を、高さ 600 の窓から見ている形。末尾の scrollTop は 1400。 */
const at = (scrollTop: number) => ({ scrollHeight: 2000, clientHeight: 600, scrollTop });

describe("会話ログの末尾の判定", () => {
  it("一番下では末尾を見ている", () => {
    expect(isNearBottom(at(1400))).toBe(true);
  });

  it("途中まで戻ると末尾を見ていない", () => {
    expect(isNearBottom(at(0))).toBe(false);
    expect(isNearBottom(at(1000))).toBe(false);
  });

  it("境界は「未満」— ちょうどしきい値ぶん残っていれば、まだ末尾ではない", () => {
    expect(isNearBottom(at(1400 - NEAR_BOTTOM_PX))).toBe(false);
    expect(isNearBottom(at(1400 - NEAR_BOTTOM_PX + 1))).toBe(true);
  });

  it("表示倍率で scrollTop が端数になり、厳密な末尾に届かなくても末尾とみなす", () => {
    expect(isNearBottom(at(1399.4))).toBe(true);
  });

  it("中身が窓より短い (スクロールできない) なら、いつも末尾を見ている", () => {
    expect(isNearBottom({ scrollHeight: 300, clientHeight: 600, scrollTop: 0 })).toBe(true);
    expect(isNearBottom({ scrollHeight: 600, clientHeight: 600, scrollTop: 0 })).toBe(true);
  });

  it("しきい値はログの下の余白 (py-5 = 20px) より広い — 余白に入った時点でボタンが消える", () => {
    expect(NEAR_BOTTOM_PX).toBeGreaterThan(20);
  });
});
