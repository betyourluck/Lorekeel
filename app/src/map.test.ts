/**
 * マップのリスト化 (spec 15 rev2) のテスト。
 *
 * 見た目は測れないが、「現在地から何が見えるか」は DTO から決まるのでここで固定できる。
 * 特に**畳み方** (同じ行き先への複数の出口) は目で見ても分からない種類の誤りになる。
 */
import { describe, expect, it } from "vitest";
import { mapList } from "./map";
import type { MapNode, MapView } from "./types/api";

const node = (id: string, over: Partial<MapNode> = {}): MapNode => ({
  id,
  title: id,
  description: "",
  image: null,
  current: false,
  visited: true,
  ...over,
});

describe("mapList", () => {
  it("現在地と、そこから出る辺だけを返す", () => {
    const view: MapView = {
      nodes: [node("hall", { current: true }), node("garden"), node("cellar"), node("attic")],
      edges: [
        { from: "hall", to: "garden", locked: false },
        { from: "hall", to: "cellar", locked: true },
        // 他の訪問済みノードから出る辺は見ない (奥は畳む)。
        { from: "garden", to: "attic", locked: false },
      ],
    };
    const { current, exits } = mapList(view);
    expect(current?.id).toBe("hall");
    expect(exits.map((x) => x.node.id)).toEqual(["garden", "cellar"]);
    expect(exits.map((x) => x.locked)).toEqual([false, true]);
  });

  it("作者が書いた exits の順を保つ (名前順に並べ替えない)", () => {
    const view: MapView = {
      nodes: [node("a", { current: true }), node("z"), node("m")],
      edges: [
        { from: "a", to: "z", locked: false },
        { from: "a", to: "m", locked: false },
      ],
    };
    expect(mapList(view).exits.map((x) => x.node.id)).toEqual(["z", "m"]);
  });

  it("同じ行き先への複数の出口は 1 行に畳み、一本でも通れるなら通れる", () => {
    const view: MapView = {
      nodes: [node("a", { current: true }), node("b")],
      edges: [
        // 正面の扉は施錠、裏口は開いている = 「行ける」が正しい。
        { from: "a", to: "b", locked: true },
        { from: "a", to: "b", locked: false },
      ],
    };
    const { exits } = mapList(view);
    expect(exits).toHaveLength(1);
    expect(exits[0].locked).toBe(false);

    // 逆に全部閉じていれば閉じたまま。
    const allLocked = mapList({
      nodes: view.nodes,
      edges: view.edges.map((e) => ({ ...e, locked: true })),
    });
    expect(allLocked.exits[0].locked).toBe(true);
  });

  it("未踏の行き先も名前つきで出る (中身は伏せたまま)", () => {
    const view: MapView = {
      nodes: [
        node("a", { current: true, description: "d" }),
        node("b", { title: "苔むした回廊", visited: false }),
      ],
      edges: [{ from: "a", to: "b", locked: false }],
    };
    const { exits } = mapList(view);
    expect(exits[0].node.title).toBe("苔むした回廊");
    expect(exits[0].node.visited).toBe(false);
    expect(exits[0].node.description).toBe(""); // backend が伏せている
  });

  it("解決できない行き先は落とす / 現在地が無ければ空", () => {
    const orphan = mapList({
      nodes: [node("a", { current: true })],
      edges: [{ from: "a", to: "ghost", locked: false }],
    });
    expect(orphan.exits).toEqual([]);

    const empty = mapList({ nodes: [], edges: [] });
    expect(empty.current).toBeNull();
    expect(empty.exits).toEqual([]);
  });
});
