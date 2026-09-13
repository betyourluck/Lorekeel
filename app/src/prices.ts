/**
 * spec 30 追補 (2026-09-13): 価格表 (prices.json) からモデル id へ単価とコンテキスト長を引く — **純関数だけ**
 * (`usage.ts` / `aiProfiles.ts` と同じ枠 = 提示層で唯一テストできる部分)。
 *
 * 取得は backend `fetch_price_table` (CSP の都合で WebView から直接は叩けない)、ここは**照合と欄埋め**。
 * 裁定 1「価格表を持たない・焼き込み既定なし」との線引き = 表はユーザーが保守する外部ソースで、
 * 取り込みは押したときだけ (opt-in)・出所と取得日を必ず添える・書き込みは従来の保存経路を通る。
 *
 * 照合は保守的に: 完全一致 → 正規化 (プロバイダ接頭辞と地域接頭辞を剥いだ形) の一致。
 * **候補が複数あり価格が食い違えば埋めない** (候補を見せて人に選ばせる = 間違った金額は無いより悪い)。
 */

export interface PriceEntry {
  key: string;
  max_input_tokens?: number | null;
  input_per_mtok: number;
  output_per_mtok: number;
  cache_read_per_mtok?: number | null;
}
/** backend `fetch_price_table` の返り。 */
export interface PriceTable {
  version: number;
  fetched: string;
  models: PriceEntry[];
}

export const DEFAULT_PRICE_TABLE_URL = "https://betyourluck.github.io/prices.json";
export const PRICE_TABLE_URL_KEY = "kataribe.priceTableUrl";

/**
 * モデル名の正規化: `openai/gpt-5` → `gpt-5`、`us.anthropic.claude-opus-4-8` → `claude-opus-4-8`、
 * `claude-opus-4-8@default` → `claude-opus-4-8`。剥がすのは **英字だけの段** (`anthropic.` / `us.`) で、
 * `gemini-3.5-flash` の `3.5` のような数字入りは触らない。大文字小文字は区別しない (`GPT-5` = `gpt-5`)。
 */
export function normalizeModelKey(key: string): string {
  let k = key.trim().toLowerCase();
  const slash = k.lastIndexOf("/");
  if (slash >= 0) k = k.slice(slash + 1);
  const at = k.indexOf("@");
  if (at > 0) k = k.slice(0, at);
  // 英字だけの段を前から剥がす (最後の段は残す = "anthropic" 単体を空にしない)
  for (;;) {
    const m = /^[a-z]+\.(.+)$/.exec(k);
    if (!m) break;
    k = m[1];
  }
  return k;
}

export type PriceMatch =
  | { kind: "exact"; entry: PriceEntry; contextTokens?: number }
  | { kind: "normalized"; entry: PriceEntry; contextTokens?: number }
  | { kind: "ambiguous"; candidates: PriceEntry[] }
  | { kind: "none" };

function samePrice(a: PriceEntry, b: PriceEntry): boolean {
  return (
    a.input_per_mtok === b.input_per_mtok &&
    a.output_per_mtok === b.output_per_mtok &&
    (a.cache_read_per_mtok ?? null) === (b.cache_read_per_mtok ?? null)
  );
}

/** 候補群から文脈長を一つ選ぶ (選んだ行に無ければ同じモデルの他の行から。全部無ければ undefined)。 */
function contextFrom(chosen: PriceEntry, siblings: PriceEntry[]): number | undefined {
  const pick = [chosen, ...siblings].find((e) => typeof e.max_input_tokens === "number" && e.max_input_tokens > 0);
  return pick?.max_input_tokens ?? undefined;
}

export function matchPrice(modelId: string, table: PriceTable): PriceMatch {
  const id = modelId.trim();
  if (!id) return { kind: "none" };
  const exact = table.models.find((e) => e.key === id);
  const norm = normalizeModelKey(id);
  const siblings = table.models.filter((e) => e !== exact && normalizeModelKey(e.key) === norm);
  if (exact) return { kind: "exact", entry: exact, contextTokens: contextFrom(exact, siblings) };
  if (siblings.length === 0) return { kind: "none" };
  if (siblings.some((e) => !samePrice(e, siblings[0]))) return { kind: "ambiguous", candidates: siblings };
  // 同価格の候補: 剥いだ結果がそのままキーの行 (地域・提供者接頭辞の無い素の行) を優先、無ければ最短のキー
  const bare = siblings.find((e) => e.key.toLowerCase() === norm);
  const chosen = bare ?? siblings.slice().sort((a, b) => a.key.length - b.key.length || (a.key < b.key ? -1 : 1))[0];
  return { kind: "normalized", entry: chosen, contextTokens: contextFrom(chosen, siblings) };
}

/**
 * 価格表の行 → 単価フォームの 3 欄。キャッシュ読みの単価が表に無い行は**空のまま**にする
 * (入力単価を写して「割引なし」とするのは推測 = 埋めない。3 欄揃わないと見積もりは出ない、が既存の規則)。
 */
export function pricingFormFromEntry(e: PriceEntry): { input: string; cacheRead: string; output: string; complete: boolean } {
  const cache = typeof e.cache_read_per_mtok === "number" ? String(e.cache_read_per_mtok) : "";
  return { input: String(e.input_per_mtok), cacheRead: cache, output: String(e.output_per_mtok), complete: cache !== "" };
}

/** コンテキスト長の入力欄 → 数値 (正の整数だけ。空・非数・0 以下は undefined = 持たない)。 */
export function parseContextTokens(raw: string): number | undefined {
  const s = raw.trim().replace(/[,_]/g, "");
  if (s === "") return undefined;
  const n = Number(s);
  if (!Number.isInteger(n) || n <= 0) return undefined;
  return n;
}

/** localStorage から読んだ値の検査 (正の整数だけ)。 */
export function readContextTokens(v: unknown): number | undefined {
  return typeof v === "number" && Number.isInteger(v) && v > 0 ? v : undefined;
}

/** 候補の一覧表示 (曖昧なとき): `key: in/cache/out`。 */
export function describeCandidate(e: PriceEntry): string {
  const cache = typeof e.cache_read_per_mtok === "number" ? String(e.cache_read_per_mtok) : "-";
  return `${e.key}: ${e.input_per_mtok} / ${cache} / ${e.output_per_mtok}`;
}
