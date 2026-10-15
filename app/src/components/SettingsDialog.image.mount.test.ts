/**
 * 設定 > 画像生成 で Meta (muse-image-1.0、2026-10-05) を選べること。
 * - プロバイダの選択肢に Meta があり、選ぶと API キー欄が出て `set_image_api_key` に provider meta で渡る
 *   (鍵の保存先は backend の `IMAGE_API_KEY_META` = 資格情報ストアの対象)
 * - 解像度段は Meta では効かない (約 2.5MP 固定・quality は無視される) ので選択を無効にする
 *
 * Red の確かめ方: 選択肢から Meta を外すと選べず落ちる / 解像度の `:disabled` を外すと落ちる。
 */
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import { t } from "../i18n";
import { currentSlot, loadImageGenSettings, toBackendConfig, withWorkflowText } from "../imageGen";
import { useGameStore } from "../stores/game";
import { ipcCalls, mountWith, type IpcTable } from "../test/mount";
import SettingsDialog from "./SettingsDialog.vue";

function ipc(keys: Record<string, string>): IpcTable {
  return {
    get_llm_config: () => ({ base_url: "", model: "", api_key: "", use_tools: true, effort: "", max_tokens: "" }),
    get_default_log_dir: () => "C:\\logs",
    get_default_image_dir: () => "C:\\images",
    get_image_api_keys: () => ({ openai: "", gemini: "", meta: "", xai: "" }),
    get_summary_llm_config: () => ({ timeout_secs: 0 }),
    get_recent_turns: () => 0,
    usage_snapshot: () => null,
    get_editor_llm_config: () => ({ base_url: "", model: "", enabled: false }),
    get_dev_mode: () => false,
    get_jev_config: () => ({ account_id: "", api_token: "" }),
    secret_store_status: () => ({ fallback_keys: [] }),
    set_image_api_key: (a) => {
      keys[String(a?.provider)] = String(a?.apiKey ?? "");
      return null;
    },
  };
}

async function openImageTab(keys: Record<string, string>): Promise<VueWrapper> {
  const wrapper = mountWith(SettingsDialog, {}, ipc(keys));
  await flushPromises();
  const tab = wrapper.findAll("button").find((b) => b.text() === t("settings.tabs.image"));
  if (!tab) throw new Error("画像生成 のタブが見つからない");
  await tab.trigger("click");
  await flushPromises();
  return wrapper;
}

/** ラベル文言で select を引く (並び順に依存しない)。 */
function selectBy(w: VueWrapper, label: string) {
  const l = w.findAll("label").find((x) => x.text().startsWith(label));
  if (!l) throw new Error(`${label} の欄が無い`);
  return l.get("select");
}

describe("画像生成のプロバイダ Meta", () => {
  it("Meta を選ぶと鍵の欄が出て Meta 用に保存され、解像度は選べない", async () => {
    const keys: Record<string, string> = {};
    const w = await openImageTab(keys);

    const provider = selectBy(w, t("settings.image.provider"));
    expect(provider.findAll("option").map((o) => o.attributes("value"))).toContain("meta");
    await provider.setValue("meta");
    await flushPromises();

    expect(selectBy(w, t("settings.image.detail")).attributes("disabled")).toBeDefined();
    const passwords = w.findAll('input[type="password"]');
    const key = passwords[passwords.length - 1];
    if (!key) throw new Error("API キー欄が無い");
    await key.setValue("meta-key");
    await key.trigger("change");
    await flushPromises();
    expect(keys).toEqual({ meta: "meta-key" });
    expect(ipcCalls.some((c) => c.cmd === "set_image_api_key")).toBe(true);

    // OpenAI に戻すと解像度は選べる (効くプロバイダでは無効にしない)。
    await provider.setValue("openai");
    await flushPromises();
    expect(selectBy(w, t("settings.image.detail")).attributes("disabled")).toBeUndefined();
  });
});

/** 解像度の段の選択肢 (value) を並べる。 */
function detailOptions(w: VueWrapper): (string | undefined)[] {
  return selectBy(w, t("settings.image.detail"))
    .findAll("option")
    .map((o) => o.attributes("value"));
}

describe("画像生成のプロバイダ xAI", () => {
  it("xAI を選ぶと鍵の欄が出て xAI 用に保存され、解像度は「最高」まで選べる", async () => {
    const keys: Record<string, string> = {};
    const w = await openImageTab(keys);

    const provider = selectBy(w, t("settings.image.provider"));
    expect(provider.findAll("option").map((o) => o.attributes("value"))).toContain("xai");
    await provider.setValue("xai");
    await flushPromises();

    expect(selectBy(w, t("settings.image.detail")).attributes("disabled")).toBeUndefined();
    expect(detailOptions(w)).toEqual(["standard", "high", "highest"]);
    const all = w.findAll('input[type="password"]');
    const key = all[all.length - 1];
    if (!key) throw new Error("API キー欄が無い");
    await key.setValue("xai-key");
    await key.trigger("change");
    await flushPromises();
    expect(keys).toEqual({ xai: "xai-key" });

    // Gemini では「最高」は「高」と同じ 2K になるので出さない。
    await provider.setValue("gemini");
    await flushPromises();
    expect(detailOptions(w)).toEqual(["standard", "high"]);
  });
});

/**
 * ComfyUI のワークフローを 3 つまで保管し、選んだものを生成に使う (2026-10-15 ユーザー要望)。
 *
 * Red の確かめ方: SettingsDialog の保管枠ボタンが withWorkflowIndex を通さず workflowIndex だけを
 * 変えると、backend へ送るワークフローが切り替わらず落ちる。
 */
describe("ComfyUI のワークフローの保管枠", () => {
  it("枠を切り替えると生成に使うワークフローが替わり、戻すと元の本文と名前が残っている", async () => {
    const w = await openImageTab({});
    const game = useGameStore();
    await selectBy(w, t("settings.image.provider")).setValue("comfy");
    await flushPromises();
    game.setImageGenSlot(withWorkflowText(currentSlot(game.imageGen), '{"a":1}'));
    await w.get('[data-testid="workflow-name"]').setValue("SDXL");
    await w.get('[data-testid="workflow-name"]').trigger("change");
    await flushPromises();

    const slots = () => w.findAll('[data-testid="workflow-slot"]');
    expect(slots().map((b) => b.text())).toEqual([
      "SDXL",
      t("settings.image.workflowSlotEmpty", { n: 2 }),
      t("settings.image.workflowSlotEmpty", { n: 3 }),
    ]);

    await slots()[1].trigger("click");
    await flushPromises();
    expect(toBackendConfig(game.imageGen).workflow_json).toBeNull(); // 空の枠
    game.setImageGenSlot(withWorkflowText(currentSlot(game.imageGen), '{"b":2}'));
    await flushPromises();
    expect(toBackendConfig(game.imageGen).workflow_json).toBe('{"b":2}');

    await slots()[0].trigger("click");
    await flushPromises();
    expect(toBackendConfig(game.imageGen).workflow_json).toBe('{"a":1}');
    expect(slots().map((b) => b.text())).toEqual(["SDXL", "2", t("settings.image.workflowSlotEmpty", { n: 3 })]);
    // 再読み込みしても残る (localStorage)。
    expect(currentSlot(loadImageGenSettings()).workflowSlots).toEqual(['{"a":1}', '{"b":2}', ""]);
  });
});
