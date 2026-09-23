/**
 * 設定 > 開発者 の「Jev による一貫性チェック」(2026-09-23)。鍵を GUI から入れられるようにした。
 * **動くのは開発者モード ON かつ鍵が 2 つとも保存済みのときだけ** (判定は backend の play_turn)。
 * 画面はその状態を 3 通りで言う: 鍵なし / 鍵はあるが開発者モード OFF / 有効。
 *
 * Red の確かめ方: jevState を「欄に何か打っていれば有効」(保存前の値で判定) にすると、保存前に
 * 「有効」と出て落ちる。鍵の保存に開発者モードの ON/OFF を混ぜると set_dev_mode が呼ばれて落ちる。
 */
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import { t } from "../i18n";
import { useGameStore } from "../stores/game";
import { ipcCalls, mountWith, type IpcTable } from "../test/mount";
import SettingsDialog from "./SettingsDialog.vue";

function ipcFor(saved: { account_id: string; api_token: string }, devMode: boolean): IpcTable {
  return {
    get_llm_config: () => ({ base_url: "", model: "", api_key: "", use_tools: true, effort: "", max_tokens: "" }),
    get_default_log_dir: () => "C:\\logs",
    get_default_image_dir: () => "C:\\images",
    get_image_api_keys: () => ({ openai: "", gemini: "" }),
    get_summary_llm_config: () => ({ timeout_secs: 0 }),
    get_recent_turns: () => 0,
    usage_snapshot: () => null,
    get_editor_llm_config: () => ({ base_url: "", model: "", enabled: false }),
    get_dev_mode: () => devMode,
    get_jev_config: () => ({ ...saved }),
    set_jev_config: (a) => {
      saved.account_id = String(a?.accountId ?? "").trim();
      saved.api_token = String(a?.apiToken ?? "").trim();
      return null;
    },
  };
}

async function openDevTab(saved: { account_id: string; api_token: string }, devMode = false): Promise<VueWrapper> {
  const wrapper = mountWith(SettingsDialog, {}, ipcFor(saved, devMode));
  await flushPromises();
  const tab = wrapper.findAll("button").find((b) => b.text() === t("settings.tabs.dev"));
  if (!tab) throw new Error("開発者 のタブが見つからない");
  await tab.trigger("click");
  await flushPromises();
  return wrapper;
}

const stateText = (w: VueWrapper) => w.get('[data-testid="jev-state"]').text();
const inputs = (w: VueWrapper) => w.get('[data-testid="jev-section"]').findAll("input");

describe("Jev による一貫性チェック", () => {
  it("鍵が無ければ無効と出て、打っただけでは有効にならない (保存して初めて変わる)", async () => {
    const w = await openDevTab({ account_id: "", api_token: "" }, true);
    expect(stateText(w)).toBe(t("settings.dev.jevState_noKeys"));

    const [acct, token] = inputs(w);
    await acct.setValue("acct123");
    await token.setValue("tok_abc");
    expect(stateText(w)).toBe(t("settings.dev.jevState_noKeys")); // 保存前は判定を変えない

    const save = w.get('[data-testid="jev-section"]').findAll("button").find((b) => b.text() === t("settings.dev.jevSave"))!;
    await save.trigger("click");
    await flushPromises();
    expect(ipcCalls.some((c) => c.cmd === "set_jev_config")).toBe(true);
    expect(ipcCalls.some((c) => c.cmd === "set_dev_mode")).toBe(false); // 鍵の保存は開発者モードに触らない
    expect(stateText(w)).toBe(t("settings.dev.jevState_on"));
    expect(token.attributes("type")).toBe("password");
  });

  it("鍵が保存済みでも、開発者モードが OFF なら動いていないと言う", async () => {
    const w = await openDevTab({ account_id: "acct", api_token: "tok" }, false);
    const [acct, token] = inputs(w);
    expect((acct.element as HTMLInputElement).value).toBe("acct");
    expect((token.element as HTMLInputElement).value).toBe("tok");
    expect(stateText(w)).toBe(t("settings.dev.jevState_devOff"));

    useGameStore().devMode = true;
    await flushPromises();
    expect(stateText(w)).toBe(t("settings.dev.jevState_on"));
  });
});
