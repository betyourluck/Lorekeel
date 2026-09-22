import { describe, expect, it } from "vitest";

import type { AiModelProfile } from "./aiProfiles";
import { contextUsage, formatPercent, formatTokens, toneOf, windowFor } from "./contextGauge";

function p(model: string, contextTokens?: number): AiModelProfile {
  return {
    id: model + String(contextTokens),
    name: model,
    model,
    baseUrl: "b",
    apiKey: "k",
    useTools: true,
    effort: "",
    maxTokens: "",
    contextTokens,
  };
}

describe("windowFor", () => {
  it("登録簿から引く", () => {
    expect(windowFor("muse-spark-1.3-contributor", [p("x", 1000), p("muse-spark-1.3-contributor", 1048576)])).toBe(1048576);
  });
  it("未設定・不在・空のモデル名では undefined (推測しない)", () => {
    expect(windowFor("m", [p("m")])).toBeUndefined();
    expect(windowFor("m", [p("other", 100)])).toBeUndefined();
    expect(windowFor("  ", [p("m", 100)])).toBeUndefined();
    expect(windowFor("m", [p("m", 0)])).toBeUndefined();
  });
  it("同じモデルの登録が複数あって窓が食い違えば undefined、一致していれば採る", () => {
    expect(windowFor("m", [p("m", 200000), p("m", 1000000)])).toBeUndefined();
    expect(windowFor("m", [p("m", 200000), p("m", 200000)])).toBe(200000);
  });
});

describe("contextUsage", () => {
  it("分子か分母が無ければ null = 出さない", () => {
    expect(contextUsage(0, 1000)).toBeNull();
    expect(contextUsage(undefined, 1000)).toBeNull();
    expect(contextUsage(100, undefined)).toBeNull();
    expect(contextUsage(100, 0)).toBeNull();
  });
  it("実測の値で比率が出る (muse-spark / opus)", () => {
    expect(contextUsage(14123, 1048576)!.ratio).toBeCloseTo(0.01347, 4);
    expect(contextUsage(30549, 1000000)!.ratio).toBeCloseTo(0.030549, 5);
  });
  it("窓より大きい入力は 1.0 で飽和する (分母の誤りで超えうる)", () => {
    expect(contextUsage(200, 100)!.ratio).toBe(1);
    expect(contextUsage(200, 100)!.tone).toBe("critical");
  });
});

describe("toneOf の境界", () => {
  it("0.75 未満は normal / 0.75 以上 warn / 0.9 以上 critical", () => {
    expect(toneOf(0.7499)).toBe("normal");
    expect(toneOf(0.75)).toBe("warn");
    expect(toneOf(0.8999)).toBe("warn");
    expect(toneOf(0.9)).toBe("critical");
  });
});

describe("整形", () => {
  it("K / M で丸める", () => {
    expect(formatTokens(999)).toBe("999");
    expect(formatTokens(14123)).toBe("14.1K");
    expect(formatTokens(1048576)).toBe("1.0M");
  });
  it("0% と出さない (壊れて見える)", () => {
    expect(formatPercent(0.0004)).toBe("<0.1%");
    expect(formatPercent(0.0135)).toBe("1.4%");
    expect(formatPercent(0.31)).toBe("31%");
  });
});
