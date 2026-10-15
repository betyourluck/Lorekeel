/**
 * 読み上げのスキップと、セリフの読み直し (2026-10-15 ユーザー要望)。
 *
 * - スキップは**いま読んでいるセリフ (段)** だけを飛ばし、次のセリフから続ける。従来は
 *   ターンの読み上げ全体を捨てていた。同じ段の残りの文は読まない。
 * - 再生中に止めたアダプタは二度と喋らない (failures #72) ので、スキップの後は新しい
 *   アダプタで続きを読む。ライブラリの stop() は再生中の発話も reject するので、鎖は止まらない。
 * - 吹き出しの読み上げボタンは、範囲 (台詞だけ) と「なし」を見ずにその一言を読む。
 *
 * Red の確かめ方: tts.ts の skip() を stop() と同じ (世代を進める) にすると「次のセリフから続ける」が
 * 落ちる。鎖の中で ensureAdapter を取り直さず最初のアダプタを使い続けると、捨てたアダプタで
 * 読もうとして「新しいアダプタで続ける」が落ちる。
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const log = vi.hoisted(() => [] as string[]);
const gate = vi.hoisted(() => ({ hold: "", started: null as null | (() => void), n: 0 }));

vi.mock("@aituber-onair/voice", () => {
  return {
    // 「。」で文に割る (同じ段の残りの文が飛ばされることを見るため)。
    splitSentence: (text: string) => text.split(/(?<=。)/).filter((s) => s.trim()),
    VoiceEngineAdapter: class {
      id = ++gate.n;
      playing = false;
      reject: ((e: Error) => void) | null = null;
      constructor(o: { speaker: string }) {
        log.push(`new#${this.id}:${o.speaker}`);
      }
      updateOptions(o: { speaker: string }) {
        log.push(`voice#${this.id}:${o.speaker}`);
      }
      speakText(text: string): Promise<void> {
        log.push(`say#${this.id}:${text}`);
        if (text !== gate.hold) return Promise.resolve();
        // この文は stop() されるまで鳴り続ける (再生中)。
        this.playing = true;
        return new Promise<void>((_, reject) => {
          this.reject = reject;
          gate.started?.();
        });
      }
      isPlaying() {
        return this.playing;
      }
      stop() {
        this.playing = false;
        this.reject?.(new Error("Speech playback stopped"));
        this.reject = null;
      }
    },
  };
});

import * as tts from "./tts";

const S = { ...tts.DEFAULT_SETTINGS, engine: "voicevox" as const, speaker: "1" };

beforeEach(() => {
  localStorage.clear();
  tts.invalidate();
  log.length = 0;
  gate.hold = "";
  gate.started = null;
  tts.saveSettings(S); // invalidate() も呼ぶので、採番はその後に戻す
  gate.n = 0;
});

describe("スキップは次のセリフへ", () => {
  it("読んでいる段の残りを飛ばし、次の段から新しいアダプタで続ける", async () => {
    tts.saveCharacterVoice(S, "pkg", "rinka", "7");
    gate.hold = "遅いわね。";
    const started = new Promise<void>((r) => (gate.started = r));
    const done = tts.speakSegments(
      [
        { speaker: null, name: "", text: "教室が静まる。" },
        { speaker: "rinka", name: "凛花", text: "遅いわね。待ってたのよ。" },
        { speaker: null, name: "", text: "扉が閉まる。" },
      ],
      "pkg",
    );
    await started;
    tts.skip();
    await done;
    expect(log).toEqual([
      "new#1:1",
      "say#1:教室が静まる。",
      "voice#1:7",
      "say#1:遅いわね。", // ← ここでスキップ。「待ってたのよ。」は読まない
      "new#2:1", // 再生中に止めたアダプタは捨て、組み直す (failures #72)
      "say#2:扉が閉まる。",
    ]);
  });

  it("stop() はターンの残り全部を捨てる (OFF・次のターン)", async () => {
    gate.hold = "一。";
    const started = new Promise<void>((r) => (gate.started = r));
    const done = tts.speakSegments(
      [
        { speaker: null, name: "", text: "一。" },
        { speaker: null, name: "", text: "二。" },
      ],
      "pkg",
    );
    await started;
    tts.stop();
    await done;
    expect(log.filter((l) => l.startsWith("say"))).toEqual(["say#1:一。"]);
  });
});

describe("セリフの読み直し", () => {
  it("範囲が台詞だけでも、声を『なし』にした人物でも、押したセリフは語り手の声で読む", async () => {
    tts.saveSettings({ ...S, scope: "dialogue" });
    const s = tts.loadSettings();
    tts.saveCharacterVoice(s, "pkg", "rinka", tts.VOICE_NONE);
    tts.saveCharacterVoice(s, "pkg", "yukina", "9");

    await tts.replaySegment({ speaker: "rinka", name: "凛花", text: "遅いわね" }, "pkg");
    await tts.replaySegment({ speaker: "yukina", name: "雪菜", text: "……" }, "pkg");
    expect(log.filter((l) => !l.startsWith("new"))).toEqual([
      "say#1:遅いわね",
      "voice#1:9",
      "say#1:……",
    ]);
  });
});
