/**
 * spec 28 v2: **実語彙**での合成テスト。
 *
 * `editorCompletion.test.ts` は手で組んだ語彙で walk を測る (ロジックの話)。
 * こちらは backend が実際に組む語彙 (`editorVocabulary.fixture.json` = Rust の
 * `vocabulary_fixture_is_fresh` が書き出し、古くなればあちらが落ちる) を使う。
 * **継ぎ目 — バリアント名の綴り・文脈名・欄名の一致 — はどちらか片方では測れない。**
 */
import { describe, expect, it } from "vitest";
import fixture from "./editorVocabulary.fixture.json";
import { candidatesAt, type EditorVocabulary } from "./editorCompletion";

const V = fixture as unknown as EditorVocabulary;

function at(kind: string, src: string) {
  const pos = src.indexOf("|");
  return candidatesAt(V, kind, src.slice(0, pos) + src.slice(pos + 1), pos);
}
const names = (r: { options: { name: string }[] }) => r.options.map((o) => o.name).sort();

describe("実語彙での合成", () => {
  it("タグ欄が未定なら `kind` / `op` を先頭に押し上げる (名前順だと真ん中に埋もれる)", () => {
    const gate = at(
      "scenario",
      `triggers:
  - id: t
    when:
      |
`,
    );
    expect(gate.needsTag).toBe("kind");
    const op = at(
      "scenario",
      `triggers:
  - id: t
    effects:
      - |
`,
    );
    expect(op.needsTag).toBe("op");
    // 決まっている文脈では押し上げない。
    const decided = at(
      "scenario",
      `triggers:
  - id: t
    when:
      kind: has_item
      |
`,
    );
    expect(decided.needsTag).toBeNull();
    // タグを持たない文脈でも押し上げない。
    const loc = at(
      "scenario",
      `locations:
  room:
    |
`,
    );
    expect(loc.needsTag).toBeNull();
  });

  it("`kind:` が無い `when:` の下は Gate の和集合 (絞る材料が無いので正しい)", () => {
    const r = at(
      "scenario",
      `triggers:
  - id: test
    when:
      |
`,
    );
    expect(names(r)).toEqual([
      "at",
      "entity",
      "item",
      "key",
      "kind",
      "of",
      "present",
      "skill",
      "turns",
      "value",
    ]);
  });

  it("`kind:` を書いた行の**次の行**からバリアントで絞られる", () => {
    const hasItem = at(
      "scenario",
      `triggers:
  - id: test
    when:
      kind: has_item
      |
`,
    );
    expect(names(hasItem)).toEqual(["entity", "item", "kind"]);

    const turns = at(
      "scenario",
      `triggers:
  - id: test
    when:
      kind: turns_since
      |
`,
    );
    expect(names(turns)).toEqual(["entity", "key", "kind", "turns"]);
  });

  it("op も同じ (`entity` を持たない `move` で確かめる = 2026-07-27 の解像度)", () => {
    const mv = at(
      "scenario",
      `triggers:
  - id: t
    effects:
      - op: move
        |
`,
    );
    expect(names(mv)).toEqual(["op", "to"]);
    expect(names(mv)).not.toContain("entity");
  });

  it("`kind:` の値そのものは全バリアント名が出る", () => {
    const r = at(
      "scenario",
      `triggers:
  - id: t
    when:
      kind: |
`,
    );
    expect(names(r)).toContain("has_item");
    expect(names(r)).toContain("presence_is");
    expect(names(r)).toContain("not");
  });

  it("challenge の帰結スロットも辿れる (degree スロットの中の effects → Op)", () => {
    const r = at(
      "scenario",
      `challenges:
  force_door:
    on_success:
      effects:
        - op: set_flag
          |
`,
    );
    expect(names(r)).toEqual(["key", "op", "value"]);
  });

  it("campaign / character / manifest のルートも実語彙で辿れる", () => {
    expect(names(at("campaign", `edges:
  - from: a
    |
`))).toEqual(["from", "on_goal", "to"]);
    expect(names(at("character", `taboos:
  - kind: has_item
    |
`))).toEqual(["entity", "item", "kind"]);
    expect(names(at("manifest", `player:
  |
`))).toContain("stats");
  });

  it("候補は doc comment 由来の説明を持つ (実データでの確認)", () => {
    const r = at(
      "scenario",
      `triggers:
  - id: t
    effects:
      - op: |
`,
    );
    const give = r.options.find((o) => o.name === "give_item");
    expect(give?.doc).toBeTruthy();
  });
});
