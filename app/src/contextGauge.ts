/**
 * コンテキストウィンドウの使用率 (2026-09-23)。
 *
 * 分子は **直近 1 回の GM 呼び出しの総入力** (`CacheStat.last_prompt`、cache 込み) で、
 * ターンや周の合計ではない — 窓の使用率は「いま 1 回のリクエストが窓のどれだけを占めたか」。
 * 却下で 2 回呼ばれたターンでは最後の 1 回が残る (backend の `record` が代入なので自然にそうなる)。
 *
 * 分母は選択中の登録モデルの `contextTokens`。**推測しない** — 未設定なら出さない
 * (既定 128,000 のような番兵を置くと、実際は 1M の窓で「23% 使用」という嘘になる)。
 *
 * 実測 (2026-09-23): muse-spark 14,123/1,048,576 = 1.3% / opus 30,549/1,000,000 = 3.1%。
 * **輪はほぼ空のまま**なので、これは警告の道具ではなく「どれだけ余っているか」の可視化。
 */
import type { AiModelProfile } from "./aiProfiles";

export type ContextTone = "normal" | "warn" | "critical";

export interface ContextUsage {
  /** 直近リクエストの総入力トークン。 */
  used: number;
  /** モデルの窓 (トークン)。 */
  window: number;
  /** 0.0〜1.0 (1.0 で飽和 — 窓より大きい入力は通らないが、分母の誤りで超えうる)。 */
  ratio: number;
  tone: ContextTone;
}

/** 色の段 (Fuseforks spec 49 と同じ閾値 — 利用者がそちらで見慣れている)。 */
export function toneOf(ratio: number): ContextTone {
  if (ratio >= 0.9) return "critical";
  if (ratio >= 0.75) return "warn";
  return "normal";
}

/**
 * 選択中モデルの窓を登録簿から引く。
 *
 * **候補が複数あって窓が食い違えば `undefined`** — 推測の分母で % を出すのは嘘になる
 * (prices.ts の「候補が複数あり価格が食い違えば埋めない」と同じ規律)。
 */
export function windowFor(model: string, profiles: AiModelProfile[]): number | undefined {
  const m = model.trim();
  if (!m) return undefined;
  const hits = profiles.filter((p) => p.model.trim() === m && typeof p.contextTokens === "number" && p.contextTokens > 0);
  if (hits.length === 0) return undefined;
  const first = hits[0].contextTokens as number;
  return hits.every((p) => p.contextTokens === first) ? first : undefined;
}

/** 分子と分母から使用率を組む。どちらかが無ければ `null` (= 出さない)。 */
export function contextUsage(used: number | undefined, window: number | undefined): ContextUsage | null {
  if (typeof used !== "number" || used <= 0) return null;
  if (typeof window !== "number" || window <= 0) return null;
  const ratio = Math.min(used / window, 1);
  return { used, window, ratio, tone: toneOf(ratio) };
}

/** 14123 -> "14.1K" / 1048576 -> "1.0M" / 999 -> "999"。 */
export function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(Math.round(n));
}

/** 表示用の百分率。0.013 -> "1.3%" / 0.0004 -> "<0.1%" (0% と出すと壊れて見える)。 */
export function formatPercent(ratio: number): string {
  const pct = ratio * 100;
  if (pct > 0 && pct < 0.1) return "<0.1%";
  return `${pct < 10 ? pct.toFixed(1) : Math.round(pct)}%`;
}
