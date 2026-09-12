/**
 * spec 30 Phase C: 利用量の表示と金額の規則 — **純関数だけ** (`aiProfiles.ts` / `map.ts` と同じ枠 =
 * 提示層で唯一テストできる部分)。
 *
 * 金額の規則 (spec 30 C 節):
 * - ledger に**申告** (`reported_cost_requests > 0`) があれば申告の合計を出し、見積もらない。
 *   申告がリクエスト数より少なければ「部分申告」(未申告分は数えない)。
 * - 申告 0 件で `pricing` (3 欄すべて) があれば見積もり
 *   `(prompt − cache_read) × input + cache_read × cache_read + completion × output` (/1M)。
 * - どちらも無ければ金額なし。**既定価格は持たない** (間違った金額は無いより悪い)。
 */

export interface LlmLedger {
  requests: number;
  /** 総入力 (cache_read を含む — adapter が正規化済み、ここで足し直さない)。 */
  prompt_tokens: number;
  cache_read_tokens: number;
  completion_tokens: number;
  reported_cost_usd: number;
  reported_cost_requests: number;
}
export interface ImageLedger {
  requests: number;
  count: number;
  prompt_tokens: number;
  completion_tokens: number;
  elapsed_sec: number;
}
export interface RoleUsage {
  role: string;
  model_id: string;
  ledger: LlmLedger;
}
/** backend `usage_snapshot` の返り。 */
export interface UsageSnapshot {
  llm: RoleUsage[];
  total: LlmLedger;
  image: ImageLedger;
}
/** 登録モデルの単価 (USD / 100 万トークン)。**3 欄すべて数値** — 部分は持たない。 */
export interface Pricing {
  inputPerMtokUsd: number;
  cacheReadPerMtokUsd: number;
  outputPerMtokUsd: number;
}

export type CostView =
  | { kind: "reported"; usd: number }
  | { kind: "partial"; usd: number; reported: number; requests: number }
  | { kind: "estimated"; usd: number }
  | { kind: "none" };

export function costOf(l: LlmLedger, pricing?: Pricing): CostView {
  if (l.reported_cost_requests > 0) {
    if (l.reported_cost_requests < l.requests) {
      return { kind: "partial", usd: l.reported_cost_usd, reported: l.reported_cost_requests, requests: l.requests };
    }
    return { kind: "reported", usd: l.reported_cost_usd };
  }
  if (pricing && l.requests > 0) {
    const uncached = Math.max(0, l.prompt_tokens - l.cache_read_tokens);
    const usd =
      (uncached * pricing.inputPerMtokUsd +
        l.cache_read_tokens * pricing.cacheReadPerMtokUsd +
        l.completion_tokens * pricing.outputPerMtokUsd) /
      1e6;
    return { kind: "estimated", usd };
  }
  return { kind: "none" };
}

/** 設定フォームの 3 欄 → Pricing。**3 欄揃って有限・非負のときだけ** (部分は捨てる)。 */
export function parsePricing(raw: { input: string; cacheRead: string; output: string }): Pricing | undefined {
  const nums = [raw.input, raw.cacheRead, raw.output].map((s) => {
    const v = s.trim();
    return v === "" ? NaN : Number(v);
  });
  if (nums.some((n) => !Number.isFinite(n) || n < 0)) return undefined;
  return { inputPerMtokUsd: nums[0], cacheReadPerMtokUsd: nums[1], outputPerMtokUsd: nums[2] };
}

/** localStorage から読んだ値の検査 (欠け・非数・負は undefined = 金額なし)。 */
export function readPricing(v: unknown): Pricing | undefined {
  if (!v || typeof v !== "object") return undefined;
  const o = v as Record<string, unknown>;
  const n = [o.inputPerMtokUsd, o.cacheReadPerMtokUsd, o.outputPerMtokUsd];
  if (n.some((x) => typeof x !== "number" || !Number.isFinite(x) || x < 0)) return undefined;
  return { inputPerMtokUsd: n[0] as number, cacheReadPerMtokUsd: n[1] as number, outputPerMtokUsd: n[2] as number };
}

/** モデル id (ledger の `model_id`) に単価を持つ登録モデルがあればそれ。 */
export function pricingFor(modelId: string, profiles: { model: string; pricing?: Pricing }[]): Pricing | undefined {
  const id = modelId.trim();
  return profiles.find((p) => p.pricing && p.model.trim() === id)?.pricing;
}

/** 金額の表示 (spec 30 未決 1 の暫定: 1 ドル未満は 4 桁、以上は 2 桁、極小は下限を告げる)。 */
export function formatUsd(usd: number): string {
  if (usd === 0) return "$0";
  if (usd < 0.0001) return "< $0.0001";
  if (usd < 1) return `$${usd.toFixed(4)}`;
  return `$${usd.toFixed(2)}`;
}

export function formatTokens(n: number): string {
  return n.toLocaleString("en-US");
}

/** 画像のトークン欄。提供者が返さないセッション (ComfyUI だけ) は "-" (合算則: ledger は 0 で足し、表示で倒す)。 */
export function imageTokensLabel(l: ImageLedger): string {
  if (l.requests > 0 && l.prompt_tokens + l.completion_tokens === 0) return "-";
  return `${formatTokens(l.prompt_tokens)} / ${formatTokens(l.completion_tokens)}`;
}
