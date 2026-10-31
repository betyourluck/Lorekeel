/**
 * spec 37: 挿絵の履歴の一覧ページ。backend の返りを描き、クリックでプレビュー、確認の後に削除、
 * 戻るで閉じる。操作列 (ImageControls) の 2×2 アイコンが開く経路も見る。
 *
 * Red の確かめ方: ページの `onMounted` から `refreshImageHistory` を外すと 1 本目が落ちる /
 * 削除の `askConfirm` を外すと 3 本目 (キャンセルで消えない) が落ちる /
 * App.vue と同じく store の `imageHistory.open` を見ないと 4 本目・5 本目が落ちる。
 */
import { flushPromises } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import { useGameStore, type ImageHistoryEntry } from "../stores/game";
import { ipcCalls, mountWith } from "../test/mount";
import ImageControls from "./ImageControls.vue";
import ImageHistoryPage from "./ImageHistoryPage.vue";

const STEM = "0123456789abcdef";
function entries(n: number): ImageHistoryEntry[] {
  return Array.from({ length: n }, (_, i) => ({
    file_name: `${STEM}_${1_761_900_000_000 + (n - i) * 1000}.png`,
    path: `C:\\app_data\\logs\\generated_images\\${STEM}_${1_761_900_000_000 + (n - i) * 1000}.png`,
    created_ms: 1_761_900_000_000 + (n - i) * 1000,
    bytes: 2048 * (i + 1),
  }));
}

describe("挿絵の履歴 (spec 37)", () => {
  it("開くと backend の一覧をサムネイルで描き、枚数と asset:// の URL が出る", async () => {
    const list = entries(3);
    const w = mountWith(ImageHistoryPage, {}, { list_generated_images: () => list });
    const game = useGameStore();
    game.started = true;
    game.imageHistory.open = true;
    await flushPromises();
    const thumbs = w.findAll('[data-testid="history-thumb"]');
    expect(thumbs).toHaveLength(3);
    // 新しい順 (backend の並びをそのまま描く)。
    expect(thumbs[0].attributes("data-name")).toBe(list[0].file_name);
    expect(w.find('[data-testid="history-count"]').text()).toContain("3");
    const img = thumbs[0].find("img");
    expect(img.attributes("src")).toContain("asset.localhost");
    expect(img.attributes("src")).toContain(STEM);
    // 読んだのは現在のログフォルダ。
    const call = ipcCalls.find((c) => c.cmd === "list_generated_images");
    expect(call?.args).toMatchObject({ folder: game.logDir });
  });

  it("空なら案内文、失敗なら理由を一覧の中に出す", async () => {
    const w = mountWith(ImageHistoryPage, {}, { list_generated_images: () => [] });
    await flushPromises();
    expect(w.find('[data-testid="history-empty"]').exists()).toBe(true);
    expect(w.find('[data-testid="history-item"]').exists()).toBe(false);

    const w2 = mountWith(ImageHistoryPage, {}, {
      list_generated_images: () => {
        throw new Error("ゲームが開始されていません");
      },
    });
    await flushPromises();
    expect(w2.find('[data-testid="history-error"]').text()).toContain("ゲームが開始されていません");
  });

  it("サムネイルのクリックでプレビュー、Esc で一覧へ、もう一度 Esc で閉じる", async () => {
    const list = entries(2);
    const w = mountWith(ImageHistoryPage, {}, { list_generated_images: () => list });
    const game = useGameStore();
    game.imageHistory.open = true;
    await flushPromises();
    expect(w.find('[data-testid="history-preview"]').exists()).toBe(false);
    await w.findAll('[data-testid="history-thumb"]')[1].trigger("click");
    const preview = w.find('[data-testid="history-preview"]');
    expect(preview.exists()).toBe(true);
    expect(preview.find("img").attributes("src")).toContain(list[1].file_name);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await flushPromises();
    expect(w.find('[data-testid="history-preview"]').exists()).toBe(false);
    expect(game.imageHistory.open).toBe(true);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await flushPromises();
    expect(game.imageHistory.open).toBe(false);
  });

  it("削除は確認の後にだけ command を呼び、返った一覧で描き直す", async () => {
    const list = entries(2);
    let deleted: string | null = null;
    const w = mountWith(ImageHistoryPage, {}, {
      list_generated_images: () => list,
      delete_generated_image: (args) => {
        deleted = String(args?.fileName);
        return list.filter((e) => e.file_name !== deleted);
      },
    });
    const game = useGameStore();
    await flushPromises();
    // キャンセル → 呼ばれない・残る。
    const p1 = w.findAll('[data-testid="history-delete"]')[0].trigger("click");
    await flushPromises();
    expect(game.confirmDialog).not.toBeNull();
    game.resolveConfirm(false);
    await p1;
    await flushPromises();
    expect(deleted).toBeNull();
    expect(w.findAll('[data-testid="history-item"]')).toHaveLength(2);
    // OK → その名前で呼ばれ、一覧が 1 枚になる。
    const p2 = w.findAll('[data-testid="history-delete"]')[0].trigger("click");
    await flushPromises();
    game.resolveConfirm(true);
    await p2;
    await flushPromises();
    expect(deleted).toBe(list[0].file_name);
    expect(w.findAll('[data-testid="history-item"]')).toHaveLength(1);
    const call = ipcCalls.find((c) => c.cmd === "delete_generated_image");
    expect(call?.args).toMatchObject({ folder: game.logDir, fileName: list[0].file_name });
  });

  it("戻るで閉じ、操作列の 2×2 アイコンが開く (ゲーム開始前は押せない)", async () => {
    const w = mountWith(ImageHistoryPage, {}, { list_generated_images: () => [] });
    const game = useGameStore();
    game.imageHistory.open = true;
    await flushPromises();
    await w.find('[data-testid="history-back"]').trigger("click");
    expect(game.imageHistory.open).toBe(false);

    const c = mountWith(ImageControls, {}, {});
    const game2 = useGameStore();
    const btn = c.find('[data-testid="image-history-open"]');
    expect(btn.exists()).toBe(true);
    expect((btn.element as HTMLButtonElement).disabled).toBe(true);
    game2.started = true;
    await flushPromises();
    expect((btn.element as HTMLButtonElement).disabled).toBe(false);
    game2.floatingPanel = "prompt";
    await btn.trigger("click");
    expect(game2.imageHistory.open).toBe(true);
    expect(game2.floatingPanel).toBeNull();
  });

  it("ヘッダのコンボリストでパッケージを切り替えると一覧を畳む", async () => {
    mountWith(ImageHistoryPage, {}, { list_generated_images: () => entries(1) });
    const game = useGameStore();
    game.packagePath = "D:/pkgs/a";
    await flushPromises();
    game.imageHistory.open = true;
    await flushPromises();
    expect(game.imageHistory.open).toBe(true);
    game.packagePath = "D:/pkgs/b";
    await flushPromises();
    expect(game.imageHistory.open).toBe(false);
  });
});
