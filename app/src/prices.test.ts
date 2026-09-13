/**
 * spec 30 追補: 価格表 (prices.json) の照合。
 *
 * 行は実表 (betyourluck.github.io/prices.json、2026-08-20 取得) からそのまま写した — 実データには
 * **同じモデルが接頭辞違いで 9 行あり、地域つき (us./eu./jp./au.) は 1 割高い** (Bedrock)。
 * ここで守るのは「完全一致が地域違いの高い行に負けない」「価格が食い違う候補を黙って選ばない」の 2 つ =
 * どちらも破れると**間違った金額が登録に入る** (裁定 1 の「無いより悪い」)。
 */
import { describe, expect, it } from "vitest";
import { matchPrice, normalizeModelKey, parseContextTokens, pricingFormFromEntry, readContextTokens, type PriceTable } from "./prices";

const TABLE: PriceTable = {
  version: 1,
  fetched: "2026-08-20",
  models: [
    { key: "anthropic.claude-opus-4-8", max_input_tokens: 1000000, input_per_mtok: 5, output_per_mtok: 25, cache_read_per_mtok: 0.5 },
    { key: "anthropic/claude-opus-4-8", input_per_mtok: 5, output_per_mtok: 25, cache_read_per_mtok: 0.5 },
    { key: "us.anthropic.claude-opus-4-8", max_input_tokens: 1000000, input_per_mtok: 5.5, output_per_mtok: 27.5, cache_read_per_mtok: 0.55 },
    { key: "claude-opus-4-8", max_input_tokens: 1000000, input_per_mtok: 5, output_per_mtok: 25, cache_read_per_mtok: 0.5 },
    { key: "claude-opus-4-8@default", max_input_tokens: 1000000, input_per_mtok: 5, output_per_mtok: 25, cache_read_per_mtok: 0.5 },
    { key: "gemini-3-flash-preview", max_input_tokens: 1048576, input_per_mtok: 0.5, output_per_mtok: 3, cache_read_per_mtok: 0.05 },
    { key: "google/gemini-3-flash-preview", input_per_mtok: 0.5, output_per_mtok: 3, cache_read_per_mtok: 0.05 },
    { key: "gpt-5.6-luna", max_input_tokens: 922000, input_per_mtok: 0.2, output_per_mtok: 1.2, cache_read_per_mtok: 0.02 },
    { key: "openai.gpt-5.6-luna", max_input_tokens: 1050000, input_per_mtok: 0.22, output_per_mtok: 1.32, cache_read_per_mtok: 0.022 },
    { key: "openai/gpt-5.6-luna", input_per_mtok: 0.2, output_per_mtok: 1.2, cache_read_per_mtok: 0.02 },
    { key: "grok-4.3", input_per_mtok: 1.25, output_per_mtok: 2.5 },
    { key: "muse-spark-1.2-contributor", max_input_tokens: 1048576, input_per_mtok: 0.1, output_per_mtok: 0.2, cache_read_per_mtok: 0.002 },
  ],
};

describe("normalizeModelKey — 接頭辞と地域を剥ぐが、版番号の点は触らない", () => {
  it("提供者/地域の段と @ 以降を剥ぐ", () => {
    expect(normalizeModelKey("openai/gpt-5")).toBe("gpt-5");
    expect(normalizeModelKey("us.anthropic.claude-opus-4-8")).toBe("claude-opus-4-8");
    expect(normalizeModelKey("claude-opus-4-8@default")).toBe("claude-opus-4-8");
    expect(normalizeModelKey("openrouter/anthropic/claude-opus-4-8")).toBe("claude-opus-4-8");
  });
  it("gemini-3.5-flash の 3.5 や grok-4.3 の 4.3 は段ではない", () => {
    expect(normalizeModelKey("gemini-3.5-flash")).toBe("gemini-3.5-flash");
    expect(normalizeModelKey("grok-4.3")).toBe("grok-4.3");
    expect(normalizeModelKey("GPT-5")).toBe("gpt-5");
  });
});

describe("matchPrice — 完全一致が勝ち、価格が食い違う候補は選ばない", () => {
  it("完全一致は地域つきの高い行 (us. = 5.5) に負けず 5.0 を返す", () => {
    const m = matchPrice("claude-opus-4-8", TABLE);
    expect(m.kind).toBe("exact");
    if (m.kind !== "exact") return;
    expect(m.entry.key).toBe("claude-opus-4-8");
    expect(m.entry.input_per_mtok).toBe(5);
    expect(m.contextTokens).toBe(1000000);
  });
  it("完全一致の行に文脈長が無ければ、同じモデルの他の行から補う", () => {
    const m = matchPrice("google/gemini-3-flash-preview", TABLE);
    expect(m.kind).toBe("exact");
    if (m.kind !== "exact") return;
    expect(m.entry.max_input_tokens ?? undefined).toBeUndefined();
    expect(m.contextTokens).toBe(1048576);
  });
  it("完全一致が無く、正規化した候補の価格が全部同じなら素の行を選ぶ", () => {
    const m = matchPrice("vertex/gemini-3-flash-preview", TABLE);
    expect(m.kind).toBe("normalized");
    if (m.kind !== "normalized") return;
    expect(m.entry.key).toBe("gemini-3-flash-preview");
    expect(m.contextTokens).toBe(1048576);
  });
  it("候補の価格が食い違えば埋めず、候補を全部返す (openai. は 1 割高い)", () => {
    const m = matchPrice("azure/gpt-5.6-luna", TABLE);
    expect(m.kind).toBe("ambiguous");
    if (m.kind !== "ambiguous") return;
    expect(m.candidates.map((c) => c.key).sort()).toEqual(["gpt-5.6-luna", "openai.gpt-5.6-luna", "openai/gpt-5.6-luna"]);
  });
  it("表に無いモデル・空のモデル名は none", () => {
    expect(matchPrice("gemini-3.8-flash", TABLE)).toEqual({ kind: "none" });
    expect(matchPrice("   ", TABLE)).toEqual({ kind: "none" });
  });
});

describe("pricingFormFromEntry — キャッシュ読みが無い行は空のまま (推測で埋めない)", () => {
  it("3 欄揃う行は complete", () => {
    expect(pricingFormFromEntry(TABLE.models[3])).toEqual({ input: "5", cacheRead: "0.5", output: "25", complete: true });
  });
  it("キャッシュ読みの無い行 (grok-4.3) は cacheRead 空・complete false", () => {
    expect(pricingFormFromEntry(TABLE.models[10])).toEqual({ input: "1.25", cacheRead: "", output: "2.5", complete: false });
  });
});

describe("コンテキスト長の入力と保存値は正の整数だけ", () => {
  it("桁区切りは許し、空・小数・0・負は持たない", () => {
    expect(parseContextTokens("1,048,576")).toBe(1048576);
    expect(parseContextTokens(" 200000 ")).toBe(200000);
    expect(parseContextTokens("")).toBeUndefined();
    expect(parseContextTokens("1.5")).toBeUndefined();
    expect(parseContextTokens("0")).toBeUndefined();
    expect(parseContextTokens("-3")).toBeUndefined();
    expect(parseContextTokens("abc")).toBeUndefined();
  });
  it("localStorage の値も同じ規則", () => {
    expect(readContextTokens(128000)).toBe(128000);
    expect(readContextTokens("128000")).toBeUndefined();
    expect(readContextTokens(0)).toBeUndefined();
    expect(readContextTokens(1.5)).toBeUndefined();
  });
});
