import { describe, expect, it } from "vitest";
import { candidatesAt, resolveContext, type EditorVocabulary } from "./editorCompletion";

/** 実表の形を写した最小語彙 (walk とカテゴリ判定を測るためのもの)。
 *  実データそのものの正しさは Rust 側 (`editor_vocab` / `gm_core::wiring`) が固定する。 */
const V: EditorVocabulary = {
  roots: { scenario: "Scenario", campaign: "Campaign", character: "Character" },
  contexts: {
    Scenario: [{ name: "start" }, { name: "locations" }, { name: "triggers" }, { name: "cast" }],
    Location: [{ name: "description" }, { name: "exits" }, { name: "present" }, { name: "image" }],
    Exit: [{ name: "to" }, { name: "gate" }],
    Trigger: [{ name: "id" }, { name: "when" }, { name: "effects" }, { name: "repeatable" }],
    Gate: [{ name: "kind" }, { name: "entity" }, { name: "item" }, { name: "key" }, { name: "at" }],
    Op: [{ name: "op" }, { name: "to" }, { name: "from" }, { name: "item" }, { name: "key" }],
    Character: [{ name: "name" }, { name: "profile" }, { name: "taboos" }],
    Campaign: [{ name: "start" }, { name: "modules" }, { name: "edges" }],
    CampaignEdge: [{ name: "from" }, { name: "on_goal" }, { name: "to" }],
    LocationItem: [{ name: "when" }, { name: "take" }],
    StatDecl: [{ name: "initial" }, { name: "min" }, { name: "max" }],
  },
  wiring: [
    { parent: "Scenario", key: "locations", kind: "map_values", child: "Location" },
    { parent: "Scenario", key: "triggers", kind: "seq", child: "Trigger" },
    { parent: "Scenario", key: "initial_stats", kind: "stat_map", child: "StatDecl" },
    { parent: "Location", key: "exits", kind: "seq", child: "Exit" },
    { parent: "Location", key: "items", kind: "item_map", child: "LocationItem", child_tagged: "Gate" },
    { parent: "Exit", key: "gate", kind: "direct", child: "Gate" },
    { parent: "Trigger", key: "when", kind: "direct", child: "Gate" },
    { parent: "Trigger", key: "effects", kind: "seq", child: "Op" },
    { parent: "Gate", key: "of", kind: "seq", child: "Gate" },
    { parent: "Character", key: "taboos", kind: "seq", child: "Gate" },
    { parent: "Campaign", key: "edges", kind: "seq", child: "CampaignEdge" },
  ],
  gate_variant_keys: {
    has_item: [{ name: "kind" }, { name: "entity" }, { name: "item" }],
    flag_is: [{ name: "kind" }, { name: "key" }, { name: "value" }],
  },
  op_variant_keys: {
    give_item: [{ name: "op" }, { name: "from" }, { name: "to" }, { name: "item" }],
    move: [{ name: "op" }, { name: "to" }],
    set_flag: [{ name: "op" }, { name: "key" }, { name: "value" }],
  },
  tag_values: { kind: [{ name: "has_item" }], op: [{ name: "give_item" }, { name: "move" }] },
  ids: {
    locations: [{ name: "hall" }, { name: "cell" }],
    entities: [{ name: "player" }, { name: "alice" }],
    flags: [{ name: "door_open" }],
    stats: [{ name: "hp" }],
    items: [{ name: "key" }],
    modules: [{ name: "study" }, { name: "cellar" }],
    goals: [{ name: "escaped" }],
  },
};

/** カーソルを `|` で書いた原文から候補を取る。 */
function at(kind: string, src: string) {
  const pos = src.indexOf("|");
  return candidatesAt(V, kind, src.slice(0, pos) + src.slice(pos + 1), pos);
}
const names = (r: { options: { name: string }[] }) => r.options.map((o) => o.name).sort();

describe("candidatesAt — 文脈は木のパスを配線で辿って決まる", () => {
  it("多行で書いた gate をバリアントまで絞る (v1 は Gate 全和集合に落ちていた)", () => {
    const r = at(
      "scenario",
      `triggers:
  - id: t
    when:
      kind: has_item
      |
`,
    );
    expect(r.isKey).toBe(true);
    expect(names(r)).toEqual(["entity", "item", "kind"]);
  });

  it("多行で書いた op も同じく絞れる", () => {
    const r = at(
      "scenario",
      `triggers:
  - id: t
    effects:
      - op: give_item
        |
`,
    );
    expect(names(r)).toEqual(["from", "item", "op", "to"]);
  });

  it("同じ `to:` が親で割れる — Exit なら場所、give_item ならエンティティ", () => {
    const asExit = at(
      "scenario",
      `locations:
  room:
    exits:
      - to: |
`,
    );
    expect(asExit.isKey).toBe(false);
    // 保存済みの語彙 (hall/cell) + このバッファで宣言中の room。
    expect(names(asExit)).toEqual(["cell", "hall", "room"]);

    const asGive = at(
      "scenario",
      `triggers:
  - id: t
    effects:
      - { op: give_item, from: player, to: |
`,
    );
    expect(names(asGive)).toEqual(["alice", "player"]);
  });

  it("同じ `start:` が文書の種類で割れる (v1 は docKind の場当たりで補っていた)", () => {
    expect(names(at("scenario", `start: |`))).toEqual(["cell", "hall"]);
    expect(names(at("campaign", `start: |`))).toEqual(["cellar", "study"]);
  });

  it("`key:` はタグでフラグと数値に割れる", () => {
    const flag = at(
      "scenario",
      `triggers:
  - id: t
    effects:
      - { op: set_flag, key: |
`,
    );
    expect(names(flag)).toEqual(["door_open"]);
  });

  it("作者の付けた名前の段では型のキーを出さない", () => {
    const r = at(
      "scenario",
      `locations:
  |
`,
    );
    expect(r.options).toEqual([]);
    // 一段下がれば Location のキー。
    const inner = at(
      "scenario",
      `locations:
  room:
    |
`,
    );
    expect(names(inner)).toEqual(["description", "exits", "image", "present"]);
  });

  it("mapping の列の新要素はキーの位置、scalar の列は値の位置", () => {
    const asKeys = at(
      "scenario",
      `locations:
  room:
    exits:
      - |
`,
    );
    expect(asKeys.isKey).toBe(true);
    expect(names(asKeys)).toEqual(["gate", "to"]);

    const asValues = at(
      "scenario",
      `cast:
  - |
`,
    );
    expect(asValues.isKey).toBe(false);
    expect(names(asValues)).toEqual(["alice", "player"]);
  });

  it("編集中のバッファで宣言した id が候補に混ざる (保存前でも効く)", () => {
    const r = at(
      "scenario",
      `locations:
  attic:
    description: 屋根裏
  room:
    exits:
      - to: |
`,
    );
    expect(names(r)).toContain("attic");
    expect(names(r)).toContain("hall"); // 保存済みの語彙も残る
  });

  it("campaign の edges は要素の型まで辿れる", () => {
    const r = at(
      "campaign",
      `edges:
  - from: study
    |
`,
    );
    expect(names(r)).toEqual(["from", "on_goal", "to"]);
    const goal = at(
      "campaign",
      `edges:
  - from: study
    on_goal: |
`,
    );
    expect(names(goal)).toEqual(["escaped"]);
  });

  it("Location.items の旧形式 (Gate 直書き) はタグで Gate へ切り替わる", () => {
    const r = at(
      "scenario",
      `locations:
  room:
    items:
      鍵:
        kind: has_item
        |
`,
    );
    expect(names(r)).toEqual(["entity", "item", "kind"]);
  });

  it("未知の文書種別は黙る", () => {
    expect(at("mystery", `start: |`).options).toEqual([]);
  });
});

describe("resolveContext — 名前の段と未知キー", () => {
  it("未知キー (作者の語彙) は文脈を変えない", () => {
    expect(resolveContext(V, "Scenario", ["locations", "room"]).ctx).toBe("Location");
    expect(resolveContext(V, "Scenario", ["locations", "room", "nonexistent"]).ctx).toBe("Location");
  });

  it("名前つき容器の直後は atName", () => {
    expect(resolveContext(V, "Scenario", ["locations"]).atName).toBe(true);
    expect(resolveContext(V, "Scenario", ["locations", "room"]).atName).toBe(false);
    expect(resolveContext(V, "Scenario", ["initial_stats"]).atName).toBe(true);
  });

  it("列は段を挟まない", () => {
    expect(resolveContext(V, "Scenario", ["triggers", "[]"]).ctx).toBe("Trigger");
    expect(resolveContext(V, "Scenario", ["triggers", "[]", "effects", "[]"]).ctx).toBe("Op");
  });
});
