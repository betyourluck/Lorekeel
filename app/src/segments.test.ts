/** モブの台詞を地の文へ畳む (2026-10-15 ユーザー要望)。 */
import { describe, expect, it } from "vitest";

import { foldMobLines, isMobLine } from "./segments";

describe("foldMobLines", () => {
  it("名前だけの話者は 名前「…」 の地の文に、地の文と登場人物はそのまま", () => {
    const segs = [
      { speaker: null, name: "", icon: null, text: "戸が開く。" },
      { speaker: null, name: " 店員 ", icon: null, text: "いらっしゃい" },
      { speaker: "rinka", name: "凛花", icon: "r.webp", text: "遅いわね" },
    ];
    expect(foldMobLines(segs)).toEqual([
      { speaker: null, name: "", icon: null, text: "戸が開く。" },
      { speaker: null, name: "", icon: null, text: "店員「いらっしゃい」" },
      { speaker: "rinka", name: "凛花", icon: "r.webp", text: "遅いわね" },
    ]);
    expect(segs[1].name).toBe(" 店員 "); // 元の配列は書き換えない
  });

  it("モブの判定: 名前があり話者 id が無いものだけ", () => {
    expect(isMobLine({ speaker: null, name: "店員", text: "" })).toBe(true);
    expect(isMobLine({ speaker: null, name: "  ", text: "" })).toBe(false);
    expect(isMobLine({ speaker: "rinka", name: "凛花", text: "" })).toBe(false);
  });
});
