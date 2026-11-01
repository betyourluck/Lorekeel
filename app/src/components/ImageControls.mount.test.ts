/**
 * 挿絵の操作列: GM (LLM) の応答待ちの間は、生成・参照ストック・プロンプト工房・履歴の 4 つを
 * 押せなくする (ユーザー要望 2026-11-02)。待ちが終われば戻る。保存・表示切替・文字切替は対象外。
 *
 * Red の確かめ方: ImageControls の `gmBusy` を `computed(() => false)` にすると 1 本目が落ちる。
 */
import { flushPromises } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import { t } from "../i18n";
import { useGameStore } from "../stores/game";
import { mountWith } from "../test/mount";
import ImageControls from "./ImageControls.vue";

function buttons(w: ReturnType<typeof mountWith>) {
  const LABELS = [t("image.generate"), t("refStock.heading"), t("promptWorkshop.heading"), t("image.history")];
  return w.findAll("button").filter((b) => LABELS.includes(b.attributes("aria-label") ?? ""));
}

describe("挿絵の操作列: GM の応答待ち", () => {
  it("loading の間は 4 つのボタンが押せず、終われば戻る", async () => {
    const w = mountWith(ImageControls);
    const game = useGameStore();
    game.started = true;
    await flushPromises();
    expect(buttons(w)).toHaveLength(4);
    expect(buttons(w).every((b) => b.attributes("disabled") === undefined)).toBe(true);

    game.loading = true;
    await flushPromises();
    expect(buttons(w).every((b) => b.attributes("disabled") !== undefined)).toBe(true);

    game.loading = false;
    await flushPromises();
    expect(buttons(w).every((b) => b.attributes("disabled") === undefined)).toBe(true);
  });

  it("文字の表示切替は応答待ちでも押せる", async () => {
    const w = mountWith(ImageControls);
    const game = useGameStore();
    game.started = true;
    game.loading = true;
    await flushPromises();
    const text = w.findAll("button").find((b) => b.attributes("aria-label") === t("image.hideText"));
    expect(text).toBeDefined();
    expect(text?.attributes("disabled")).toBeUndefined();
  });
});
