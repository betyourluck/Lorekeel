/**
 * spec 36 Phase C: 読み上げの操作列の「合成中」。backend の応答を待っている間だけ出る
 * (サーバーの直列待ちを「壊れた」と読ませない)。store の橋渡し (initTtsBridge) まで含めて見る。
 *
 * Red の確かめ方: TtsControls の `v-if="game.ttsBusy"` を外すと 1 本目が落ちる /
 * store の initTtsBridge から onSynthesizing の購読を消すと 2 本目が落ちる。
 */
import { flushPromises } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";

import { useGameStore } from "../stores/game";
import { mountWith } from "../test/mount";
import * as tts from "../tts";
import TtsControls from "./TtsControls.vue";

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

describe("読み上げの操作列: 合成中 (spec 36)", () => {
  it("store の ttsBusy で「合成中」が出入りする", async () => {
    const w = mountWith(TtsControls);
    const game = useGameStore();
    expect(w.find('[data-testid="tts-busy"]').exists()).toBe(false);
    game.ttsBusy = true;
    await flushPromises();
    expect(w.find('[data-testid="tts-busy"]').exists()).toBe(true);
    game.ttsBusy = false;
    await flushPromises();
    expect(w.find('[data-testid="tts-busy"]').exists()).toBe(false);
  });

  it("initTtsBridge が backend の合成中を store へ、失敗をトーストへ橋渡しする", async () => {
    let release: ((v: { mime: string; data_b64: string }) => void) | null = null;
    const w = mountWith(TtsControls, {}, {
      synthesize_speech: () => new Promise((resolve) => (release = resolve)),
    });
    const game = useGameStore();
    const off = game.initTtsBridge();
    localStorage.clear();
    tts.saveSettings({ ...tts.DEFAULT_SETTINGS, engine: "openaiCompatible", model: "irodori-tts" });
    (URL as unknown as { createObjectURL: (b: Blob) => string }).createObjectURL = () => "blob:x";
    (URL as unknown as { revokeObjectURL: (u: string) => void }).revokeObjectURL = () => {};
    vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(function (this: HTMLMediaElement) {
      queueMicrotask(() => this.dispatchEvent(new Event("ended")));
      return Promise.resolve();
    });

    const p = tts.speak("合成する。");
    await vi.waitFor(() => expect(release).not.toBeNull());
    await flushPromises();
    expect(game.ttsBusy).toBe(true);
    expect(w.find('[data-testid="tts-busy"]').exists()).toBe(true);
    release!({ mime: "audio/wav", data_b64: btoa("RIFF") });
    await p;
    await flushPromises();
    expect(game.ttsBusy).toBe(false);

    // 失敗はトースト (沈黙にしない)。
    const w2 = mountWith(TtsControls, {}, {
      synthesize_speech: () => {
        throw { kind: "network", detail: "connection refused" };
      },
    });
    const game2 = useGameStore();
    off();
    const off2 = game2.initTtsBridge();
    await tts.speak("失敗する。");
    expect(game2.logToast).toContain("connection refused");
    off2();
    w.unmount();
    w2.unmount();
  });
});
