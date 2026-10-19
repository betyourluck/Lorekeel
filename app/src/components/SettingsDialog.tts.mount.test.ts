/**
 * spec 36 Phase C: 設定 > サウンド の OpenAI 互換 (backend 経由) まわり。
 * - 待ち時間の select は OpenAI 互換のときだけ出て、選ぶと設定に保存される (0 = 既定)
 * - 試聴の失敗は backend の 4 種を区別した文面になる (timeout なら秒数と「初回はモデル読み込み」)
 *
 * Red の確かめ方: select の `v-if` を外すと 1 本目が落ちる / testVoice の `describeError(e, t)` を
 * `String(e)` に戻すと 2 本目が落ちる (文面に秒数が出ない)。
 */
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { t } from "../i18n";
import { mountWith, type IpcTable } from "../test/mount";
import * as tts from "../tts";
import SettingsDialog from "./SettingsDialog.vue";

vi.mock("@aituber-onair/voice", () => ({
  splitSentence: (text: string) => [text],
  VoiceEngineAdapter: class {
    updateOptions() {}
    async speakText() {}
    isPlaying() {
      return false;
    }
    stop() {}
  },
}));

function ipc(extra: IpcTable = {}): IpcTable {
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
    ...extra,
  };
}

async function openSoundTab(extra: IpcTable = {}): Promise<VueWrapper> {
  const wrapper = mountWith(SettingsDialog, {}, ipc(extra));
  await flushPromises();
  const tab = wrapper.findAll("button").find((b) => b.text() === t("settings.tabs.sound"));
  if (!tab) throw new Error("サウンド のタブが見つからない");
  await tab.trigger("click");
  await flushPromises();
  return wrapper;
}

beforeEach(() => {
  localStorage.clear();
  tts.saveFeature(true);
});

describe("設定 > サウンド: OpenAI 互換 (backend 経由、spec 36)", () => {
  it("待ち時間の select は OpenAI 互換のときだけ出て、選ぶと設定に保存される", async () => {
    tts.saveSettings({ ...tts.DEFAULT_SETTINGS, engine: "voicevox" });
    let w = await openSoundTab();
    expect(w.find('[data-testid="voice-timeout"]').exists()).toBe(false);
    w.unmount();

    tts.saveSettings({ ...tts.DEFAULT_SETTINGS, engine: "openaiCompatible", model: "irodori-tts" });
    w = await openSoundTab();
    const sel = w.find('[data-testid="voice-timeout"]');
    expect(sel.exists()).toBe(true);
    await sel.setValue("300");
    await flushPromises();
    expect(tts.loadSettings().timeoutSecs).toBe(300);
    await sel.setValue("0");
    await flushPromises();
    expect(tts.loadSettings().timeoutSecs).toBe(0);
  });

  it("試聴の失敗は backend の種別で文面が変わる (timeout は秒数と初回の可能性を言う)", async () => {
    tts.saveSettings({ ...tts.DEFAULT_SETTINGS, engine: "openaiCompatible", model: "irodori-tts" });
    const w = await openSoundTab({
      synthesize_speech: () => {
        throw { kind: "timeout", secs: 600 };
      },
    });
    const btn = w.findAll("button").find((b) => b.text() === t("settings.voice.test"));
    if (!btn) throw new Error("試聴 のボタンが見つからない");
    await btn.trigger("click");
    await flushPromises();
    const expected = t("settings.voice.failed", {
      msg: t("settings.voice.routeTimeout", { secs: 600, warmup: tts.WARMUP_TIMEOUT_SECS }),
    });
    expect(w.text()).toContain(expected);
  });
});
