/**
 * 終幕後のセーブから再開したとき、エピローグを読み返せる (spec 11 改訂、2026-09-19 ユーザー要望)。
 *
 * 当初 spec 11 はエピローグを会話ログにだけ積み、SessionSave に入れていなかった — 終幕の後に
 * 再開・スロットのロードをすると、結末までの語りは出るのにエピローグだけが消えていた。
 * backend は SessionSave.epilogue を ResumeView.epilogue で返す (保存と往復は harness の
 * `epilogue_survives_save_and_old_save_reads_none` が固定)。ここでは表示の並びを固定する。
 * Red の確かめ方: stores/game.ts の applyGameView から `view.resumed.epilogue` の分岐を消すと落ちる。
 *
 * store は `transport.ts` を import 時に読むので IPC の偽装が要る = mount project に置く
 * (部品は立てない)。アセットは全部 null なので IPC は 1 本も飛ばない (偽装の表は空)。
 */
import { describe, expect, it } from "vitest";

import { t } from "../i18n";
import { prepare } from "../test/mount";
import type { GameView, ResumeView } from "../types/api";
import { useGameStore } from "./game";

/** ログの本文だけを並べる (本文を持たない行 = ビート等は空文字)。 */
const texts = (log: ReturnType<typeof useGameStore>["log"]) => log.map((e) => ("text" in e ? e.text : ""));

function viewResumedWith(resumed: ResumeView): GameView {
  return {
    title: "湖畔の洋館",
    location: "hall",
    description: "古い洋館の玄関。",
    state: {} as GameView["state"],
    background: null,
    bgm: null,
    present_characters: [],
    resumed,
    warnings: [],
    synopsis: [],
    recent_log: [],
    map: { nodes: [], edges: [] },
    decision: null,
    contest: null,
    facts: [],
    facts_policy: "locked",
  };
}

describe("終幕後の再開 (spec 11 改訂)", () => {
  it("前回までの語りの後に、エピローグをマーカーつきで出す", async () => {
    prepare();
    const game = useGameStore();
    await game.applyGameView(
      viewResumedWith({ turn: 12, last_narration: "扉が静かに閉じた。", warnings: [], epilogue: "三年後、彼女は再び湖畔を訪れた。" }),
      "packages/lakeside_manor",
    );
    const lines = texts(game.log);
    const last = lines.indexOf("扉が静かに閉じた。");
    expect(last).toBeGreaterThan(-1);
    expect(lines.slice(last + 1)).toEqual([t("store.epilogueMarker"), "三年後、彼女は再び湖畔を訪れた。"]);
  });

  it("エピローグの無いセーブ (終幕前・旧セーブ) ではマーカーも出さない", async () => {
    prepare();
    const game = useGameStore();
    await game.applyGameView(
      viewResumedWith({ turn: 5, last_narration: "霧が窓を這う。", warnings: [], epilogue: null }),
      "packages/lakeside_manor",
    );
    expect(texts(game.log)).not.toContain(t("store.epilogueMarker"));
  });
});
