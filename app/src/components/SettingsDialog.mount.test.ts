/**
 * 設定 > AIモデル の「保存 + 登録モデルを更新」を固定する (spec 31 Phase B)。
 *
 * #103 (2026-09-13): 単価を打って「保存 + 登録モデルを更新」を押すと単価が消えた。saveLlm の末尾の
 * 同期 (syncSelectionToConfig) が**更新前の登録の値**で単価フォームを上書きし、その空を更新処理が
 * 読んでいた。処方は「単価を .env を書く前に取り出す」+「同期は選択が別の登録へ移ったときだけフォームに触る」。
 * Red の確かめ方: syncSelectionToConfig を常に上書きする形に、saveLlmAndProfile を
 * `saveLlm()` (同期あり) で書いてから単価を読む形に戻す (61513cd^ の形) と、フォームも登録簿も空になって落ちる。
 *
 * 残り半分 (2026-09-14): 思考の深さ・出力上限を変えて同じボタンを押すと、登録はまだ古い値を持つので
 * 書いたばかりの .env と一致せず、同期が「一致なし = 選択が変わった」とみなしてフォームを空にした
 * (登録簿には値が残る)。処方は「別の登録に一致したときだけ差し替える」(selectionAfterSave) +
 * 「保存 + 更新は同期を飛ばす」(`saveLlm(false)`)。
 * Red の確かめ方: 61513cd の形 (一致なしを『変わった』とみなす同期 + `saveLlm()`) に戻すと、
 * 登録簿は残るのにフォームが空になって落ちる。
 *
 * 偽装の表は冒頭に全部並べる。部品のローダーは失敗を握り潰すので、表から漏れても画面上は何も
 * 起きない — 漏れは後始末 (mount.ts) が拾って落とす。
 */
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import type { AiModelProfile } from "../aiProfiles";
import { t } from "../i18n";
import { ipcCalls, mountWith, type IpcTable } from "../test/mount";
import { useGameStore } from "../stores/game";
import SettingsDialog from "./SettingsDialog.vue";

/** backend 側の .env の写し。set_llm_config で書き換わり get_llm_config で読める。 */
interface EnvView {
  base_url: string;
  model: string;
  api_key: string;
  use_tools: boolean;
  effort: string;
  max_tokens: string;
}

const OPUS: AiModelProfile = {
  id: "p-opus",
  name: "Opus",
  model: "claude-opus-4-6",
  baseUrl: "https://api.anthropic.com/v1",
  apiKey: "sk-test",
  useTools: true,
  effort: "",
  maxTokens: "",
};

/** 資格情報ストアの写し (spec 34)。登録モデルの鍵は `profile:<id>` ではなく id をそのまま鍵にして持つ。 */
export type Keyring = Record<string, string>;

function ipcFor(env: EnvView, keyring: Keyring = {}): IpcTable {
  return {
    // --- spec 34: 登録モデルの鍵は資格情報ストア ---
    get_profile_secrets: (a) =>
      Object.fromEntries(((a?.ids as string[]) ?? []).filter((id) => id in keyring).map((id) => [id, keyring[id]])),
    set_profile_secrets: (a) => {
      for (const e of (a?.entries as { id: string; key: string }[]) ?? []) {
        if (e.key) keyring[e.id] = e.key;
        else delete keyring[e.id];
      }
      return null;
    },
    secret_store_status: () => ({ fallback_keys: [] }),
    // --- 開いた瞬間に投げるもの (onMounted のローダー群) ---
    get_llm_config: () => ({ ...env }),
    get_default_log_dir: () => "C:\\logs",
    get_default_image_dir: () => "C:\\images",
    get_image_api_keys: () => ({ openai: "", gemini: "" }),
    get_summary_llm_config: () => ({ timeout_secs: 0 }),
    get_recent_turns: () => 0,
    usage_snapshot: () => null,
    get_editor_llm_config: () => ({ base_url: "", model: "", enabled: false }),
    get_dev_mode: () => false,
    get_jev_config: () => ({ account_id: "", api_token: "" }),
    // --- 保存で投げるもの ---
    set_llm_config: (a) => {
      env.base_url = String(a?.baseUrl ?? "");
      env.model = String(a?.model ?? "");
      env.api_key = String(a?.apiKey ?? "");
      env.use_tools = a?.useTools !== false;
      env.effort = String(a?.effort ?? "");
      env.max_tokens = String(a?.maxTokens ?? "");
      return []; // 警告なし
    },
    // 保存後の refreshLlmModel がウィンドウタイトルを書き換える
    "plugin:window|set_title": () => null,
  };
}

/** 登録 1 件と、それに一致する .env を置いて設定を開き、AIモデル タブへ移る。 */
async function openModelTab(): Promise<{ wrapper: VueWrapper; env: EnvView; keyring: Keyring }> {
  // spec 34 の形: localStorage の登録に鍵は無く、鍵は資格情報ストアに居る。開いたときに取り寄せてから
  // 突き合わせないと、鍵が空の登録は .env と一致せず選択が外れる (下の最初の expect が落ちる)。
  localStorage.setItem("kataribe.aiModelProfiles", JSON.stringify([{ ...OPUS, apiKey: "" }]));
  const keyring: Keyring = { [OPUS.id]: OPUS.apiKey };
  const env: EnvView = {
    base_url: OPUS.baseUrl,
    model: OPUS.model,
    api_key: OPUS.apiKey,
    use_tools: true,
    effort: "",
    max_tokens: "",
  };
  const wrapper = mountWith(SettingsDialog, {}, ipcFor(env, keyring));
  await flushPromises();
  const tab = wrapper.findAll("button").find((b) => b.text() === t("settings.tabs.model"));
  if (!tab) throw new Error("AIモデル のタブが見つからない");
  await tab.trigger("click");
  await flushPromises();
  return { wrapper, env, keyring };
}

const pricingInput = (w: VueWrapper, placeholder: string) => w.get(`input[placeholder="${placeholder}"]`);
const effortSelect = (w: VueWrapper) => {
  const s = w.findAll("select").find((x) => x.find('option[value="xhigh"]').exists());
  if (!s) throw new Error("思考の深さの select が見つからない");
  return s;
};

/** 単価 3 欄とコンテキスト長を打ち、「保存 + 登録モデルを更新」を押す。 */
async function typePricingAndSave(w: VueWrapper): Promise<void> {
  await pricingInput(w, "3.00").setValue("3");
  await pricingInput(w, "0.30").setValue("0.3");
  await pricingInput(w, "15.00").setValue("15");
  await pricingInput(w, "200000").setValue("200000");
  const save = w.findAll("button").find((b) => b.text() === t("settings.model.saveWithProfile"));
  if (!save) throw new Error("「保存 + 登録モデルを更新」が見つからない");
  await save.trigger("click");
  await flushPromises();
}

function storedProfile(): AiModelProfile {
  const list = JSON.parse(localStorage.getItem("kataribe.aiModelProfiles") ?? "[]") as AiModelProfile[];
  expect(list).toHaveLength(1);
  return list[0];
}

function expectFormKept(w: VueWrapper): void {
  expect((pricingInput(w, "3.00").element as HTMLInputElement).value).toBe("3");
  expect((pricingInput(w, "0.30").element as HTMLInputElement).value).toBe("0.3");
  expect((pricingInput(w, "15.00").element as HTMLInputElement).value).toBe("15");
  expect((pricingInput(w, "200000").element as HTMLInputElement).value).toBe("200000");
}

describe("保存 + 登録モデルを更新 (#103)", () => {
  it("単価とコンテキスト長が、フォームにも登録簿にも残る", async () => {
    const { wrapper, keyring } = await openModelTab();
    // 開いた時点で .env に一致する登録が選ばれている (更新ボタンが押せる前提)
    const select = wrapper.findAll("select").find((s) => s.find(`option[value="${OPUS.id}"]`).exists());
    expect((select?.element as HTMLSelectElement | undefined)?.value).toBe(OPUS.id);

    await typePricingAndSave(wrapper);

    expect(ipcCalls.filter((c) => c.cmd === "set_llm_config")).toHaveLength(1);
    expectFormKept(wrapper);
    const p = storedProfile();
    expect(p.pricing).toEqual({ inputPerMtokUsd: 3, cacheReadPerMtokUsd: 0.3, outputPerMtokUsd: 15 });
    expect(p.contextTokens).toBe(200000);
    // spec 34: 保存しても localStorage に鍵は書かれず、鍵は資格情報ストアに居る
    expect(p.apiKey).toBe("");
    expect(keyring[OPUS.id]).toBe(OPUS.apiKey);
  });

  it("思考の深さを変えて押しても同じ (残り半分 = 一致が外れる経路)", async () => {
    const { wrapper, env } = await openModelTab();
    await effortSelect(wrapper).setValue("high");

    await typePricingAndSave(wrapper);

    // .env には新しい深さが書かれ、登録も同じ値になる
    expect(env.effort).toBe("high");
    const p = storedProfile();
    expect(p.effort).toBe("high");
    expect(p.pricing).toEqual({ inputPerMtokUsd: 3, cacheReadPerMtokUsd: 0.3, outputPerMtokUsd: 15 });
    expect(p.contextTokens).toBe(200000);
    // 画面が空に戻っていない (実機の報告はここ — 登録簿には残るのに画面が空、次の更新で本当に消える)
    expectFormKept(wrapper);
  });
});

describe("API キーの保存先 (spec 34)", () => {
  it("資格情報ストアが使えなければ鍵ごと localStorage へ退避し、そう言う", async () => {
    const { wrapper } = await openModelTabWith({
      set_profile_secrets: () => {
        throw new Error("unavailable");
      },
    });
    await typePricingAndSave(wrapper);
    expect(storedProfile().apiKey).toBe(OPUS.apiKey); // 鍵を黙って捨てない
    const status = wrapper.get('[data-testid="key-store-status"]').text();
    expect(status).toContain(t("settings.model.keysFallbackProfiles"));
  });

  it("「保存した API キーをすべて削除」は登録の id を渡し、画面の鍵も空にする", async () => {
    const { wrapper } = await openModelTabWith({ delete_all_secrets: () => null });
    await wrapper.get('[data-testid="delete-all-keys"]').trigger("click");
    await flushPromises();
    // 確認ダイアログ (store.askConfirm) を承認する
    const game = useGameStore();
    game.resolveConfirm(true);
    await flushPromises();
    const call = ipcCalls.find((c) => c.cmd === "delete_all_secrets");
    expect(call?.args?.profileIds).toEqual([OPUS.id]);
    expect((wrapper.get('input[type="password"]').element as HTMLInputElement).value).toBe("");
    expect(storedProfile().apiKey).toBe("");
  });
});

/** openModelTab と同じ盤面で、偽装の表の一部を差し替えて開く。 */
async function openModelTabWith(extra: IpcTable): Promise<{ wrapper: VueWrapper }> {
  localStorage.setItem("kataribe.aiModelProfiles", JSON.stringify([{ ...OPUS, apiKey: "" }]));
  const env: EnvView = {
    base_url: OPUS.baseUrl,
    model: OPUS.model,
    api_key: OPUS.apiKey,
    use_tools: true,
    effort: "",
    max_tokens: "",
  };
  const wrapper = mountWith(SettingsDialog, {}, { ...ipcFor(env, { [OPUS.id]: OPUS.apiKey }), ...extra });
  await flushPromises();
  const tab = wrapper.findAll("button").find((b) => b.text() === t("settings.tabs.model"));
  if (!tab) throw new Error("AIモデル のタブが見つからない");
  await tab.trigger("click");
  await flushPromises();
  return { wrapper };
}
