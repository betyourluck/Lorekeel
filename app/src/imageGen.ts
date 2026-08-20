// 画像生成 (spec 24) の設定 — **非秘密だけ** を localStorage に持つ。API キーは backend の
// app_data/.env (契約 config_sources)。型は backend の `image_gen::ImageGenConfig` と対応する
// (serde の rename_all = snake_case)。
//
// spec 26 (2026-08-21): 設定を共有部とプロバイダ別スロット (`perProvider`) に分離。
// プロバイダ切替は表示スロットの切替だけで、値は一切失われない (旧ヒューリスティック
// 「既定値なら差し替え」は撤去 — A 用のカスタム URL が B へ漏れる事故の根治)。
import comfyGeneric from "./assets/comfy_generic.json";

export type ImageProvider = "openai" | "gemini" | "comfy";
export type ImageShape = "square" | "landscape" | "portrait";
export type ImageDetail = "standard" | "high" | "highest";
export type ImagePromptStyle = "tags" | "prose";

/** プロバイダ別スロット (spec 26)。値の意味がプロバイダ (とその奥のモデル) に紐づくもの。 */
export interface ProviderSlot {
  baseUrl: string;
  model: string;
  /** "" = プロバイダ既定 (openai/gemini=prose, comfy=tags)。 */
  style: ImagePromptStyle | "";
  negative: string;
  workflowJson: string;
  /** 0 = プロバイダ別既定 (backend `ImageGenConfig::timeout()` が受ける)。 */
  timeoutSecs: number;
}

/** frontend の設定 (localStorage)。backend へは `toBackendConfig` で実効スロットを畳む。 */
export interface ImageGenSettings {
  /** 機能を使うか (操作列の表示条件)。プロバイダ別には持たない (spec 26 決定)。 */
  enabled: boolean;
  provider: ImageProvider;
  shape: ImageShape;
  detail: ImageDetail;
  /** 共有 — 「どんな絵が欲しいか」の意図。散文推奨 (タグの年齢バイアスは言い換え後も残る)。 */
  userPrefix: string;
  /** 第三層の不透明度 (0.3〜1.0)。 */
  opacity: number;
  /** 保存フォルダ ("" = 既定 app_data/images)。 */
  folder: string;
  perProvider: Record<ImageProvider, ProviderSlot>;
}

const KEY = "kataribe.imageGen";

export const DEFAULT_BASE_URL: Record<ImageProvider, string> = {
  openai: "https://api.openai.com/v1",
  gemini: "https://generativelanguage.googleapis.com",
  comfy: "http://127.0.0.1:8188",
};

export const DEFAULT_MODEL: Record<ImageProvider, string> = {
  openai: "gpt-image-1-mini",
  gemini: "gemini-3.1-flash-lite-image",
  comfy: "",
};

const PROVIDERS: ImageProvider[] = ["openai", "gemini", "comfy"];

/** スロットの既定値 (移行・部分欠損の埋め草)。baseUrl/model は UI placeholder と同じ既定。 */
export function defaultSlot(p: ImageProvider): ProviderSlot {
  return {
    baseUrl: DEFAULT_BASE_URL[p],
    model: DEFAULT_MODEL[p],
    style: "",
    negative: "",
    workflowJson: "",
    timeoutSecs: 0,
  };
}

export function defaultImageGenSettings(): ImageGenSettings {
  return {
    enabled: false,
    provider: "openai",
    shape: "landscape",
    detail: "standard",
    userPrefix: "",
    opacity: 0.85,
    folder: "",
    perProvider: {
      openai: defaultSlot("openai"),
      gemini: defaultSlot("gemini"),
      comfy: defaultSlot("comfy"),
    },
  };
}

/** 現プロバイダのスロット (欠損は既定 — 写像の `?? EMPTY_SLOT` ガードと同じ規則)。 */
export function currentSlot(s: ImageGenSettings): ProviderSlot {
  return s.perProvider?.[s.provider] ?? defaultSlot(s.provider);
}

function sanitizeSlot(p: ImageProvider, raw: unknown): ProviderSlot {
  const base = defaultSlot(p);
  if (!raw || typeof raw !== "object") return base;
  const r = raw as Partial<ProviderSlot>;
  return {
    baseUrl: typeof r.baseUrl === "string" ? r.baseUrl : base.baseUrl,
    model: typeof r.model === "string" ? r.model : base.model,
    style: r.style === "tags" || r.style === "prose" ? r.style : "",
    negative: typeof r.negative === "string" ? r.negative : "",
    workflowJson: typeof r.workflowJson === "string" ? r.workflowJson : "",
    timeoutSecs: typeof r.timeoutSecs === "number" && r.timeoutSecs > 0 ? r.timeoutSecs : 0,
  };
}

/**
 * 旧フラット形 → 新形式の一方向移行 (spec 26 何を作るか 4)。純関数。
 * - `perProvider` があれば新形式: 欠けたスロットだけ既定で埋める (将来プロバイダ追加後の旧データ)。
 * - 無ければ旧フラット形: baseUrl/model/style/negative/workflowJson/timeoutSecs を
 *   **その時の provider のスロットのみに**写す。他スロットは既定 — 旧 style が明示 tags でも
 *   複製しない (移行直後、現プロバイダの実効値は従来と一致。他プロバイダは既定に初期化され、
 *   フラット器由来の漏れはこの時点で解消される = 意図的な挙動変化)。
 */
export function migrateImageGenSettings(raw: unknown): ImageGenSettings {
  const out = defaultImageGenSettings();
  if (!raw || typeof raw !== "object") return out;
  const r = raw as Record<string, unknown>;
  if (typeof r.enabled === "boolean") out.enabled = r.enabled;
  if (r.provider === "openai" || r.provider === "gemini" || r.provider === "comfy") {
    out.provider = r.provider;
  }
  if (r.shape === "square" || r.shape === "landscape" || r.shape === "portrait") out.shape = r.shape;
  if (r.detail === "standard" || r.detail === "high" || r.detail === "highest") out.detail = r.detail;
  if (typeof r.userPrefix === "string") out.userPrefix = r.userPrefix;
  out.opacity = Math.min(1, Math.max(0.3, Number(r.opacity) || 0.85));
  if (typeof r.folder === "string") out.folder = r.folder;

  if (r.perProvider && typeof r.perProvider === "object") {
    const pp = r.perProvider as Record<string, unknown>;
    for (const p of PROVIDERS) {
      out.perProvider[p] = p in pp ? sanitizeSlot(p, pp[p]) : defaultSlot(p);
    }
    return out;
  }
  // 旧フラット形: 現 provider のスロットのみに写す。
  out.perProvider[out.provider] = sanitizeSlot(out.provider, {
    baseUrl: typeof r.baseUrl === "string" && r.baseUrl.trim() ? r.baseUrl : undefined,
    model: typeof r.model === "string" && r.model.trim() ? r.model : undefined,
    style: r.style,
    negative: r.negative,
    workflowJson: r.workflowJson,
    timeoutSecs: r.timeoutSecs,
  });
  return out;
}

export function loadImageGenSettings(): ImageGenSettings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return defaultImageGenSettings();
    return migrateImageGenSettings(JSON.parse(raw));
  } catch {
    return defaultImageGenSettings();
  }
}

/** 書き戻しは常に新形式。 */
export function saveImageGenSettings(s: ImageGenSettings): void {
  localStorage.setItem(KEY, JSON.stringify(s));
}

/** 様式の実効値 (契約 prompt_writer: 既定はプロバイダに倒す)。 */
export function effectiveStyle(s: ImageGenSettings): ImagePromptStyle {
  const slot = currentSlot(s);
  if (slot.style) return slot.style;
  return s.provider === "comfy" ? "tags" : "prose";
}

/** ネガティブプロンプトが効くプロバイダか (契約 negative_prompt)。 */
export function supportsNegative(p: ImageProvider): boolean {
  return p === "comfy";
}

/** 同梱の汎用 ComfyUI ワークフロー (API 形式、プレースホルダ入り)。 */
export function genericComfyWorkflow(): string {
  return JSON.stringify(comfyGeneric, null, 2);
}

/** backend `image_gen::ImageGenConfig` の形 (snake_case)。 */
export interface BackendImageGenConfig {
  provider: ImageProvider;
  base_url: string;
  model: string;
  shape: ImageShape;
  detail: ImageDetail;
  style: ImagePromptStyle | null;
  user_prefix: string;
  negative: string;
  workflow_json: string | null;
  timeout_secs: number | null;
}

/**
 * 実効値写像 (spec 26 何を作るか 3 で凍結)。**他プロバイダのスロットは一切読まない** —
 * provider のスロット + 共有部だけが wire に乗る。negative/workflowJson は comfy 以外で
 * 送らない (逆向き漏れの封鎖)。timeout の既定は backend `ImageGenConfig::timeout()` が
 * 受ける (frontend に既定表を複製しない — base_url/model の既定は UI placeholder が
 * 表示に使うので frontend が持つ、という線引き)。
 */
export function toBackendConfig(s: ImageGenSettings): BackendImageGenConfig {
  const slot = currentSlot(s);
  return {
    provider: s.provider,
    base_url: slot.baseUrl.trim() || DEFAULT_BASE_URL[s.provider],
    // comfy はワークフロー側でモデルが決まるので空を許容 (DEFAULT_MODEL.comfy = "" だが写像として明示)。
    model: slot.model.trim() || (s.provider === "comfy" ? "" : DEFAULT_MODEL[s.provider]),
    shape: s.shape,
    detail: s.detail,
    style: slot.style || null,
    user_prefix: s.userPrefix,
    negative: s.provider === "comfy" ? slot.negative : "",
    workflow_json: s.provider === "comfy" && slot.workflowJson.trim() ? slot.workflowJson : null,
    timeout_secs: slot.timeoutSecs > 0 ? Math.floor(slot.timeoutSecs) : null,
  };
}

/** 保存名の日時 stamp (saveLog と同じ形)。 */
export function imageStamp(now = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return (
    `${now.getFullYear()}${p(now.getMonth() + 1)}${p(now.getDate())}` +
    `_${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}`
  );
}
