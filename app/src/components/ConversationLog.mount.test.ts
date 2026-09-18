/**
 * #94 (2026-09-01 実プレイ「1 ターンで 2 回ダイスを振ると 2 回目がクリックできない」) を固定する。
 *
 * DiceReveal は `state` を idle → rolling へ一方向にしか進めない。判定 2 件が**同じログ行**に積まれると
 * v-if は真のままなので、呼び出し側が key を切らないと Vue が**同じインスタンスを使い回し**、2 枚目は
 * rolling のまま = クリックが start() の先頭で弾かれる。処方は ConversationLog の
 * `:key="`${i}-${entry.revealed}`"` (値でなく位置で切る)。
 *
 * Red の確かめ方: ConversationLog.vue の checks 側 DiceReveal から `:key` を消すと、このテストの
 * 「2 枚目を押すと開く」で落ちる (revealed が 1 のまま)。
 */
import { flushPromises } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";

import { useGameStore } from "../stores/game";
import { mountWith } from "../test/mount";
import type { CheckView } from "../types/api";
import ConversationLog from "./ConversationLog.vue";

function check(entity: string, stat: string, roll: number): CheckView {
  return {
    entity,
    stat,
    sides: 20,
    count: 1,
    times: 1,
    roll,
    modifier: 0,
    total: roll,
    dc: 10,
    success: roll >= 10,
    narration: "",
    sound: null,
    degree: null,
    pushed: false,
    spent: 0,
    pending: false,
  };
}

/** 開帳の演出 (スクランブル 1.1 秒 + 着地の一拍 0.4 秒) を最後まで進める。 */
async function finishScramble() {
  await vi.advanceTimersByTimeAsync(2000);
  await flushPromises();
}

describe("開帳カード (#94)", () => {
  it("同じ行に積まれた 2 件目のカードは、1 件目を開けたあとクリックに応じる", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
    const wrapper = mountWith(ConversationLog, {}, { reveal_next: () => ({ revealed: 1, total: 2 }) });
    const game = useGameStore();
    game.log = [{ kind: "checks", checks: [check("player", "目星", 7), check("player", "聞き耳", 13)], revealed: 0 }];
    await flushPromises();

    // 1 枚目
    const first = wrapper.get("button.dice-card");
    expect(first.text()).toContain("目星");
    await first.trigger("click");
    await finishScramble();
    expect(game.log[0]).toMatchObject({ revealed: 1 });

    // 2 枚目: 別の判定のカードが待っていて、押せば開く
    const second = wrapper.get("button.dice-card");
    expect(second.text()).toContain("聞き耳");
    await second.trigger("click");
    await finishScramble();
    expect(game.log[0]).toMatchObject({ revealed: 2 });
    expect(game.hasUnrevealedDice).toBe(false);
    expect(wrapper.find("button.dice-card").exists()).toBe(false);
  });

  it("同じ出目が 2 回出ても 2 枚目は開く (key は値でなく位置で切る)", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
    const wrapper = mountWith(ConversationLog, {}, { reveal_next: () => ({ revealed: 1, total: 2 }) });
    const game = useGameStore();
    game.log = [{ kind: "checks", checks: [check("player", "目星", 7), check("player", "目星", 7)], revealed: 0 }];
    await flushPromises();

    await wrapper.get("button.dice-card").trigger("click");
    await finishScramble();
    await wrapper.get("button.dice-card").trigger("click");
    await finishScramble();
    expect(game.log[0]).toMatchObject({ revealed: 2 });
  });
});
