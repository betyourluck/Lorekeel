/**
 * spec 35 Phase C: ターンの段 (TurnView.segments) が会話ログへ届き、台詞の顔アイコンは
 * アセット ID から URL へ解決されている (DTO と会話ログの継ぎ目)。
 *
 * Red の確かめ方: stores/game.ts の ingestTurn で narration を積むときに `segments` を
 * 渡さないと「会話ログに段が届く」が落ちる。先読みのリストから段のアイコンを外すと
 * 「アイコンが URL になる」が落ちる (先読みしていない ID は assetUrl が null を返す)。
 */
import { describe, expect, it } from "vitest";

import { prepare } from "../test/mount";
import type { TurnView } from "../types/api";
import { useGameStore } from "./game";

function turn(): TurnView {
  return {
    accepted: true,
    narration: "教室が静まる。\n凛花「遅いわね」",
    segments: [
      { speaker: null, name: "", icon: null, text: "教室が静まる。" },
      { speaker: "rinka", name: "凛花", icon: "rinka.webp", text: "遅いわね" },
    ],
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

describe("台詞の段が会話ログへ届く (spec 35)", () => {
  it("段は語りの項目に載り、台詞のアイコンは URL に解決される", async () => {
    const store = useGameStore(
      prepare({ resolve_asset_path: (args) => `C:\\pkg\\images\\${String(args?.id)}` }),
    );
    await store.ingestTurn(turn());

    const entry = store.log.find((e) => e.kind === "narration");
    expect(entry).toBeDefined();
    if (entry?.kind !== "narration") throw new Error("narration が無い");
    expect(entry.segments?.map((s) => s.name)).toEqual(["", "凛花"]);
    const icon = entry.segments?.[1].icon ?? "";
    expect(icon).toContain("rinka.webp");
    expect(icon).not.toBe("rinka.webp"); // ID のままではなく URL
    expect(entry.text).toBe("教室が静まる。\n凛花「遅いわね」"); // ログ書き出し用の平らな文字列も残す
  });
});
