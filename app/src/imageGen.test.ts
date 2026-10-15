import { describe, expect, it } from "vitest";
import {
  DEFAULT_BASE_URL,
  DEFAULT_MODEL,
  defaultImageGenSettings,
  effectiveStyle,
  migrateImageGenSettings,
  supportsDetail,
  supportsHighestDetail,
  supportsNegative,
  toBackendConfig,
  withWorkflowIndex,
  withWorkflowName,
  withWorkflowText,
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

// xAI grok-imagine (2026-10-09)。解像度段は (resolution, quality) の組に写る (backend の SizeMap)。
describe("xAI の画像生成プロバイダ", () => {
  it("既定値と backend への写像 (散文・negative とワークフローは送らない)", () => {
    const s = { ...defaultImageGenSettings(), provider: "xai" as const };
    expect(DEFAULT_BASE_URL.xai).toBe("https://api.x.ai/v1");
    expect(DEFAULT_MODEL.xai).toBe("grok-imagine-image-2.0");
    const b = toBackendConfig(s);
    expect(b.provider).toBe("xai");
    expect(b.base_url).toBe("https://api.x.ai/v1");
    expect(b.model).toBe("grok-imagine-image-2.0");
    expect(b.negative).toBe("");
    expect(b.workflow_json).toBeNull();
    expect(b.lock_seed).toBe(false);
    expect(effectiveStyle(s)).toBe("prose");
  });

  it("保存済みの provider: xai を読め、xai のスロットを持たない古いデータは既定で埋まる", () => {
    expect(migrateImageGenSettings({ provider: "xai", perProvider: {} }).provider).toBe("xai");
    const m = migrateImageGenSettings({ provider: "meta", perProvider: { meta: { baseUrl: "https://m/v1" } } });
    expect(m.perProvider.xai.baseUrl).toBe("https://api.x.ai/v1");
    expect(m.perProvider.meta.baseUrl).toBe("https://m/v1");
  });

  it("解像度段は効き、「最高」(2k + medium) も別の段として出す", () => {
    expect(supportsDetail("xai")).toBe(true);
    expect(supportsHighestDetail("xai")).toBe(true);
    expect(supportsHighestDetail("openai")).toBe(true);
    expect(supportsHighestDetail("gemini")).toBe(false);
    expect(supportsNegative("xai")).toBe(false);
  });
});

// ComfyUI のワークフローの保管枠 (2026-10-15、3 つまで)。
describe("ワークフローの保管枠", () => {
  it("保管枠を持たない旧データは、今のワークフローが 1 枠目に入る (黙って消えない)", () => {
    const old = { provider: "comfy", perProvider: { comfy: { workflowJson: '{"old":1}' } } };
    const slot = migrateImageGenSettings(old).perProvider.comfy;
    expect(slot.workflowIndex).toBe(0);
    expect(slot.workflowSlots).toEqual(['{"old":1}', "", ""]);
    // 旧フラット形 (spec 26 以前) も同じ。
    const flat = migrateImageGenSettings({ provider: "comfy", workflowJson: '{"flat":1}' }).perProvider.comfy;
    expect(flat.workflowSlots).toEqual(['{"flat":1}', "", ""]);
  });

  it("壊れた保管データは枠数ちょうどに揃え、選んでいる枠は workflowJson を正とする", () => {
    const raw = {
      provider: "comfy",
      perProvider: { comfy: { workflowJson: "B", workflowSlots: ["x", 5, "y", "z"], workflowIndex: 9 } },
    };
    const slot = migrateImageGenSettings(raw).perProvider.comfy;
    expect(slot.workflowIndex).toBe(0);
    expect(slot.workflowSlots).toEqual(["B", "", "y"]);
    expect(slot.workflowNames).toEqual(["", "", ""]);
  });

  it("書き換え・切り替え・名前は選んでいる枠にだけ効き、送るのは選んでいる枠", () => {
    const s = { ...defaultImageGenSettings(), provider: "comfy" as const };
    const apply = (patch: object) => {
      s.perProvider = { ...s.perProvider, comfy: { ...s.perProvider.comfy, ...patch } };
    };
    apply(withWorkflowText(s.perProvider.comfy, "A"));
    apply(withWorkflowName(s.perProvider.comfy, "SDXL"));
    apply(withWorkflowIndex(s.perProvider.comfy, 2));
    apply(withWorkflowText(s.perProvider.comfy, "C"));
    expect(toBackendConfig(s).workflow_json).toBe("C");
    apply(withWorkflowIndex(s.perProvider.comfy, 0));
    expect(toBackendConfig(s).workflow_json).toBe("A");
    expect(s.perProvider.comfy.workflowSlots).toEqual(["A", "", "C"]);
    expect(s.perProvider.comfy.workflowNames).toEqual(["SDXL", "", ""]);
    // 範囲外は端に寄せる。
    expect(withWorkflowIndex(s.perProvider.comfy, 7).workflowIndex).toBe(2);
  });
});
