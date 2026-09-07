import { describe, expect, it } from "vitest";
import { placeCard, shouldShowTour, spotlightBox, TOUR_STEPS } from "./tour";

describe("shouldShowTour — 初回だけ", () => {
  it("印が無く、モデルもパッケージも無ければ出す", () => {
    expect(shouldShowTour({ done: false, profileCount: 0, packageCount: 0 })).toBe(true);
  });
  it("一度見たら出さない", () => {
    expect(shouldShowTour({ done: true, profileCount: 0, packageCount: 0 })).toBe(false);
  });
  it("既に使っている痕跡 (モデル登録 / パッケージ登録) があれば初回ではない = 出さない", () => {
    expect(shouldShowTour({ done: false, profileCount: 1, packageCount: 0 })).toBe(false);
    expect(shouldShowTour({ done: false, profileCount: 0, packageCount: 2 })).toBe(false);
  });
  it("案内の順序は 設定 → パッケージ → 開始 → 入力", () => {
    expect([...TOUR_STEPS]).toEqual(["settings", "packages", "start", "input"]);
  });
});

describe("spotlightBox — 対象を pad だけ広げる", () => {
  it("四方に広がる", () => {
    expect(spotlightBox({ left: 100, top: 50, width: 20, height: 10 }, 6)).toEqual({
      left: 94,
      top: 44,
      width: 32,
      height: 22,
    });
  });
  it("画面の左上端に接する対象は負の座標へ出ない (はみ出しぶんは片側だけ広がる)", () => {
    expect(spotlightBox({ left: 2, top: 0, width: 20, height: 10 }, 6)).toEqual({
      left: 0,
      top: 0,
      width: 28, // 20 + 2 (左に寄れた分) + 6
      height: 16, // 10 + 0 + 6
    });
  });
});

describe("placeCard — 下 → 上 → 右 → 左 の順に収まる側へ", () => {
  const vp = { width: 1000, height: 600 };
  const card = { width: 300, height: 120 };

  it("余裕があれば下に、対象の左端に揃える", () => {
    const p = placeCard({ left: 200, top: 40, width: 30, height: 30 }, vp, card, 12);
    expect(p).toEqual({ left: 200, top: 82, side: "below" });
  });
  it("右端の対象 (タイトルバーの歯車など) は右へはみ出さないよう左へ寄せる", () => {
    const p = placeCard({ left: 960, top: 4, width: 30, height: 30 }, vp, card, 12);
    expect(p.side).toBe("below");
    expect(p.left + card.width).toBeLessThanOrEqual(vp.width - 8);
  });
  it("画面下端の対象 (入力欄) は上に出す", () => {
    const p = placeCard({ left: 100, top: 540, width: 600, height: 50 }, vp, card, 12);
    expect(p).toEqual({ left: 100, top: 540 - 12 - 120, side: "above" });
  });
  it("上下とも無理なら右へ", () => {
    const tall = { width: 1000, height: 200 };
    const p = placeCard({ left: 10, top: 40, width: 30, height: 130 }, tall, card, 12);
    expect(p.side).toBe("right");
    expect(p.left).toBe(52);
  });
  it("どこにも収まらなくても画面内に押し戻す (消えない)", () => {
    const tiny = { width: 320, height: 200 };
    const p = placeCard({ left: 0, top: 40, width: 320, height: 130 }, tiny, card, 12);
    expect(p.left).toBeGreaterThanOrEqual(8);
    expect(p.left + card.width).toBeLessThanOrEqual(tiny.width - 8);
    expect(p.top + card.height).toBeLessThanOrEqual(tiny.height - 8);
  });
});
