/**
 * 右ペインの「進行」タブ — 旧「状態」タブの統合 (2026-09-23 ユーザーFB「進行と状態をガチャガチャ
 * 切り替える必要があり煩わしい」)。並びは ターン / 現在地 (+マップへのリンク) / 目標 (3 件ずつの
 * ページ送り) / 所持品 / フラグ (畳める) + シードリセット / この場にいる (下に固定)。
 *
 * Red の確かめ方: 「状態」タブのボタンを rail に戻すとタブの件数で、目標を `pagedGoals` でなく
 * `game.state.goals` で回すとページ送りで、この場にいる を スクロール領域の内側へ戻すと
 * 固定の検査で落ちる。
 * **届かない半分**: 「下に固定されて見えるか」は実寸のレイアウト (happy-dom は測らない) の話。
 * ここで固定するのは DOM の構造 (スクロール領域の外に置かれていること) まで。
 */
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import { t } from "../i18n";
import { useGameStore } from "../stores/game";
import { mountWith } from "../test/mount";
import type { StateView } from "../types/api";
import StatePanel from "./StatePanel.vue";

const goals = (n: number) =>
  Array.from({ length: n }, (_, i) => ({ id: `g${i + 1}`, title: `目標${i + 1}`, hint: "" }));

function state(over: Partial<StateView> = {}): StateView {
  return {
    turn: 7,
    location: "living",
    location_title: "リビング",
    inventory: ["スマートフォン"],
    flags: [{ key: "met", title: "出会った", turn: 2, cause: "" }],
    entities: [],
    goal_reached: false,
    goals: goals(7),
    reached_goal: null,
    party: [],
    ...over,
  } as StateView;
}

async function mountProgress(over: Partial<StateView> = {}): Promise<VueWrapper> {
  const wrapper = mountWith(StatePanel, { attachTo: document.body });
  const game = useGameStore();
  game.state = state(over);
  game.presentCharacters = [{ id: "player", name: "ゆうじ", icon: null, iconId: null } as never];
  await flushPromises();
  return wrapper;
}

const goalTitles = (w: VueWrapper) => w.findAll("li").map((li) => li.text()).filter((s) => s.startsWith("目標"));

describe("進行タブ (旧「状態」を統合)", () => {
  it("「状態」タブは無く、進行タブに現在地・所持品・フラグ・シードリセットが並ぶ", async () => {
    const w = await mountProgress();
    expect(w.find(`button[title="${t("state.tabProgressTitle")}"]`).exists()).toBe(true);
    const tabTitles = w.findAll("nav button").map((b) => b.attributes("title") ?? "");
    expect(tabTitles.some((s) => s.startsWith("状態"))).toBe(false);
    expect(tabTitles.filter((s) => s.includes("Ctrl+"))).toHaveLength(3); // 進行・マップ・あらすじ (既成事実は facts_policy 既定 locked で出ない)
    const text = w.text();
    for (const s of ["7", "リビング", "スマートフォン", "出会った", t("state.resetSeed")]) {
      expect(text).toContain(s);
    }
    // 指定の並び: ターン → 現在地 → 目標 → 所持品 → フラグ
    const order = [t("state.turn"), t("state.location"), t("state.goals"), t("state.inventory"), t("state.flags")].map((s) =>
      text.indexOf(s),
    );
    expect(order).toEqual([...order].sort((a, b) => a - b));
  });

  it("現在地のリンクでマップのタブへ移る", async () => {
    const w = await mountProgress();
    const link = w.findAll("button").find((b) => b.text().includes(t("state.viewMap")))!;
    await link.trigger("click");
    await flushPromises();
    expect(w.find('[data-testid="present-footer"]').exists()).toBe(false); // 進行タブを離れた
    expect(w.get(`button[title="${t("state.tabMapTitle")}"]`).classes()).toContain("border-ember");
  });

  it("目標は 3 件ずつで、● がページ数ぶん並び、押すと切り替わる", async () => {
    const w = await mountProgress();
    expect(goalTitles(w)).toEqual(["目標1", "目標2", "目標3"]);
    const dots = w.get('[data-testid="goal-pages"]').findAll("button");
    expect(dots).toHaveLength(3); // 7 件 = 3 ページ
    await dots[2].trigger("click");
    expect(goalTitles(w)).toEqual(["目標7"]);
  });

  it("1 ページに収まるなら ● は出さない", async () => {
    const w = await mountProgress({ goals: goals(3) });
    expect(w.find('[data-testid="goal-pages"]').exists()).toBe(false);
  });

  it("到達した目標があれば、そのページへ寄せる", async () => {
    const w = await mountProgress();
    useGameStore().state = state({ reached_goal: "g5" });
    await flushPromises();
    expect(goalTitles(w)[0]).toBe("目標4");
    expect(goalTitles(w).some((s) => s.includes("目標5"))).toBe(true);
  });

  it("フラグは畳めて、開閉は覚えられる", async () => {
    localStorage.removeItem("kataribe.flagsCollapsed");
    const w = await mountProgress();
    expect(w.find('[data-testid="flags-body"]').exists()).toBe(true);
    await w.get('[data-testid="flags-toggle"]').trigger("click");
    expect(w.find('[data-testid="flags-body"]').exists()).toBe(false);
    expect(localStorage.getItem("kataribe.flagsCollapsed")).toBe("1");
    // 畳んでもシードリセットは残る (フラグの下に置く)
    expect(w.text()).toContain(t("state.resetSeed"));
    w.unmount();

    const again = await mountProgress();
    expect(again.find('[data-testid="flags-body"]').exists()).toBe(false);
    localStorage.removeItem("kataribe.flagsCollapsed");
  });

  it("この場にいる はスクロール領域の外 (下に固定)", async () => {
    const w = await mountProgress();
    const footer = w.get('[data-testid="present-footer"]');
    const scroller = footer.element.previousElementSibling as HTMLElement;
    expect(scroller.className).toContain("overflow-y-auto");
    expect(scroller.contains(footer.element)).toBe(false);
    expect(scroller.textContent).toContain(t("state.resetSeed"));
  });
});
