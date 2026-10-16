/**
 * spec 35 Phase D: キャラ別の声の純関数。段から「何をどの声で読むか」と、割り当ての保存形。
 * (読み上げの鎖の中で声を切り替える順序は tts.mount.test.ts が見る。)
 */
import { describe, expect, it } from "vitest";

import { DEFAULT_SETTINGS, planSegments, VOICE_NONE, voiceEngineKey, withCharacterVoice, type SpokenSegment } from "./tts";

const SEGS: SpokenSegment[] = [
  { speaker: null, name: "", text: "教室が静まる。" },
  { speaker: "rinka", name: "凛花", text: "遅いわね" },
  { speaker: "yukina", name: "雪菜", text: "……" },
  { speaker: null, name: "店員", text: "毎度" },
  { speaker: null, name: "", text: "   " },
];

describe("planSegments", () => {
  it("すべて: 地の文・割り当ての無い人物・モブは語り手の声、割り当てのある人物はその声", () => {
    expect(planSegments(SEGS, "all", { rinka: "7" })).toEqual([
      { text: "教室が静まる。", voice: null },
      { text: "遅いわね", voice: "7" },
      { text: "……", voice: null },
      { text: "毎度", voice: null },
    ]);
  });

  it("声を『なし』にした人物の台詞は読まない (地の文と他の人物は読む)", () => {
    expect(planSegments(SEGS, "all", { rinka: VOICE_NONE, yukina: "3" })).toEqual([
      { text: "教室が静まる。", voice: null },
      { text: "……", voice: "3" },
      { text: "毎度", voice: null },
    ]);
  });

  it("主人公の台詞は読まない (吹き出しは出すが声は付けない = 2026-10-16)", () => {
    const withPlayer: SpokenSegment[] = [...SEGS, { speaker: "player", name: "ゆうじ", text: "行くぞ" }];
    expect(planSegments(withPlayer, "all", { player: "7" }).map((p) => p.text)).not.toContain("行くぞ");
    expect(planSegments(withPlayer, "dialogue", {}).map((p) => p.text)).not.toContain("行くぞ");
  });

  it("台詞だけ: 地の文を読まない。モブ (登場人物にいない人物) の台詞も地の文として読まない", () => {
    expect(planSegments(SEGS, "dialogue", { rinka: "7" }).map((p) => p.text)).toEqual(["遅いわね", "……"]);
  });
});

describe("キャラ別の声の保存形", () => {
  it("エンジン + URL とパッケージで分け、空の話者 ID は割り当てを消して箱を畳む", () => {
    const vv = voiceEngineKey({ ...DEFAULT_SETTINGS, engine: "voicevox" });
    expect(vv).toBe("voicevox|http://localhost:50021"); // URL 未入力なら既定値

    let m = withCharacterVoice({}, vv, "pkgA", "rinka", "7");
    m = withCharacterVoice(m, vv, "pkgB", "rinka", "3");
    m = withCharacterVoice(m, "aivisSpeech|http://localhost:10101", "pkgA", "rinka", "abc");
    expect(m[vv]).toEqual({ pkgA: { rinka: "7" }, pkgB: { rinka: "3" } });

    m = withCharacterVoice(m, vv, "pkgA", "rinka", "");
    expect(m[vv]).toEqual({ pkgB: { rinka: "3" } }); // 空になったパッケージの箱は畳む
    m = withCharacterVoice(m, vv, "pkgB", "rinka", "  ");
    expect(m[vv]).toBeUndefined(); // エンジンの箱も畳む
    expect(m["aivisSpeech|http://localhost:10101"]).toEqual({ pkgA: { rinka: "abc" } });
  });

  it("元の割り当てを書き換えない (純関数)", () => {
    const before = { k: { p: { a: "1" } } };
    withCharacterVoice(before, "k", "p", "a", "2");
    expect(before).toEqual({ k: { p: { a: "1" } } });
  });
});
