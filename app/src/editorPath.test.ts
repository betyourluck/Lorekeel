import { describe, expect, it } from "vitest";
import { contextAt, rangeOfPath } from "./editorPath";

/** カーソルを `|` で書いた原文から (text, pos) を作る。 */
function cursor(src: string): [string, number] {
  const pos = src.indexOf("|");
  return [src.slice(0, pos) + src.slice(pos + 1), pos];
}
const at = (src: string) => contextAt(...cursor(src));

describe("contextAt — 書きかけでも文脈が解ける", () => {
  it("多行で書いた gate の兄弟 kind: を拾う (v1 は同一行しか見えなかった)", () => {
    const c = at(`triggers:
  - id: t1
    when:
      kind: has_item
      ite|
`);
    expect(c.where).toBe("key");
    expect(c.path).toEqual(["triggers", "[]", "when"]);
    expect(c.tag.kind).toBe("has_item");
  });

  it("空行 (インデントだけ) でも解ける", () => {
    const c = at(`triggers:
  - id: t1
    when:
      kind: has_item
      item: k
      |
`);
    expect(c.where).toBe("key");
    expect(c.path).toEqual(["triggers", "[]", "when"]);
    expect(c.tag.kind).toBe("has_item");
  });

  it("flow style の途中でも op を拾う", () => {
    const c = at(`triggers:
  - id: t
    effects:
      - { op: set_flag, |
`);
    expect(c.where).toBe("key");
    expect(c.path).toEqual(["triggers", "[]", "effects", "[]"]);
    expect(c.tag.op).toBe("set_flag");
  });

  it("値の位置は末尾が欄名になる", () => {
    const c = at(`locations:
  room:
    exits:
      - to: |
`);
    expect(c.where).toBe("value");
    expect(c.path).toEqual(["locations", "room", "exits", "[]", "to"]);
  });

  it("列の新要素は item (容器が mapping の列か scalar の列かは構文では決まらない)", () => {
    const c = at(`locations:
  room:
    exits:
      - to: hall
      - |
`);
    expect(c.where).toBe("item");
    expect(c.path).toEqual(["locations", "room", "exits", "[]"]);
  });

  it("列の境界を跨いで兄弟タグを拾わない (of: の新要素が親 gate の kind を継がない)", () => {
    const c = at(`triggers:
  - id: t
    when:
      kind: all
      of:
        - |
`);
    expect(c.path).toEqual(["triggers", "[]", "when", "of", "[]"]);
    expect(c.tag.kind).toBeUndefined();
  });

  it("入れ子の内側の gate は自分の kind を使う", () => {
    const c = at(`triggers:
  - id: t
    when:
      kind: all
      of:
        - kind: has_item
          ite|
`);
    expect(c.path).toEqual(["triggers", "[]", "when", "of", "[]"]);
    expect(c.tag.kind).toBe("has_item");
  });

  it("ルート直下", () => {
    const c = at(`start: room
loc|`);
    expect(c.where).toBe("key");
    expect(c.path).toEqual([]);
  });

  it("名前つき map の子の直下", () => {
    const c = at(`challenges:
  force_door:
    stat: STR
    |
`);
    expect(c.where).toBe("key");
    expect(c.path).toEqual(["challenges", "force_door"]);
  });
});

describe("rangeOfPath — 診断のパスをキーの範囲へ", () => {
  const src = `title: t
locations:
  hall:
    description: 広間
    exits:
      - to: cell
  cell:
    description: 独房
    exits:
      - to: hall
        gaet: { kind: flag_is, key: f }
`;

  it("同名キー (hall 側の exits) を跨いで一意に当てる", () => {
    const r = rangeOfPath(src, "locations.cell.exits[0].gaet");
    expect(r).not.toBeNull();
    expect(src.slice(r!.from, r!.to)).toBe("gaet");
    // 行全体でなくキーそのもの。
    expect(src.slice(0, r!.from).split("\n").length).toBe(11);
  });

  it("flow style の中でも解ける (v1 の行近似はここで諦めていた)", () => {
    const flow = `name: ア
stats: { HP: { initial: 1, mox: 2 } }
`;
    const r = rangeOfPath(flow, "stats.HP.mox");
    expect(r).not.toBeNull();
    expect(flow.slice(r!.from, r!.to)).toBe("mox");
  });

  it("引用キーも解ける", () => {
    const q = `locations:
  "a b":
    descriptoin: x
`;
    const r = rangeOfPath(q, "locations.a b.descriptoin");
    expect(q.slice(r!.from, r!.to)).toBe("descriptoin");
  });

  it("解けなければ null (位置を偽らない)", () => {
    expect(rangeOfPath(src, "locations.ghost.description")).toBeNull();
    expect(rangeOfPath(src, "")).toBeNull();
    expect(rangeOfPath(src, "locations.cell.exits[9].to")).toBeNull();
  });
});
