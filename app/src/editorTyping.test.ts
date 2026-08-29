import { describe, expect, it } from "vitest";
import { findClusterBreak } from "@codemirror/state";
import { countChars, overwriteSpan } from "./editorTyping";

/** 実際の呼び出し形（CodeMirror の `findClusterBreak` を噛ませる）。 */
function span(line: string, offset: number): number {
  return overwriteSpan(line.length, offset, findClusterBreak(line, offset));
}

describe("overwriteSpan — 上書きで消す幅", () => {
  it("ふつうの 1 文字", () => {
    expect(span("abc", 0)).toBe(1);
    expect(span("あいう", 1)).toBe(1);
  });

  it("行末では消さない (次の行を食わない)", () => {
    expect(span("abc", 3)).toBe(0);
    expect(span("", 0)).toBe(0);
  });

  it("サロゲートペアを半分にしない", () => {
    const line = "a🎉b"; // 🎉 は UTF-16 で 2
    expect(span(line, 1)).toBe(2);
    // 行末判定も符号単位で数える (🎉 の後ろは 3)
    expect(span(line, 3)).toBe(1);
    expect(span(line, 4)).toBe(0);
  });

  it("結合文字を割らない", () => {
    const line = "が゙b"; // 濁点つき (結合文字)
    expect(span(line, 0)).toBeGreaterThanOrEqual(1);
    expect(span(line, 0)).toBe(findClusterBreak(line, 0));
  });

  it("クラスタが進まない異常値では触らない", () => {
    expect(overwriteSpan(5, 2, 2)).toBe(0);
    expect(overwriteSpan(5, 2, 1)).toBe(0);
  });

  it("行の長さを越えない", () => {
    expect(overwriteSpan(3, 2, 9)).toBe(1);
  });
});

describe("countChars — フッタの文字数", () => {
  it("コードポイントで数える (絵文字を 2 と数えない)", () => {
    expect(countChars("abc")).toBe(3);
    expect(countChars("あいう")).toBe(3);
    expect(countChars("🎉")).toBe(1);
    expect("🎉".length).toBe(2); // 素の .length との差 = この関数が在る理由
  });

  it("改行も 1 文字として数える (エディタの見え方に合わせる)", () => {
    expect(countChars("a\nb")).toBe(3);
    expect(countChars("")).toBe(0);
  });
});
