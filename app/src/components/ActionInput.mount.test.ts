/**
 * コンテキスト使用率と送信マークが**同じ場所で排他に入れ替わる**ことを固定する (2026-09-23)。
 *
 * 利用者要望: 「プロンプトに何も入っていないときは ↲ が出ないので、その場所にメーターを。
 * 文字が入っているときは ↲ に」。空き地を使う配置なので、**入れ替わりが壊れると
 * どちらも出ない / 両方出るという形で即座に破綻する**。
 *
 * Red の確かめ方: ゲージの `v-if="!text.trim()"` を `v-if="true"` に変えると
 * 「文字を打つとゲージが消える」で落ちる。
 *
 * **届かない半分** (spec 31 の凍結どおり): 位置が揃っているか・幅が入力欄を潰さないか・
 * placeholder と重ならないかは **レイアウト**なので happy-dom では測れない。
 * そちらは dev サーバの実測で確認した (760px / 1280px の両方で中心 y 一致・重なりなし)。
 */
import { flushPromises } from "@vue/test-utils";
import { beforeEach, describe, expect, it } from "vitest";

import { useGameStore } from "../stores/game";
import { mountWith } from "../test/mount";
import ActionInput from "./ActionInput.vue";

/** ゲージが描かれる条件 (分子 = 直近の入力・分母 = 登録モデルの窓) を揃える。 */
function seedUsage(game: ReturnType<typeof useGameStore>): void {
  localStorage.setItem(
    "kataribe.aiModelProfiles",
    JSON.stringify([
      {
        id: "a", name: "muse", model: "muse-spark", baseUrl: "b", apiKey: "k",
        useTools: true, effort: "", maxTokens: "", contextTokens: 1_048_576,
      },
    ]),
  );
  game.llmModel = "muse-spark";
  game.lastPrompt = 14_123;
  // textarea は未開始だと disabled で入力が届かない (実使用と同じ条件に揃える)。
  game.started = true;
}

describe("入力欄の右下 — ゲージと送信マークの排他", () => {
  beforeEach(() => localStorage.clear());

  it("空欄ではゲージが出て、送信マークは隠れている", async () => {
    const wrapper = mountWith(ActionInput, { attachTo: document.body });
    seedUsage(useGameStore());
    await flushPromises();

    const gauge = wrapper.find(".tabular-nums");
    expect(gauge.exists()).toBe(true);
    expect(gauge.text().replace(/\s+/g, " ")).toContain("1.3%");
    // 送信マークは v-show なので**存在はする**が display:none
    expect(wrapper.find("button").attributes("style") ?? "").toContain("display: none");
  });

  it("文字を打つとゲージが消え、送信マークが出る", async () => {
    const wrapper = mountWith(ActionInput, { attachTo: document.body });
    seedUsage(useGameStore());
    await flushPromises();

    await wrapper.find("textarea").setValue("扉を開ける");
    await flushPromises();

    expect(wrapper.find(".tabular-nums").exists()).toBe(false);
    expect(wrapper.find("button").attributes("style") ?? "").not.toContain("display: none");
  });

  it("消すと元に戻る (片道ではない)", async () => {
    const wrapper = mountWith(ActionInput, { attachTo: document.body });
    seedUsage(useGameStore());
    await flushPromises();
    const ta = wrapper.find("textarea");
    expect(wrapper.find(".tabular-nums").exists()).toBe(true);
    await ta.setValue("あ");
    await flushPromises();
    expect(wrapper.find(".tabular-nums").exists()).toBe(false);

    await ta.setValue("");
    await flushPromises();
    expect(wrapper.find(".tabular-nums").exists()).toBe(true);
  });

  it("空白だけの入力はゲージのまま (trim で判定している)", async () => {
    const wrapper = mountWith(ActionInput, { attachTo: document.body });
    seedUsage(useGameStore());
    await flushPromises();
    await wrapper.find("textarea").setValue("   ");
    await flushPromises();
    expect(wrapper.find(".tabular-nums").exists()).toBe(true);
  });

  it("窓が引けない登録モデルでは何も出ない (空欄でも)", async () => {
    const wrapper = mountWith(ActionInput, { attachTo: document.body });
    const game = useGameStore();
    localStorage.setItem(
      "kataribe.aiModelProfiles",
      JSON.stringify([{ id: "b", name: "x", model: "unknown-model", baseUrl: "b", apiKey: "", useTools: true, effort: "", maxTokens: "" }]),
    );
    game.llmModel = "unknown-model";
    game.lastPrompt = 14_123;
    game.started = true;
    await flushPromises();
    expect(wrapper.find(".tabular-nums").exists()).toBe(false);
  });
});
