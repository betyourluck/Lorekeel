/**
 * spec 37: 挿絵の履歴の一覧ページ。backend の返りを描き、クリックでプレビュー、確認の後に削除、
 * 戻るで閉じる。操作列 (ImageControls) の 2×2 アイコンが開く経路も見る。
 *
 * Red の確かめ方: ページの `onMounted` から `refreshImageHistory` を外すと 1 本目が落ちる /
 * 削除の `askConfirm` を外すと 3 本目 (キャンセルで消えない) が落ちる /
 * App.vue と同じく store の `imageHistory.open` を見ないと 4 本目・5 本目が落ちる。
 * 書き出し (2026-11-26 追補) は 4 通りで Red を確かめた: ボタンを外す (4 本とも落ちる) /
 * ゴミ箱の後ろへ並べ替える (並びの 1 本) / store が null でもトーストする (キャンセルの 1 本) /
 * 押し直しの止めを外す (二重に開かないの 1 本)。
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

  // 書き出し (spec 37 追補, 2026-11-26)。OS の保存ダイアログは backend の中で開くので、ここで
  // 見えるのは command の呼び方と返り (書いたパス / キャンセルの null / 失敗) の扱いまで。
  it("書き出しはカードでもプレビューでもゴミ箱の左に並ぶ", async () => {
    const w = mountWith(ImageHistoryPage, {}, { list_generated_images: () => entries(2) });
    await flushPromises();
    for (const item of w.findAll('[data-testid="history-item"]')) {
      const exp = item.find('[data-testid="history-export"]');
      const del = item.find('[data-testid="history-delete"]');
      expect(exp.exists()).toBe(true);
      // 文書順で書き出しが先 = 左。
      expect(exp.element.compareDocumentPosition(del.element) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    }
    await w.findAll('[data-testid="history-thumb"]')[0].trigger("click");
    const pexp = w.find('[data-testid="history-preview-export"]');
    const pdel = w.find('[data-testid="history-preview-delete"]');
    expect(pexp.exists()).toBe(true);
    expect(pexp.element.compareDocumentPosition(pdel.element) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("書き出しを押すとその名前で command を呼び、書いたパスをトーストする (一覧も元のまま)", async () => {
    const list = entries(2);
    const w = mountWith(ImageHistoryPage, {}, {
      list_generated_images: () => list,
      export_generated_image: () => "D:\\Pictures\\気に入った一枚.png",
    });
    const game = useGameStore();
    await flushPromises();
    await w.findAll('[data-testid="history-export"]')[1].trigger("click");
    await flushPromises();
    const call = ipcCalls.find((c) => c.cmd === "export_generated_image");
    expect(call?.args).toMatchObject({ folder: game.logDir, fileName: list[1].file_name });
    expect(game.logToast).toContain("D:\\Pictures\\気に入った一枚.png");
    // コピーなので一覧は変わらず、確認ダイアログも出ない。
    expect(w.findAll('[data-testid="history-item"]')).toHaveLength(2);
    expect(game.confirmDialog).toBeNull();

    // プレビューからも同じ command。
    await w.findAll('[data-testid="history-thumb"]')[0].trigger("click");
    await w.find('[data-testid="history-preview-export"]').trigger("click");
    await flushPromises();
    const calls = ipcCalls.filter((c) => c.cmd === "export_generated_image");
    expect(calls).toHaveLength(2);
    expect(calls[1].args).toMatchObject({ fileName: list[0].file_name });
  });

  it("キャンセル (null) なら何も言わず、失敗なら理由をトーストする", async () => {
    let reply: () => unknown = () => null;
    const w = mountWith(ImageHistoryPage, {}, {
      list_generated_images: () => entries(1),
      export_generated_image: () => reply(),
    });
    const game = useGameStore();
    await flushPromises();
    await w.find('[data-testid="history-export"]').trigger("click");
    await flushPromises();
    expect(ipcCalls.some((c) => c.cmd === "export_generated_image")).toBe(true);
    expect(game.logToast).toBe("");
    expect(w.findAll('[data-testid="history-item"]')).toHaveLength(1);

    reply = () => {
      throw new Error("同じ名前のファイルが既にあります");
    };
    await w.find('[data-testid="history-export"]').trigger("click");
    await flushPromises();
    expect(game.logToast).toContain("同じ名前のファイルが既にあります");
  });

  it("書き出しの最中はボタンが止まり、終われば戻る (ダイアログを二重に開かない)", async () => {
    let finish: (v: string | null) => void = () => {};
    const w = mountWith(ImageHistoryPage, {}, {
      list_generated_images: () => entries(2),
      export_generated_image: () => new Promise((r) => (finish = r)),
    });
    await flushPromises();
    const btns = () => w.findAll('[data-testid="history-export"]');
    await btns()[0].trigger("click");
    await flushPromises();
    expect(btns().every((b) => (b.element as HTMLButtonElement).disabled)).toBe(true);
    await btns()[1].trigger("click");
    await flushPromises();
    expect(ipcCalls.filter((c) => c.cmd === "export_generated_image")).toHaveLength(1);
    finish(null);
    await flushPromises();
    expect(btns().every((b) => !(b.element as HTMLButtonElement).disabled)).toBe(true);
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
