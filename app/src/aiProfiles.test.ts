/**
 * 登録モデル (AiModelProfile) の純粋部のテスト。
 *
 * 2026-09-10 に「思考の深さ」と「出力上限」をモデルごとに持たせた (ユーザー要望
 * 「他のモデルでは LLM_EFFORT を効かせて Opus では効かせない」)。ここで守るのは 2 つ:
 * **前方互換** (欄を持たない古い登録が壊れない) と、**選択表示が嘘をつかない**こと。
 */
import { beforeEach, describe, expect, it } from "vitest";
import { loadAiProfiles, profileMatchesConfig, type AiModelProfile } from "./aiProfiles";

/** node 環境なので localStorage を素朴に立てる (loadAiProfiles はグローバルを直に見る)。 */
function stubStorage(init: Record<string, string> = {}) {
  const map = new Map(Object.entries(init));
  (globalThis as unknown as { localStorage: unknown }).localStorage = {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
  };
}

const profile = (over: Partial<AiModelProfile> = {}): AiModelProfile => ({
  id: "p1",
  name: "Opus",
  model: "claude-opus-5",
  baseUrl: "https://api.anthropic.com",
  apiKey: "k",
  useTools: true,
  effort: "",
  maxTokens: "",
  ...over,
});

const config = (over: Record<string, unknown> = {}) => ({
  base_url: "https://api.anthropic.com",
  model: "claude-opus-5",
  api_key: "k",
  use_tools: true,
  effort: "",
  max_tokens: "",
  ...over,
});

beforeEach(() => stubStorage());

describe("profileMatchesConfig", () => {
  it("その登録が書く欄がすべて一致したときだけ一致とみなす", () => {
    expect(profileMatchesConfig(profile(), config())).toBe(true);
    expect(profileMatchesConfig(profile(), config({ model: "other" }))).toBe(false);
  });

  it("思考の深さだけ違うなら一致ではない (選択表示が嘘をつかないため)", () => {
    // ここを見ないと「Opus を選択中」と見せながら、実際には思考が有効なまま、という
    // 状態を作れてしまう (2026-09-07 に EDITOR_LLM で直したのと同型の嘘)。
    expect(profileMatchesConfig(profile({ effort: "high" }), config())).toBe(false);
    expect(profileMatchesConfig(profile(), config({ effort: "high" }))).toBe(false);
    expect(profileMatchesConfig(profile({ effort: "high" }), config({ effort: "high" }))).toBe(true);
  });

  it("出力上限だけ違うなら一致ではない", () => {
    expect(profileMatchesConfig(profile({ maxTokens: "16000" }), config())).toBe(false);
    expect(
      profileMatchesConfig(profile({ maxTokens: "16000" }), config({ max_tokens: "16000" })),
    ).toBe(true);
  });

  it("前後の空白は無視する (未設定と空白だけを同じに扱う)", () => {
    expect(profileMatchesConfig(profile({ effort: " " }), config({ effort: "" }))).toBe(true);
  });
});

describe("loadAiProfiles", () => {
  it("欄を持たない古い登録は未設定として読む (前方互換)", () => {
    stubStorage({
      "kataribe.aiModelProfiles": JSON.stringify([
        { id: "old", name: "Grok", model: "grok-4.3", baseUrl: "https://api.x.ai/v1", apiKey: "k" },
      ]),
    });
    const [p] = loadAiProfiles();
    expect(p.effort).toBe("");
    expect(p.maxTokens).toBe("");
    expect(p.useTools).toBe(true); // 既存の既定も変えていない
  });

  it("保存された値は読み戻る", () => {
    stubStorage({
      "kataribe.aiModelProfiles": JSON.stringify([
        { id: "n", name: "flash", model: "m", baseUrl: "b", apiKey: "k", effort: "high", maxTokens: "16000" },
      ]),
    });
    const [p] = loadAiProfiles();
    expect(p.effort).toBe("high");
    expect(p.maxTokens).toBe("16000");
  });
});
