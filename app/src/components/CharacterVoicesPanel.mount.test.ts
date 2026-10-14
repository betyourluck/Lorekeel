/**
 * spec 35 Phase D: キャラクターの声のパネル (会話ペイン右下の読み上げの操作から開く)。
 *
 * 一覧はいま遊んでいるパッケージの登場人物 (GameView.cast)。選んだ声はエンジン + URL と
 * パッケージの鍵で保存され (`kataribe.tts.characterVoices`)、声一覧のキャッシュ
 * (`kataribe.tts.voices`) は壊さない。読み上げの操作のボタンで開閉し、画像側の浮遊パネルとは
 * 排他 (同じ位置に出るので、2 枚開くと 1 枚しか見えない)。設定 > サウンドからはキャラの声の
 * 行を外し、場所を案内する一文だけを残した (ユーザーFB 2026-10-15)。
 *
 * Red の確かめ方: CharacterVoicesPanel.vue の setVoice で saveCharacterVoice を呼ばないと
 * 「選んだ声が保存される」が落ちる。TtsControls のボタンが store の floatingPanel を使わず
 * ローカルの状態で開くと「画像側のパネルとは排他」が落ちる。
 */
import { flushPromises } from "@vue/test-utils";
import { beforeEach, describe, expect, it } from "vitest";

import { t } from "../i18n";
import { useGameStore } from "../stores/game";
import { mountWith, type IpcTable } from "../test/mount";
import * as tts from "../tts";
import CharacterVoicesPanel from "./CharacterVoicesPanel.vue";
import SettingsDialog from "./SettingsDialog.vue";
import TtsControls from "./TtsControls.vue";

const SETTINGS: tts.TtsSettings = { ...tts.DEFAULT_SETTINGS, engine: "voicevox", speaker: "1" };
const PKG = "D:\\pkgs\\friday_lemmon";

beforeEach(() => {
  localStorage.clear();
  tts.saveSettings(SETTINGS);
  tts.saveVoiceList(SETTINGS, [
    { id: "1", label: "語り手の声" },
    { id: "7", label: "つんけんした声" },
  ]);
});

function withCast(game: ReturnType<typeof useGameStore>) {
  game.activePackagePath = PKG;
  game.cast = [
    { id: "akari", name: "岬あかり", icon: null },
    { id: "genzo", name: "源蔵", icon: null },
  ];
}

describe("キャラクターの声のパネル (spec 35 Phase D)", () => {
  it("登場人物ごとに声を選ぶと、そのエンジンとパッケージの鍵で保存され、開き直しても残る", async () => {
    let w = mountWith(CharacterVoicesPanel, {}, {});
    withCast(useGameStore());
    await flushPromises();
    const rows = w.findAll('[data-testid="character-voice-row"]');
    expect(rows.map((r) => r.text())).toEqual([expect.stringContaining("岬あかり"), expect.stringContaining("源蔵")]);
    expect(w.text()).toContain(t("settings.voice.engineVoicevox")); // どのエンジンの割り当てかを見せる

    await rows[0].get("select").setValue("7");
    await flushPromises();
    expect(tts.loadCharacterVoices(SETTINGS, PKG)).toEqual({ akari: "7" });
    expect(tts.loadVoiceList(SETTINGS).map((v) => v.id)).toEqual(["1", "7"]); // キャッシュは壊さない

    w.unmount();
    w = mountWith(CharacterVoicesPanel, {}, {});
    withCast(useGameStore());
    await flushPromises();
    const select = w.findAll('[data-testid="character-voice-row"]')[0].get("select").element as HTMLSelectElement;
    expect(select.value).toBe("7");
  });

  it("ゲーム前は登場人物が居ないことを言う", async () => {
    const w = mountWith(CharacterVoicesPanel, {}, {});
    useGameStore().cast = [];
    await flushPromises();
    expect(w.findAll('[data-testid="character-voice-row"]')).toHaveLength(0);
    expect(w.get('[data-testid="character-voices"]').text()).toContain(t("settings.voice.charactersEmpty"));
  });

  it("読み上げの操作のボタンで開閉し、画像側の浮遊パネルとは排他", async () => {
    const w = mountWith(TtsControls, {}, {});
    const game = useGameStore();
    withCast(game);
    await flushPromises();
    const button = w.get('[data-testid="voices-button"]');

    await button.trigger("click");
    await flushPromises();
    expect(game.floatingPanel).toBe("voices");
    expect(w.find('[data-testid="character-voices"]').exists()).toBe(true);

    // 画像側のパネル (参照ストック) を開けば、こちらは閉じる (同じ位置に 2 枚出さない)。
    game.toggleFloatingPanel("refStock");
    await flushPromises();
    expect(w.find('[data-testid="character-voices"]').exists()).toBe(false);

    // 押し直しで閉じる。
    await button.trigger("click");
    await flushPromises();
    expect(game.floatingPanel).toBe("voices");
    await button.trigger("click");
    await flushPromises();
    expect(game.floatingPanel).toBe(null);
  });
});

const settingsIpc: IpcTable = {
  get_llm_config: () => ({ base_url: "", model: "", api_key: "", use_tools: true, effort: "", max_tokens: "" }),
  get_default_log_dir: () => "C:\\logs",
  get_default_image_dir: () => "C:\\images",
  get_image_api_keys: () => ({ openai: "", gemini: "" }),
  get_summary_llm_config: () => ({ timeout_secs: 0 }),
  get_recent_turns: () => 0,
  usage_snapshot: () => null,
  get_editor_llm_config: () => ({ base_url: "", model: "", enabled: false }),
  get_dev_mode: () => false,
  get_jev_config: () => ({ account_id: "", api_token: "" }),
  secret_store_status: () => ({ fallback_keys: [] }),
};

describe("設定 > サウンド (spec 35 Phase D)", () => {
  it("読み上げる範囲は設定に保存され、キャラの声は場所の案内だけ (行は置かない)", async () => {
    const w = mountWith(SettingsDialog, {}, settingsIpc);
    withCast(useGameStore());
    await flushPromises();
    const tab = w.findAll("button").find((b) => b.text() === t("settings.tabs.sound"));
    if (!tab) throw new Error("サウンドのタブが見つからない");
    await tab.trigger("click");
    await flushPromises();

    expect(w.findAll('[data-testid="character-voice-row"]')).toHaveLength(0);
    expect(w.get('[data-testid="character-voices-moved"]').text()).toBe(t("settings.voice.charactersMoved"));

    await w.get('[data-testid="voice-scope"]').setValue("dialogue");
    await flushPromises();
    expect(tts.loadSettings().scope).toBe("dialogue");
  });
});
