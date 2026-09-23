/**
 * 開帳 (spec 18 Phase A) の漏洩防止 — **右ペインの盤面も伏せる** (2026-09-23 ユーザーFB
 * 「サイコロを振るクリックをする前に結果がある程度わかってしまう」)。
 *
 * 会話ログの後続行・SE・CG は開帳完了まで保留していたが、右ペインが読む盤面 (状態 = フラグ・所持品・
 * 到達した目標 / この場にいる / マップ / あらすじの「最近の出来事」) はターンの結果が届いた瞬間に
 * 差し替えていた。成功で立つフラグ・手に入る道具・到達した目標・「判定の結末: …」つきの要約は、
 * どれも出目の帰結そのもの。
 *
 * Red の確かめ方: stores/game.ts の ingestTurn で `this.putBoard(…, revealing)` の `revealing` を
 * `false` にすると「開く前は古い盤面のまま」が落ちる。
 */
import { describe, expect, it } from "vitest";

import { prepare } from "../test/mount";
import type { StateView, TurnView } from "../types/api";
import { useGameStore } from "./game";

function board(turn: number, flags: string[], inventory: string[]): StateView {
  return {
    turn,
    location: "gate",
    location_title: "門",
    inventory,
    flags: flags.map((key) => ({ key, title: key, turn, cause: "" })),
    entities: [],
    goal_reached: false,
    goals: [],
    reached_goal: null,
    party: [],
  } as StateView;
}

const CHECK = {
  entity: "player",
  stat: "STR",
  sides: 20,
  count: 1,
  times: 1,
  roll: 17,
  modifier: 2,
  total: 19,
  dc: 15,
  success: true,
  tier: null,
  narration: "",
  sound: null,
  degree: null,
  pushed: false,
  spent: 0,
  pending: false,
};

/** 判定つき (withDice) か否かの受理ターン。結果の盤面は「扉が開き、鍵を手に入れた」。 */
function turnWith(withDice: boolean): TurnView {
  return {
    accepted: true,
    narration: "力をこめて扉を押した。",
    rolls: [],
    checks: withDice ? [CHECK] : [],
    stat_rolls: [],
    beats: [],
    attempts: 1,
    retries: [],
    reasons: [],
    state: board(2, ["扉を開けた"], ["鍵"]),
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
    new_log: [{ turn: 2, summary: "扉を押した／判定の結末: 扉が開いた" }],
    epilogue: null,
    map: { nodes: [], edges: [] },
    decision: null,
    contest: null,
    consistency: null,
    facts: null,
    facts_policy: "locked",
  } as unknown as TurnView;
}

const ipc = { reveal_next: () => ({ revealed: 1, total: 1 }), reveal_all: () => ({ revealed: 1, total: 1 }) };

describe("開帳が済むまで右ペインの盤面を伏せる", () => {
  it("開く前は古い盤面のまま・開いたら新しい盤面になる", async () => {
    const store = useGameStore(prepare(ipc));
    store.diceReveal = true;
    store.state = board(1, [], []);
    await store.ingestTurn(turnWith(true));

    expect(store.hasUnrevealedDice).toBe(true);
    expect(store.state?.flags.map((f) => f.key)).toEqual([]); // 成功で立つフラグが見えない
    expect(store.state?.inventory).toEqual([]); // 手に入る道具も見えない
    expect(store.recentLog).toEqual([]); // 「判定の結末」つきの要約も見えない

    store.revealNext(store.revealTargetIndex); // 伏せカードを開く (1 枚。先頭の未開帳の行から順にしか開けない)
    expect(store.hasUnrevealedDice).toBe(false);
    expect(store.state?.flags.map((f) => f.key)).toEqual(["扉を開けた"]);
    expect(store.state?.inventory).toEqual(["鍵"]);
    expect(store.recentLog.map((l) => l.turn)).toEqual([2]);
  });

  it("判定の無いターンと、開帳の演出を切ったときは即座に反映する", async () => {
    const store = useGameStore(prepare(ipc));
    store.diceReveal = true;
    store.state = board(1, [], []);
    await store.ingestTurn(turnWith(false));
    expect(store.state?.inventory).toEqual(["鍵"]);

    const off = useGameStore(prepare(ipc));
    off.diceReveal = false;
    off.state = board(1, [], []);
    await off.ingestTurn(turnWith(true));
    expect(off.state?.inventory).toEqual(["鍵"]);
  });

  it("全部開く (演出オフへの切替時の脱出口) でも盤面が反映される", async () => {
    const store = useGameStore(prepare(ipc));
    store.diceReveal = true;
    store.state = board(1, [], []);
    await store.ingestTurn(turnWith(true));
    expect(store.pendingBoard).not.toBeNull();
    store.revealAll();
    expect(store.pendingBoard).toBeNull();
    expect(store.state?.inventory).toEqual(["鍵"]);
  });
});
