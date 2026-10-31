/**
 * spec 37 追補: 参照ストックの「生成済みフォルダから」。空きの末尾の枠にだけ出て、OS のダイアログ
 * (`pick_generated_image`) をログフォルダで開き、選ばれたパスを `put_reference_from_path` へ渡す。
 * キャンセル (null) なら何も入れない。
 *
 * Red の確かめ方: RefStockDialog からリンクを外すと 1 本目が落ちる / store の `putRefFromGenerated`
 * が null を見ずに put を呼ぶと 2 本目が落ちる。
 */
import { flushPromises } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import { useGameStore } from "../stores/game";
import { ipcCalls, mountWith } from "../test/mount";
import RefStockDialog from "./RefStockDialog.vue";

const EMPTY = { dir: "C:/app_data/refs/abc", picked: [] as [string, number][], skipped: [] as [string, string][], max: 3 };

describe("参照ストック — 生成済みフォルダから (spec 37 追補)", () => {
  it("空きの末尾にだけリンクが出て、ダイアログで選んだパスを枠へ渡す", async () => {
    const PICKED = "C:/app_data/logs/generated_images/0123456789abcdef_1761900000000.png";
    const w = mountWith(RefStockDialog, {}, {
      list_settings_sheets: () => EMPTY,
      pick_generated_image: () => PICKED,
      put_reference_from_path: () => ({ ...EMPTY, picked: [["ref1.png", 2048]] }),
    });
    const game = useGameStore();
    game.started = true; // 参照ストックはゲーム中だけ読む (onMounted は開始前に走ったので読み直す)
    await game.loadRefStock();
    await flushPromises();
    const links = w.findAll('[data-testid="ref-from-generated"]');
    expect(links).toHaveLength(1); // 前詰め = 押せる空きは末尾の 1 つだけ
    await links[0].trigger("click");
    await flushPromises();
    const pick = ipcCalls.find((c) => c.cmd === "pick_generated_image");
    expect(pick?.args).toMatchObject({ folder: game.logDir });
    const put = ipcCalls.find((c) => c.cmd === "put_reference_from_path");
    expect(put?.args).toMatchObject({ provider: game.imageGen.provider, slot: 1, path: PICKED });
    expect(game.refStock?.picked).toEqual([["ref1.png", 2048]]);
    // 1 枚入ったので、リンクは次の空き (枠 2) に移る。
    await flushPromises();
    expect(w.findAll('[data-testid="ref-from-generated"]')).toHaveLength(1);
  });

  it("ダイアログをキャンセルしたら何も入れない", async () => {
    const w = mountWith(RefStockDialog, {}, {
      list_settings_sheets: () => EMPTY,
      pick_generated_image: () => null,
      put_reference_from_path: () => {
        throw new Error("呼ばれてはいけない");
      },
    });
    const game = useGameStore();
    game.started = true;
    await game.loadRefStock();
    await flushPromises();
    await w.find('[data-testid="ref-from-generated"]').trigger("click");
    await flushPromises();
    expect(ipcCalls.some((c) => c.cmd === "put_reference_from_path")).toBe(false);
    expect(game.refStock?.picked).toEqual([]);
    expect(game.logToast).toBe("");
  });
});
