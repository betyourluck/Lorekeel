/**
 * 最新へ飛ぶボタン (2026-11-17、ユーザー要望「最新までスクロールする FAB。一番下までいくと消える」→
 * 同日「もっと一番下に。音声や画像生成のバーの右で良い。右下にしましょう」)。
 *
 * 判定そのもの (しきい値・端数・スクロールできない中身) は `logScroll.test.ts` が純関数で留めている。
 * ここで確かめるのは**配線**だけ: スクロールと中身の出入りで取り直すこと・ボタンが出入りすること・
 * 押すと末尾を指して送ること・応答待ちの間は三点になること・**ボタンが出ている間は右下角の操作列
 * (読み上げ・挿絵) が左へ寄ること**。
 *
 * happy-dom はレイアウトを持たない (`scrollHeight` などは常に 0) ので、スクロール量はテストが要素へ
 * 与える。**与えた数値は実機の数値ではない** — ボタンの位置・大きさ・操作列と重ならないことは
 * dev サーバの実寸で確かめる (spec 31 の届かない範囲)。
 *
 * Red の確かめ方 (2026-11-17 に確認。操作列は読み上げ・挿絵を別々に壊した = 5 通り):
 * - `@scroll.passive="syncAtBottom"` を外す → スクロールしても出ないので、出ることを見るテストが落ちる。
 * - `mutations.observe(...)` を外す → 「行が増えて末尾が遠のけば出る」が落ちる。
 * - ボタンの `v-if` を `!game.logAtBottom` にする (文字の表示を見ない) → 「文字を隠している間」が落ちる。
 * - 操作列の `:class` を定位置だけにする → 「操作列が左へ寄る」が落ちる。
 */
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";

import { useGameStore } from "../stores/game";
import { mountWith } from "../test/mount";
import ConversationLog from "./ConversationLog.vue";
import ImageControls from "./ImageControls.vue";
import TtsControls from "./TtsControls.vue";

const FAB = '[data-testid="jump-latest"]';

/** レイアウトの無い happy-dom で、スクロール量を要素へ直接与える。 */
function setMetrics(el: Element, m: { scrollHeight: number; clientHeight: number; scrollTop: number }): void {
  for (const [key, value] of Object.entries(m)) {
    Object.defineProperty(el, key, { configurable: true, writable: true, value });
  }
}

/** 高さ 2000 の中身を高さ 600 の窓から見ている形にして、指定の位置へスクロールしたことにする。 */
async function scrollTo(wrapper: VueWrapper, scrollTop: number): Promise<void> {
  const scroller = wrapper.get('[data-testid="log-scroller"]');
  setMetrics(scroller.element, { scrollHeight: 2000, clientHeight: 600, scrollTop });
  await scroller.trigger("scroll");
}

/** MutationObserver のコールバックと、それに続く描き直しを待つ。 */
async function settle(): Promise<void> {
  await flushPromises();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await flushPromises();
}

async function mountLog(): Promise<VueWrapper> {
  const wrapper = mountWith(ConversationLog, {}, {});
  const game = useGameStore();
  game.log = [{ kind: "narration", text: "霧が晴れる。" }];
  await settle();
  return wrapper;
}

describe("最新へ飛ぶボタン", () => {
  it("末尾が見えている間は出ず、途中まで戻ると出て、一番下まで行くと消える", async () => {
    const wrapper = await mountLog();
    expect(wrapper.find(FAB).exists()).toBe(false); // 開いた直後 (スクロールできない中身)

    await scrollTo(wrapper, 0);
    expect(wrapper.find(FAB).exists()).toBe(true);

    await scrollTo(wrapper, 1400); // 2000 - 600 = 末尾
    expect(wrapper.find(FAB).exists()).toBe(false);
  });

  it("流れる側の外に置く (中に置くと中身と一緒に流れて右下角に留まらない)", async () => {
    const wrapper = await mountLog();
    await scrollTo(wrapper, 0);
    const scroller = wrapper.get('[data-testid="log-scroller"]').element;
    expect(scroller.contains(wrapper.get(FAB).element)).toBe(false);
  });

  it("押すと、中身の末尾を指してなめらかに送る", async () => {
    const wrapper = await mountLog();
    await scrollTo(wrapper, 0);
    const scroller = wrapper.get('[data-testid="log-scroller"]').element as HTMLElement;
    const spy = vi.fn();
    scroller.scrollTo = spy as unknown as typeof scroller.scrollTo;

    await wrapper.get(FAB).trigger("click");
    expect(spy).toHaveBeenCalledWith({ top: 2000, behavior: "smooth" });
  });

  it("スクロールが起きなくても、行が増えて末尾が遠のけば出る", async () => {
    const wrapper = await mountLog();
    await scrollTo(wrapper, 1400);
    expect(wrapper.find(FAB).exists()).toBe(false);

    // 長い語りが届いた形: 位置はそのままで中身だけが伸びる (scroll イベントは起きない)。
    const scroller = wrapper.get('[data-testid="log-scroller"]').element;
    setMetrics(scroller, { scrollHeight: 3000, clientHeight: 600, scrollTop: 1400 });
    useGameStore().log.push({ kind: "narration", text: "長い語りが届いた。" });
    await settle();
    expect(wrapper.find(FAB).exists()).toBe(true);
  });

  it("文字を隠している間 (挿絵の鑑賞) は出さず、部品が外れたら末尾扱いへ戻す", async () => {
    const wrapper = await mountLog();
    const game = useGameStore();
    await scrollTo(wrapper, 0);
    expect(game.jumpToLatestVisible).toBe(true);

    game.showText = false;
    await settle();
    expect(wrapper.find(FAB).exists()).toBe(false);
    expect(game.jumpToLatestVisible).toBe(false); // 操作列も左へ寄らない
    game.showText = true;
    await settle();
    expect(wrapper.find(FAB).exists()).toBe(true);

    // 編集モード・挿絵の履歴ページへ切り替わると会話ログは外れる。操作列が寄ったまま残らないこと。
    wrapper.unmount();
    expect(game.logAtBottom).toBe(true);
    expect(game.jumpToLatestVisible).toBe(false);
  });

  it("GM の応答を待っている間は三点、そうでなければ ↓", async () => {
    const wrapper = await mountLog();
    const game = useGameStore();
    await scrollTo(wrapper, 0);
    expect(wrapper.find('[data-testid="jump-latest-dots"]').exists()).toBe(false);
    expect(wrapper.find(`${FAB} svg`).exists()).toBe(true);

    game.loading = true;
    await settle();
    expect(wrapper.find('[data-testid="jump-latest-dots"]').exists()).toBe(true);
    expect(wrapper.find(`${FAB} svg`).exists()).toBe(false);
    // 三点の間も押せば同じく末尾へ送る。
    const scroller = wrapper.get('[data-testid="log-scroller"]').element as HTMLElement;
    const spy = vi.fn();
    scroller.scrollTo = spy as unknown as typeof scroller.scrollTo;
    await wrapper.get(FAB).trigger("click");
    expect(spy).toHaveBeenCalledTimes(1);
  });
});

/**
 * ボタンは会話ペインの右下角 = 読み上げ・挿絵の操作列の定位置に出る。出ている間は操作列がその左へ
 * 寄る。寄せた量で実際に重ならないことは dev サーバの実寸で確かめた (ここで見るのは出し分けだけ)。
 */
describe("右下角の操作列は、最新へ飛ぶボタンの左へ寄る", () => {
  it("読み上げの操作列", async () => {
    const wrapper = mountWith(TtsControls);
    const game = useGameStore();
    const classes = () => wrapper.get('[data-testid="tts-controls"]').classes();
    expect(classes()).toContain("right-4");

    game.logAtBottom = false;
    await flushPromises();
    expect(classes()).toContain("right-14");
    expect(classes()).not.toContain("right-4");

    game.logAtBottom = true;
    await flushPromises();
    expect(classes()).toContain("right-4");
  });

  it("挿絵の操作列 (読み上げと同居するときは、その左からさらに寄る)", async () => {
    const wrapper = mountWith(ImageControls);
    const game = useGameStore();
    const classes = () => wrapper.get('[data-testid="image-controls"]').classes();
    expect(classes()).toContain("right-4");

    game.logAtBottom = false;
    await flushPromises();
    expect(classes()).toContain("right-14");

    game.ttsFeature = true;
    await flushPromises();
    expect(classes()).toContain("right-[11.5rem]");

    game.logAtBottom = true;
    await flushPromises();
    expect(classes()).toContain("right-36");
  });
});
