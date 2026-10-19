/**
 * spec 36: 読み上げの OpenAI 互換経路を backend 経由に (BackendSynth)。
 *
 * ライブラリは偽装し (アダプタが**作られない**ことも見る)、backend command は mount.ts の
 * IPC 表で偽装する。再生は happy-dom の Audio が `ended` を出さないので、play() を
 * 「次のマイクロタスクで ended を出す」形に差し替える。
 *
 * Red の確かめ方: tts.ts の `toRouteConfig` から `|| "none"` を消すと 1 本目が落ちる /
 * `BackendSynth.speak` の `if (this.stopped) throw` を消すと 2 本目が落ちる (捨てた結果を再生する) /
 * `test()` の `ensureSynth(WARMUP_TIMEOUT_SECS)` を `ensureSynth()` にすると 3 本目が落ちる。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { prepare } from "./test/mount";

const adapters = vi.hoisted(() => [] as string[]);
vi.mock("@aituber-onair/voice", () => ({
  splitSentence: (text: string) => text.split(/(?<=。)/).filter((t) => t.trim()),
  VoiceEngineAdapter: class {
    constructor(o: { speaker: string }) {
      adapters.push(o.speaker);
    }
    updateOptions() {}
    async speakText() {}
    isPlaying() {
      return false;
    }
    stop() {}
  },
}));

import * as tts from "./tts";

const WAV_B64 = btoa("RIFF....WAVEfmt ");
type Call = { config: tts.TtsRouteConfig; text: string };

let revoked: string[] = [];
let played = 0;

function fakeAudioThatEnds(): void {
  // prepare() の spy を上書き: play() は解決し、次のマイクロタスクで ended を出す。
  vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(function (this: HTMLMediaElement) {
    played++;
    queueMicrotask(() => this.dispatchEvent(new Event("ended")));
    return Promise.resolve();
  });
}

beforeEach(() => {
  localStorage.clear();
  adapters.length = 0;
  revoked = [];
  played = 0;
  let n = 0;
  (URL as unknown as { createObjectURL: (b: Blob) => string }).createObjectURL = () => `blob:test-${++n}`;
  (URL as unknown as { revokeObjectURL: (u: string) => void }).revokeObjectURL = (u: string) => {
    revoked.push(u);
  };
  tts.invalidate();
});

afterEach(() => {
  tts.stop();
});

const BASE: tts.TtsSettings = {
  ...tts.DEFAULT_SETTINGS,
  engine: "openaiCompatible",
  model: "irodori-tts",
  speaker: "",
  volume: 0.5,
};

describe("backend 経由の読み上げ (spec 36)", () => {
  it("1 文 1 invoke・voice は空なら none・割り当ては voice 欄で渡る・ライブラリのアダプタは作らない・URL は毎回 revoke", async () => {
    const calls: Call[] = [];
    prepare({
      synthesize_speech: (args) => {
        calls.push(args as unknown as Call);
        return { mime: "audio/wav", data_b64: WAV_B64 };
      },
    });
    fakeAudioThatEnds();
    tts.saveSettings(BASE);
    tts.saveCharacterVoice(BASE, "pkgA", "rinka", "alice");

    await tts.speakSegments(
      [
        { speaker: null, name: "", text: "霧が出た。風が止む。" },
        { speaker: "rinka", name: "凛花", text: "遅いわね" },
      ],
      "pkgA",
    );

    expect(calls.map((c) => [c.text, c.config.voice])).toEqual([
      ["霧が出た。", "none"],
      ["風が止む。", "none"],
      ["遅いわね", "alice"],
    ]);
    expect(calls[0].config).toMatchObject({
      endpoint: "http://localhost:8088/v1/audio/speech",
      model: "irodori-tts",
      speed: 1,
      timeout_secs: null, // 設定 0 = 既定は backend が持つ
    });
    expect(adapters).toEqual([]); // ライブラリの経路は通っていない
    expect(played).toBe(3);
    expect(revoked).toEqual(["blob:test-1", "blob:test-2", "blob:test-3"]);
  });

  it("stop() が invoke の途中に来たら、解決した結果を再生しない (URL も作らない)", async () => {
    let release: ((v: { mime: string; data_b64: string }) => void) | null = null;
    prepare({
      synthesize_speech: () => new Promise((resolve) => (release = resolve)),
    });
    fakeAudioThatEnds();
    tts.saveSettings(BASE);

    const p = tts.speak("長い合成。");
    await vi.waitFor(() => expect(release).not.toBeNull());
    // 合成中に次のターンが来た (stop)。backend の処理は止められないので、結果を捨てる側で守る。
    tts.stop();
    release!({ mime: "audio/wav", data_b64: WAV_B64 });
    await p;
    expect(played).toBe(0);
    expect(revoked).toEqual([]);
  });

  it("試聴はウォームアップを兼ねる: 待ち時間は max(設定, 600)・本番は設定どおり", async () => {
    const calls: Call[] = [];
    prepare({
      synthesize_speech: (args) => {
        calls.push(args as unknown as Call);
        return { mime: "audio/wav", data_b64: WAV_B64 };
      },
    });
    fakeAudioThatEnds();
    tts.saveSettings(BASE);
    await tts.test("試聴です。");
    expect(calls[0].config.timeout_secs).toBe(600);

    tts.saveSettings({ ...BASE, timeoutSecs: 900 });
    await tts.test("試聴です。");
    expect(calls[1].config.timeout_secs).toBe(900);

    await tts.speak("本番。");
    expect(calls[2].config.timeout_secs).toBe(900);
  });

  it("試聴の失敗は 4 種を区別して投げ、本番の失敗は購読者へ渡す (沈黙にしない)", async () => {
    prepare({
      synthesize_speech: () => {
        throw { kind: "http", status: 404, body_head: '{"detail":"Not Found"}' };
      },
    });
    fakeAudioThatEnds();
    tts.saveSettings(BASE);

    await expect(tts.test("x")).rejects.toMatchObject({ failure: { kind: "http", status: 404 } });

    const seen: tts.RouteError[] = [];
    const off = tts.onRouteFailure((e) => seen.push(e));
    await tts.speak("本番。"); // 投げない (プレイを止めない)
    off();
    expect(seen.map((e) => e.failure.kind)).toEqual(["http"]);
    expect(played).toBe(0);
  });
});
