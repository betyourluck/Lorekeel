import { describe, expect, it } from "vitest";
import {
  DEFAULT_BASE_URL,
  DEFAULT_MODEL,
  defaultImageGenSettings,
  effectiveStyle,
  migrateImageGenSettings,
  supportsDetail,
  supportsNegative,
  toBackendConfig,
} from "./imageGen";

// Meta muse-image (2026-10-05) を足したときの設定の移行と写像。
describe("Meta の画像生成プロバイダ", () => {
  it("既定値と backend への写像 (散文・negative とワークフローは送らない)", () => {
    const s = { ...defaultImageGenSettings(), provider: "meta" as const };
    expect(DEFAULT_BASE_URL.meta).toBe("https://api.meta.ai/v1");
    expect(DEFAULT_MODEL.meta).toBe("muse-image-1.0");
    const b = toBackendConfig(s);
    expect(b.provider).toBe("meta");
    expect(b.base_url).toBe("https://api.meta.ai/v1");
    expect(b.model).toBe("muse-image-1.0");
    expect(b.negative).toBe("");
    expect(b.workflow_json).toBeNull();
    expect(b.lock_seed).toBe(false);
    expect(effectiveStyle(s)).toBe("prose");
  });

  it("保存済みの設定で provider: meta を読める", () => {
    const m = migrateImageGenSettings({ provider: "meta", perProvider: {} });
    expect(m.provider).toBe("meta");
  });

  it("meta のスロットを持たない古い保存データでも既定のスロットが埋まる", () => {
    const old = {
      provider: "openai",
      perProvider: { openai: { baseUrl: "https://x/v1", model: "m" } },
    };
    const m = migrateImageGenSettings(old);
    expect(m.perProvider.meta.baseUrl).toBe("https://api.meta.ai/v1");
    expect(m.perProvider.openai.baseUrl).toBe("https://x/v1");
  });

  it("解像度段が効くのは OpenAI と Gemini だけ (Meta は約 2.5MP 固定・ComfyUI は形だけ)", () => {
    expect(supportsDetail("openai")).toBe(true);
    expect(supportsDetail("gemini")).toBe(true);
    expect(supportsDetail("meta")).toBe(false);
    expect(supportsDetail("comfy")).toBe(false);
    expect(supportsNegative("meta")).toBe(false);
  });
});
