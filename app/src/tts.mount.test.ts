/**
 * spec 35 Phase D: 読み上げの鎖の中で、段ごとに声を切り替える順序。
 *
 * ライブラリは合成のたびに options.speaker を読むので、鎖の中で文ごとに
 * updateOptions({ speaker }) を呼べば、読み上げ器を声の数だけ作らずに済む。
 * ここではライブラリを偽装し、「声 → 文」の順に呼ばれることと、範囲が台詞だけなら
 * 地の文 (エピローグを含む) を読まないことを見る。
 *
 * Red の確かめ方: tts.ts の speakParts から updateOptions の呼び出しを消すと、
 * 「凛花の台詞の前に声が 7 に変わる」が落ちる (全部が語り手の声で読まれる)。
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const calls = vi.hoisted(() => [] as Array<[string, string]>);

vi.mock("@aituber-onair/voice", () => ({
  splitSentence: (text: string) => [text],
  VoiceEngineAdapter: class {
    constructor(o: { speaker: string }) {
      calls.push(["new", o.speaker]);
    }
    updateOptions(o: { speaker: string }) {
      calls.push(["voice", o.speaker]);
    }
    async speakText(text: string) {
      calls.push(["say", text]);
    }
    isPlaying() {
      return false;
    }
    stop() {}
  },
}));

import * as tts from "./tts";

const SEGS: tts.SpokenSegment[] = [
  { speaker: null, name: "", text: "教室が静まる。" },
  { speaker: "rinka", name: "凛花", text: "遅いわね" },
  { speaker: "yukina", name: "雪菜", text: "……" },
  { speaker: null, name: "店員", text: "毎度" },
];

beforeEach(() => {
  localStorage.clear();
  tts.invalidate();
  calls.length = 0;
});

describe("キャラ別の声の読み上げ (spec 35 Phase D)", () => {
  it("割り当てのある台詞の前で声を変え、次の段で語り手の声へ戻す", async () => {
    const s = { ...tts.DEFAULT_SETTINGS, engine: "voicevox" as const, speaker: "1" };
    tts.saveSettings(s);
    tts.saveCharacterVoice(s, "pkgA", "rinka", "7");

    await tts.speakSegments(SEGS, "pkgA");
    expect(calls).toEqual([
      ["new", "1"],
      ["say", "教室が静まる。"],
      ["voice", "7"],
      ["say", "遅いわね"],
      ["voice", "1"], // 割り当ての無い雪菜は語り手の声
      ["say", "……"],
      ["say", "毎度"], // モブは地の文 = 語り手の声 (同じ声なら呼び直さない)
    ]);
  });

  it("割り当てはパッケージごと — 別のパッケージでは全員が語り手の声", async () => {
    const s = { ...tts.DEFAULT_SETTINGS, engine: "voicevox" as const, speaker: "1" };
    tts.saveSettings(s);
    tts.saveCharacterVoice(s, "pkgA", "rinka", "7");

    await tts.speakSegments(SEGS, "pkgB");
    expect(calls.filter(([k]) => k === "voice")).toEqual([]);
  });

  it("台詞だけ: 地の文・モブの台詞・エピローグ (平らな文字列) は読まない", async () => {
    const s = { ...tts.DEFAULT_SETTINGS, engine: "voicevox" as const, speaker: "1", scope: "dialogue" as const };
    tts.saveSettings(s);

    await tts.speakSegments(SEGS, "pkgA");
    await tts.speak("そして二人は、湖のほとりで別れた。", { queue: true });
    expect(calls.filter(([k]) => k === "say").map(([, t]) => t)).toEqual(["遅いわね", "……"]);
  });
});
