// 画像生成 (spec 24) の設定 — **非秘密だけ** を localStorage に持つ。API キーは backend の
// app_data/.env (契約 config_sources)。型は backend の `image_gen::ImageGenConfig` と対応する
// (serde の rename_all = snake_case)。
import comfyGeneric from "./assets/comfy_generic.json";

export type ImageProvider = "openai" | "gemini" | "comfy";
export type ImageShape = "square" | "landscape" | "portrait";
export type ImageDetail = "standard" | "high" | "highest";
export type ImagePromptStyle = "tags" | "prose";

/** frontend の設定 (localStorage)。backend へは `toBackendConfig` で写す。 */
export interface ImageGenSettings {
  /** 機能を使うか (操作列の表示条件)。 */
  enabled: boolean;
  provider: ImageProvider;
  baseUrl: string;
  model: string;
  shape: ImageShape;
  detail: ImageDetail;
  /** "" = プロバイダ既定 (openai/gemini=prose, comfy=tags)。 */
  style: ImagePromptStyle | "";
  userPrefix: string;
  negative: string;
  workflowJson: string;
  /** 0 = プロバイダ別既定。 */
  timeoutSecs: number;
  /** 第三層の不透明度 (0.3〜1.0)。 */
  opacity: number;
  /** 保存フォルダ ("" = 既定 app_data/images)。 */
  folder: string;
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

export function defaultImageGenSettings(): ImageGenSettings {
  return {
    enabled: false,
    provider: "openai",
    baseUrl: DEFAULT_BASE_URL.openai,
    model: DEFAULT_MODEL.openai,
    shape: "landscape",
    detail: "standard",
    style: "",
    userPrefix: "",
    negative: "",
    workflowJson: "",
    timeoutSecs: 0,
    opacity: 0.85,
    folder: "",
  };
}

export function loadImageGenSettings(): ImageGenSettings {
  const base = defaultImageGenSettings();
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return base;
    const parsed = JSON.parse(raw) as Partial<ImageGenSettings>;
    const merged = { ...base, ...parsed };
    merged.opacity = Math.min(1, Math.max(0.3, Number(merged.opacity) || 0.85));
    return merged;
  } catch {
    return base;
  }
}

export function saveImageGenSettings(s: ImageGenSettings): void {
  localStorage.setItem(KEY, JSON.stringify(s));
}

/** 様式の実効値 (契約 prompt_writer: 既定はプロバイダに倒す)。 */
export function effectiveStyle(s: ImageGenSettings): ImagePromptStyle {
  if (s.style) return s.style;
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

export function toBackendConfig(s: ImageGenSettings): BackendImageGenConfig {
  return {
    provider: s.provider,
    base_url: s.baseUrl.trim() || DEFAULT_BASE_URL[s.provider],
    model: s.model.trim(),
    shape: s.shape,
    detail: s.detail,
    style: s.style || null,
    user_prefix: s.userPrefix,
    negative: supportsNegative(s.provider) ? s.negative : "",
    workflow_json: s.provider === "comfy" && s.workflowJson.trim() ? s.workflowJson : null,
    timeout_secs: s.timeoutSecs > 0 ? Math.floor(s.timeoutSecs) : null,
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
