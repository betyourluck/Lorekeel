/**
 * spec 38 Phase D: AI でパッケージを組む (AI 編集パネルの「パッケージ全体」)。
 *
 * - 作者が画面で直した計画 (指示の書き換え・行を外す) が、そのまま組み立てに渡る
 * - 変更セットは「雛形のまま」を既定で選ばない。ファイル名を押すと差分が開く
 * - 適用したら一覧・層 2・語彙を取り直し、開いているファイルが書き換わったら読み直す
 * - 組み立ての最中は編集モード全体が止まる (保存・作成・出る・対象の切り替え = 決定 10)
 * - 未保存の変更があると、計画の前に保存を聞く (決定 11)
 *
 * Red の確かめ方: `removePlanItem` の splice を外す / 既定の選択から `template_only` の除外を外す
 * → 1 本目 / 適用後の読み直しか `refreshEditorIssues` を外す → 2 本目 / `saveEditorFile` の
 * `buildLocked` か `exitEditor` の busy 判定を外す → 3 本目 / `planBuild` の
 * `settleUnsavedForBuild` を外す → 4 本目。
 */
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import { useGameStore } from "../stores/game";
import { ipcCalls, mountWith } from "../test/mount";
import type { BuildPlanView, BuildView } from "../types/api";
import EditAssistPanel from "./EditAssistPanel.vue";

const PLAN: BuildPlanView = {
  items: [
    { path: "characters/alice.yaml", action: "create", instruction: "司書のアリス" },
    { path: "memoria/promise.yaml", action: "create", instruction: "約束の伏線" },
    { path: "scenarios/main.yaml", action: "edit", instruction: "alice を cast と present に" },
  ],
  rejected: ["`scenarios/side.yaml` を読ませる行がありません"],
  model: "fake-model",
};
/** IPC の偽装は返した値をそのまま渡すので、store が計画を直すと元の定数まで書き換わる — 毎回写す。 */
const plan = (): BuildPlanView => JSON.parse(JSON.stringify(PLAN));

function view(): BuildView {
  const file = (path: string, action: "create" | "edit", extra: Partial<BuildView["files"][number]> = {}) => ({
    path,
    action,
    changed: true,
    template_only: false,
    repaired: false,
    iterations: 3,
    summary: `${path} を書きました`,
    diagnostics: [],
    diff: `--- a/${path}\n+++ b/${path}\n@@ -1 +1 @@\n-name: x\n+name: アリス`,
    after: "name: アリス\n",
    ...extra,
  });
  return {
    files: [
      file("characters/alice.yaml", "create"),
      file("scenarios/main.yaml", "edit", { repaired: true }),
      file("memoria/promise.yaml", "create", { template_only: true }),
    ],
    failed: [],
    not_run: [],
    package_issues: [],
    preexisting_issues: 0,
    iterations: 9,
    prompt_tokens: 100,
    completion_tokens: 10,
    cache_read: 50,
    model: "fake-model",
  };
}

const REQUEST = "アリスと約束の場面を足して";

/** 編集モードに入った体にして、パネルを「パッケージ全体」へ。 */
async function openPackageMode(w: VueWrapper) {
  const game = useGameStore();
  game.editor.on = true;
  game.editor.root = "C:/pkg";
  game.editor.assist.open = true;
  await flushPromises();
  await w.find('[data-testid="assist-mode-package"]').trigger("click");
  await w.find('[data-testid="build-request"]').setValue(REQUEST);
  return game;
}

async function planAndRun(w: VueWrapper) {
  await w.find('[data-testid="build-plan"]').trigger("click");
  await flushPromises();
  await w.find('[data-testid="build-run"]').trigger("click");
  await flushPromises();
}

describe("AI でパッケージを組む (spec 38 Phase D)", () => {
  it("計画の行を直して外した形が、そのまま組み立てに渡る", async () => {
    let ran: Record<string, unknown> | undefined;
    const w = mountWith(EditAssistPanel, {}, {
      build_plan: () => plan(),
      build_run: (args) => {
        ran = JSON.parse(JSON.stringify(args));
        return view();
      },
    });
    const game = await openPackageMode(w);
    await w.find('[data-testid="build-plan"]').trigger("click");
    await flushPromises();
    expect(ipcCalls.find((c) => c.cmd === "build_plan")?.args).toEqual({ request: REQUEST });
    expect(w.findAll('[data-testid="build-plan-row"]')).toHaveLength(3);
    expect(w.find('[data-testid="build-rejected"]').exists()).toBe(true); // 立て直したことを言う

    // 2 行目の指示を直し、3 行目を外す。
    await w.findAll('[data-testid="build-plan-instruction"]')[1].setValue("約束の伏線 (丘の樫の木)");
    await w.findAll('[data-testid="build-plan-remove"]')[2].trigger("click");
    expect(w.findAll('[data-testid="build-plan-row"]')).toHaveLength(2);

    await w.find('[data-testid="build-run"]').trigger("click");
    await flushPromises();
    expect(ran).toEqual({
      request: REQUEST,
      plan: [PLAN.items[0], { ...PLAN.items[1], instruction: "約束の伏線 (丘の樫の木)" }],
    });

    // 変更セット: 雛形のまま終わったファイルは既定で選ばない。
    expect(game.editor.build.phase).toBe("done");
    const boxes = w.findAll('[data-testid="build-file-select"]');
    expect(boxes.map((b) => (b.element as HTMLInputElement).checked)).toEqual([true, true, false]);
    // ファイル名を押すと差分が開く。
    expect(w.find('[data-testid="build-diff"]').exists()).toBe(false);
    await w.findAll('[data-testid="build-file-path"]')[0].trigger("click");
    expect(w.find('[data-testid="build-diff"]').text()).toContain("+name: アリス");
  });

  it("選んだファイルを適用し、一覧・検査・語彙を取り直して、開いているファイルを読み直す", async () => {
    const w = mountWith(EditAssistPanel, {}, {
      build_plan: () => plan(),
      build_run: () => view(),
      build_apply: () => ({
        written: ["characters/alice.yaml", "scenarios/main.yaml"],
        conflicts: [],
        errors: [],
        forked: false,
        files: [
          { rel_path: "package.yaml", category: "package" },
          { rel_path: "characters/alice.yaml", category: "character" },
          { rel_path: "scenarios/main.yaml", category: "scenario" },
        ],
      }),
      read_editor_file: () => "title: 新しい部屋\r\n",
      inspect_editor_package: () => [],
      editor_vocabulary: () => ({ keys: {}, ids: {} }),
    });
    const game = await openPackageMode(w);
    game.editor.current = "scenarios/main.yaml";
    game.editor.text = "title: 部屋\n";
    game.editor.savedText = "title: 部屋\n";
    await planAndRun(w);

    await w.find('[data-testid="build-apply-selected"]').trigger("click");
    await flushPromises();
    expect(ipcCalls.find((c) => c.cmd === "build_apply")?.args).toEqual({
      paths: ["characters/alice.yaml", "scenarios/main.yaml"],
      fork: false,
    });
    expect(game.editor.build.phase).toBe("applied");
    expect(w.find('[data-testid="build-applied"]').exists()).toBe(true);
    expect(game.editor.files.map((f) => f.relPath)).toContain("characters/alice.yaml");
    // 開いていたファイルはディスクの新しい本文で読み直す (改行は LF にそろえ、元の改行を覚える)。
    expect(game.editor.text).toBe("title: 新しい部屋\n");
    expect(game.editor.savedText).toBe(game.editor.text);
    expect(game.editor.eol).toBe("\r\n");
    expect(ipcCalls.some((c) => c.cmd === "inspect_editor_package")).toBe(true);
    expect(ipcCalls.some((c) => c.cmd === "editor_vocabulary")).toBe(true);
  });

  it("組み立ての最中は編集モード全体が止まる (保存・作成・出る・対象の切り替え)", async () => {
    let release: (v: BuildView) => void = () => {};
    const w = mountWith(EditAssistPanel, {}, {
      build_plan: () => plan(),
      build_run: () =>
        new Promise<BuildView>((r) => {
          release = r;
        }),
    });
    const game = await openPackageMode(w);
    game.editor.current = "scenarios/main.yaml";
    game.editor.savedText = "x";
    game.editor.text = "x";
    await planAndRun(w);
    expect(game.editorBuildBusy).toBe(true);

    game.editor.text = "y"; // エディタは読み取り専用だが、store を直に触っても保存はされない
    await game.saveEditorFile();
    await game.createEditorFile("character", "bob");
    expect(await game.exitEditor()).toBe(false);
    expect(game.editor.on).toBe(true);
    const touched = ipcCalls.filter((c) => ["save_package_file", "create_editor_file", "close_editor"].includes(c.cmd));
    expect(touched).toEqual([]);
    expect((w.find('[data-testid="assist-mode-file"]').element as HTMLButtonElement).disabled).toBe(true);

    release(view());
    await flushPromises();
    expect(game.editorBuildBusy).toBe(false);
    expect(game.editor.build.phase).toBe("done");
  });

  it("未保存の変更があると、計画の前に保存を聞く (断れば計画しない・承れば保存してから)", async () => {
    const w = mountWith(EditAssistPanel, {}, {
      build_plan: () => plan(),
      save_package_file: () => ({ forked: false, fork_warning: null }),
      inspect_editor_package: () => [],
      editor_vocabulary: () => ({ keys: {}, ids: {} }),
    });
    const game = await openPackageMode(w);
    game.editor.current = "scenarios/main.yaml";
    game.editor.savedText = "old";
    game.editor.text = "edited";

    const p1 = w.find('[data-testid="build-plan"]').trigger("click");
    await flushPromises();
    expect(game.confirmDialog).not.toBeNull();
    game.resolveConfirm(false);
    await p1;
    await flushPromises();
    expect(ipcCalls.some((c) => c.cmd === "build_plan")).toBe(false);

    const p2 = w.find('[data-testid="build-plan"]').trigger("click");
    await flushPromises();
    game.resolveConfirm(true);
    await p2;
    await flushPromises();
    const save = ipcCalls.findIndex((c) => c.cmd === "save_package_file");
    const planned = ipcCalls.findIndex((c) => c.cmd === "build_plan");
    expect(save).toBeGreaterThanOrEqual(0);
    expect(planned).toBeGreaterThan(save); // 保存してから計画する
    expect(ipcCalls[save].args).toMatchObject({ relPath: "scenarios/main.yaml", text: "edited" });
    expect(game.editor.build.phase).toBe("planned");
  });
});
