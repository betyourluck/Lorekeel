/**
 * spec 30 Phase C: 金額の規則と表示の純関数。
 *
 * 見た目は測れないが、「申告があれば見積もらない」「3 欄揃わなければ金額なし」は数字で決まる。
 * ここが崩れると **二重計上** (申告 + 見積もり) か **部分見積もり** (揃っていない単価で計算) が
 * 画面に出る — どちらも間違った金額で、無いより悪い。
 */
import { describe, expect, it } from "vitest";
import { costOf, formatUsd, imageTokensLabel, parsePricing, pricingFor, readPricing, type LlmLedger } from "./usage";

const P = { inputPerMtokUsd: 3, cacheReadPerMtokUsd: 0.3, outputPerMtokUsd: 15 };
const ledger = (o: Partial<LlmLedger>): LlmLedger => ({
  requests: 0,
  prompt_tokens: 0,
  cache_read_tokens: 0,
  completion_tokens: 0,
  reported_cost_usd: 0,
  reported_cost_requests: 0,
  ...o,
});

describe("costOf — 申告優先・見積もりは 3 欄揃ったときだけ", () => {
  it("申告があれば単価があっても見積もらない (二重計上を作らない)", () => {
    const l = ledger({ requests: 2, prompt_tokens: 20000, completion_tokens: 500, reported_cost_usd: 0.02, reported_cost_requests: 2 });
    expect(costOf(l, P)).toEqual({ kind: "reported", usd: 0.02 });
  });
  it("部分申告は申告分だけを出し、未申告分は数えない", () => {
    const l = ledger({ requests: 3, prompt_tokens: 30000, reported_cost_usd: 0.01, reported_cost_requests: 1 });
    expect(costOf(l, P)).toEqual({ kind: "partial", usd: 0.01, reported: 1, requests: 3 });
  });
  it("申告 0 件 + 単価あり = (prompt − cache_read)×input + cache_read×cache + completion×output (/1M)", () => {
    const l = ledger({ requests: 1, prompt_tokens: 10000, cache_read_tokens: 8000, completion_tokens: 1000 });
    const c = costOf(l, P);
    expect(c.kind).toBe("estimated");
    // 2000×3 + 8000×0.3 + 1000×15 = 6000 + 2400 + 15000 = 23400 / 1e6
    expect((c as { usd: number }).usd).toBeCloseTo(0.0234, 9);
  });
  it("cache_read が prompt を上回る壊れた値でも負にならない", () => {
    const l = ledger({ requests: 1, prompt_tokens: 100, cache_read_tokens: 200 });
    expect((costOf(l, P) as { usd: number }).usd).toBeCloseTo((200 * 0.3) / 1e6, 12);
  });
  it("単価が無ければ金額なし・リクエスト 0 でも金額なし", () => {
    expect(costOf(ledger({ requests: 1, prompt_tokens: 10 }))).toEqual({ kind: "none" });
    expect(costOf(ledger({}), P)).toEqual({ kind: "none" });
  });
});

describe("parsePricing / readPricing — 3 欄揃って有限・非負のときだけ", () => {
  it("揃えば数値に、空・非数・負が 1 つでもあれば undefined", () => {
    expect(parsePricing({ input: "3", cacheRead: "0.3", output: " 15 " })).toEqual(P);
    expect(parsePricing({ input: "3", cacheRead: "", output: "15" })).toBeUndefined();
    expect(parsePricing({ input: "3", cacheRead: "abc", output: "15" })).toBeUndefined();
    expect(parsePricing({ input: "-1", cacheRead: "0", output: "0" })).toBeUndefined();
    expect(parsePricing({ input: "0", cacheRead: "0", output: "0" })).toEqual({ inputPerMtokUsd: 0, cacheReadPerMtokUsd: 0, outputPerMtokUsd: 0 });
  });
  it("localStorage の値も同じ規則で検査する", () => {
    expect(readPricing(P)).toEqual(P);
    expect(readPricing({ inputPerMtokUsd: 3 })).toBeUndefined();
    expect(readPricing({ ...P, outputPerMtokUsd: "15" })).toBeUndefined();
    expect(readPricing(null)).toBeUndefined();
  });
  it("pricingFor は model id が一致し単価を持つ登録だけ", () => {
    const profiles = [
      { model: "gpt-x", pricing: undefined },
      { model: "claude-y", pricing: P },
    ];
    expect(pricingFor("claude-y", profiles)).toEqual(P);
    expect(pricingFor("gpt-x", profiles)).toBeUndefined();
    expect(pricingFor("unknown", profiles)).toBeUndefined();
  });
});

describe("表示", () => {
  it("formatUsd の桁", () => {
    expect(formatUsd(0)).toBe("$0");
    expect(formatUsd(0.00001)).toBe("< $0.0001");
    expect(formatUsd(0.0234)).toBe("$0.0234");
    expect(formatUsd(12.345)).toBe("$12.35");
  });
  it("画像のトークン欄は ComfyUI だけのセッションで '-'、返れば二値", () => {
    expect(imageTokensLabel({ requests: 2, count: 2, prompt_tokens: 0, completion_tokens: 0, elapsed_sec: 24 })).toBe("-");
    expect(imageTokensLabel({ requests: 1, count: 1, prompt_tokens: 272, completion_tokens: 1056, elapsed_sec: 24.8 })).toBe("272 / 1,056");
    expect(imageTokensLabel({ requests: 0, count: 0, prompt_tokens: 0, completion_tokens: 0, elapsed_sec: 0 })).toBe("0 / 0");
  });
});
