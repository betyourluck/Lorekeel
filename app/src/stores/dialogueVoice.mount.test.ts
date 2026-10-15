/**
 * spec 35 Phase D: 受理ターンの読み上げは、段があれば speakSegments (キャラ別の声) へ、
 * いま遊んでいるパッケージの鍵つきで渡る。段が無い語りは従来どおり speak。
 *
 * Red の確かめ方: stores/game.ts の ingestTurn で speakSegments の代わりに
 * speak(turn.narration) を呼ぶと「段があれば speakSegments」が落ちる。
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const spoken = vi.hoisted(() => ({ segments: [] as unknown[], plain: [] as string[] }));

vi.mock("../tts", async (importOriginal) => {
  const real = await importOriginal<typeof import("../tts")>();
  return {
    ...real,
    speakSegments: vi.fn(async (segs: unknown, pkg: string) => {
      spoken.segments.push({ segs, pkg });
    }),
    speak: vi.fn(async (text: string) => {
      spoken.plain.push(text);
    }),
  };
});

import { prepare } from "../test/mount";
import type { TurnView } from "../types/api";
import { useGameStore } from "./game";

function turn(segments: TurnView["segments"]): TurnView {
  return {
    accepted: true,
    narration: "教室が静まる。\n凛花「遅いわね」",
    segments,
    rolls: [],
    checks: [],
    stat_rolls: [],
    beats: [],
    attempts: 1,
    retries: [],
    reasons: [],
    state: null as unknown as TurnView["state"],
    background: null,
    bgm: null,
    present_characters: [],
    goal_reached: false,
    goal_id: null,
    goal_title: null,
    goal_narration: null,
    transition: null,
    cache: {} as TurnView["cache"],
    new_synopsis: [],
    new_log: [],
    epilogue: null,
    map: { nodes: [], edges: [] },
    decision: null,
    contest: null,
    consistency: null,
    facts: null,
    facts_policy: "locked",
  } as unknown as TurnView;
}

beforeEach(() => {
  spoken.segments.length = 0;
  spoken.plain.length = 0;
});

describe("受理ターンの読み上げ (spec 35 Phase D)", () => {
  it("段があれば speakSegments へ、いま遊んでいるパッケージの鍵つきで渡る", async () => {
    const store = useGameStore(prepare({ resolve_asset_path: () => null }));
    store.ttsFeature = true;
    store.ttsEnabled = true;
    store.activePackagePath = "D:\\pkgs\\friday_lemmon";
    const segs = [
      { speaker: null, name: "", icon: null, text: "教室が静まる。" },
      { speaker: "rinka", name: "凛花", icon: null, text: "遅いわね" },
    ];
    await store.ingestTurn(turn(segs));

    expect(spoken.segments).toEqual([{ segs, pkg: "D:\\pkgs\\friday_lemmon" }]);
    expect(spoken.plain).toEqual([]);
  });

  it("段が無い語りは従来どおり speak", async () => {
    const store = useGameStore(prepare({ resolve_asset_path: () => null }));
    store.ttsFeature = true;
    store.ttsEnabled = true;
    await store.ingestTurn(turn([]));
    expect(spoken.plain).toEqual(["教室が静まる。\n凛花「遅いわね」"]);
    expect(spoken.segments).toEqual([]);
  });
});

/**
 * モブ (登場人物にいない人物) の台詞は地の文として読む (2026-10-15 ユーザー要望)。
 *
 * Red の確かめ方: ingestTurn で foldMobLines を通さず turn.segments をそのまま渡すと落ちる。
 */
describe("モブの台詞の読み上げ", () => {
  it("モブの台詞は 名前「…」 の地の文に畳んでから speakSegments へ渡る", async () => {
    const store = useGameStore(prepare({ resolve_asset_path: () => null }));
    store.ttsFeature = true;
    store.ttsEnabled = true;
    store.activePackagePath = "pkg";
    await store.ingestTurn(
      turn([
        { speaker: "rinka", name: "凛花", icon: null, text: "遅いわね" },
        { speaker: null, name: "店員", icon: null, text: "毎度" },
      ]),
    );
    expect(spoken.segments).toEqual([
      {
        segs: [
          { speaker: "rinka", name: "凛花", icon: null, text: "遅いわね" },
          { speaker: null, name: "", icon: null, text: "店員「毎度」" },
        ],
        pkg: "pkg",
      },
    ]);
  });
});
