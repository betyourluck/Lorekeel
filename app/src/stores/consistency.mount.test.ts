/**
 * 一貫性検査の表示 (spec 32 Phase A)。
 *
 * **閾値を超えた軸があるときだけ 1 行**出す。全件は app_data/logs/consistency.jsonl にあるので
 * 画面は静かに保つ — 毎ターン 7 行出すと没入を壊し、誤警告が真の警告の信頼を削る
 * (cache_floor / workflowNoRefs と同じ規則)。
 *
 * Red の確かめ方: stores/game.ts の `flagged.length > 0` を `flagged.length >= 0` に変えると
 * 「閾値以下では鳴らさない」が落ちる。`.filter((f) => f.flagged)` を外しても同じ。
 *
 * store は `transport.ts` を import 時に読むので IPC の偽装が要る = mount project に置く
 * (部品は立てない)。アセットは全部 null なので IPC は 1 本も飛ばない。
 */
import { describe, expect, it } from "vitest";

import { prepare } from "../test/mount";
import type { ConsistencyView, TurnView } from "../types/api";
import { useGameStore } from "./game";

/** 検査結果だけを差し替えた最小の受理ターン。 */
function acceptedTurn(consistency: ConsistencyView | null): TurnView {
  return {
    accepted: true,
    narration: "あかりは俯いた。",
    rolls: [],
    checks: [],
    stat_rolls: [],
    beats: [],
    attempts: 1,
    retries: [],
    state: {} as TurnView["state"],
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
    consistency,
    facts: null,
    facts_policy: "locked",
  } as TurnView;
}

const systemLines = (store: ReturnType<typeof useGameStore>) =>
  store.log.filter((e) => e.kind === "system").map((e) => ("text" in e ? e.text : ""));

describe("一貫性検査の表示", () => {
  it("閾値を超えた軸があるときだけ 1 行出す", async () => {
    const store = useGameStore(prepare());
    await store.ingestTurn(
      acceptedTurn({
        model: "jev-1.13.0",
        findings: [
          { axis: "不在者の発話", question_id: "absent_speaker", score: 0.93, flagged: true, advisory: false },
          { axis: "未所持物の譲渡", question_id: "handed_unowned", score: 0.08, flagged: false, advisory: false },
        ],
      }),
    );

    const lines = systemLines(store);
    expect(lines).toHaveLength(1);
    expect(lines[0]).toContain("不在者の発話");
    expect(lines[0]).toContain("0.93");
    // 閾値以下の軸は画面に出さない (jsonl には残る)。
    expect(lines[0]).not.toContain("未所持物の譲渡");
  });

  it("閾値以下しか無いターンでは鳴らさない", async () => {
    const store = useGameStore(prepare());
    await store.ingestTurn(
      acceptedTurn({
        model: "jev-1.13.0",
        findings: [
          { axis: "不在者の発話", question_id: "absent_speaker", score: 0.23, flagged: false, advisory: false },
          { axis: "過去の語りとの矛盾", question_id: "past_contradiction", score: 0.15, flagged: false, advisory: false },
        ],
      }),
    );
    expect(systemLines(store)).toHaveLength(0);
  });

  it("検査が無いターン (dev off / 鍵なし) は何も出さない", async () => {
    const store = useGameStore(prepare());
    await store.ingestTurn(acceptedTurn(null));
    expect(systemLines(store)).toHaveLength(0);
    // 語りそのものは従来どおり出る (検査の不在がターンの表示を変えない)。
    expect(store.log.some((e) => e.kind === "narration")).toBe(true);
  });

  it("秘匿は「参考」と明示し、backend が並べた順をそのまま出す", async () => {
    const store = useGameStore(prepare());
    // backend (run_consistency_check) はスコアの降順で渡してくる。**frontend は並べ替えない** —
    // 二重にソートすると、並べ方を決める場所が 2 つになって必ずずれる。
    await store.ingestTurn(
      acceptedTurn({
        model: "jev-1.13.0",
        findings: [
          { axis: "人物像からの逸脱", question_id: "profile_deviation", score: 0.88, flagged: true, advisory: false },
          { axis: "秘匿の漏洩", question_id: "secret:genzo", score: 0.61, flagged: true, advisory: true },
        ],
      }),
    );

    const line = systemLines(store)[0];
    expect(line.indexOf("人物像からの逸脱")).toBeLessThan(line.indexOf("秘匿の漏洩"));
    // 秘匿だけ「参考」が付く (示唆でも鳴るので二値で切れない = 還流対象外なのと同じ理由)。
    expect(line).toContain("秘匿の漏洩 0.61(参考)");
    expect(line).not.toContain("人物像からの逸脱 0.88(参考)");
  });
});
