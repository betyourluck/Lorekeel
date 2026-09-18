/**
 * StatePanel のファイルタブ (編集モード) の行内編集を固定する (spec 31 Phase A)。
 *
 * #92 (2026-08-28): 入力欄は v-for の内側に置く。文字列の ref だと Vue が**配列**として集め、
 * 配列に .focus() は無いので例外になり、`void focusDraft(...)` がそれを握り潰して
 * **フォーカスが一度も当たらない** → blur が発火せず入力欄が開きっぱなしになった。
 * 処方は関数 ref (`:ref="bindDraft"`)。
 * Red の確かめ方: `:ref="bindDraft"` を `ref="draftInput"` に戻し、focusDraft が
 * `draftInput.value` を読む形にすると「フォーカスが入力欄にある」で落ちる。
 *
 * ダブルクリック (2026-09-14): 名前のクリックが**即座に**全画面プレビューを開いていたので、
 * 2 回目のクリックがその幕に当たり dblclick が名前に届かなかった。処方はクリックを 300ms 遅らせ、
 * dblclick (と detail > 1) で取り消す。
 * Red の確かめ方: 名前のボタンの @click を修正前の即時プレビュー
 * (`g.category === 'image' ? (preview = mediaUrl(f.relPath)) : toggleAudio(f.relPath)`) に戻すと
 * 「300ms 経つまで開かない」で落ちる。
 * **届かない半分**: 実機の症状は「2 回目のクリックが幕に**落ちる**」ことだったが、happy-dom は
 * 座標から要素を決める hit-testing をしないので、幕が 2 回目を奪う様子そのものは再現できない。
 * ここで固定するのは取り消しの機構 (遅延と dblclick) だけ。
 */
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";

import { t } from "../i18n";
import { useGameStore } from "../stores/game";
import { mountWith } from "../test/mount";
import StatePanel from "./StatePanel.vue";

/** 部品を立て、編集モードへ入ってファイルタブを開いた状態にする。 */
async function openFilesTab(setup: (game: ReturnType<typeof useGameStore>) => void): Promise<VueWrapper> {
  const wrapper = mountWith(StatePanel, { attachTo: document.body });
  const game = useGameStore();
  setup(game);
  // ファイルタブは editor.on が false → true に変わった瞬間に開く (watch は即時実行ではない)
  game.editor.on = true;
  await flushPromises();
  return wrapper;
}

describe("新規作成の入力欄 (#92)", () => {
  it("開いた入力欄にフォーカスが当たり、空のまま抜けると閉じる", async () => {
    const wrapper = await openFilesTab(() => {});
    await wrapper.get(`button[title="${t("editor.new_scenario")}"]`).trigger("click");
    await flushPromises();

    const input = wrapper.get(`input[placeholder="${t("editor.newFilePlaceholder")}"]`);
    expect(document.activeElement).toBe(input.element);

    // 空のまま抜ける = 取り消し (command は投げない)
    await input.trigger("blur");
    await flushPromises();
    expect(wrapper.find(`input[placeholder="${t("editor.newFilePlaceholder")}"]`).exists()).toBe(false);
  });

  it("改名の入力欄にもフォーカスが当たり、拡張子を除いた部分が選ばれる", async () => {
    const wrapper = await openFilesTab((game) => {
      game.editor.files = [{ relPath: "scenarios/main.yaml", category: "scenario" }];
    });
    await wrapper.get('button[title="' + t("editor.rowTitle") + '"]').trigger("dblclick");
    await flushPromises();

    const input = wrapper.get("input");
    expect(document.activeElement).toBe(input.element);
    const el = input.element as HTMLInputElement;
    expect([el.selectionStart, el.selectionEnd]).toEqual([0, "main".length]);
  });
});

describe("メディアの名前のクリックとダブルクリック (2026-09-14)", () => {
  const image = { relPath: "images/gate.webp", category: "image" };

  async function openMedia(): Promise<VueWrapper> {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    return openFilesTab((game) => {
      game.editor.view = "media";
      game.editor.media = [image];
      game.editor.absRoot = "C:\\pkg";
    });
  }
  const nameButton = (w: VueWrapper) => w.get(`button[title="${t("editor.assetRowTitle")}"]`);
  const previewShown = (w: VueWrapper) => w.find("div.cursor-zoom-out").exists();

  it("クリックしてから 300ms 経つまでプレビューは開かない (経てば開く)", async () => {
    const wrapper = await openMedia();
    await nameButton(wrapper).trigger("click", { detail: 1 });
    expect(previewShown(wrapper)).toBe(false);

    await vi.advanceTimersByTimeAsync(300);
    expect(previewShown(wrapper)).toBe(true);
  });

  it("その間にダブルクリックが来れば改名の入力欄になり、プレビューは開かない", async () => {
    const wrapper = await openMedia();
    const button = nameButton(wrapper);
    await button.trigger("click", { detail: 1 });
    await button.trigger("click", { detail: 2 });
    await button.trigger("dblclick");
    await flushPromises();

    const input = wrapper.get("input");
    expect((input.element as HTMLInputElement).value).toBe("gate.webp");

    await vi.advanceTimersByTimeAsync(1000);
    expect(previewShown(wrapper)).toBe(false);
  });
});
