/**
 * CodeMirror 組み込み文言の和訳表のテスト (2026-09-01)。
 *
 * 核心は **網羅を手書きの一覧で確かめないこと** — `@codemirror/search` の dist から
 * `phrase()` に渡される文字列を実際に抽出して照合する。ライブラリを上げて新しい文言が
 * 増えたら、このテストが落ちて気づける (放置すると「その 1 行だけ英語のまま」という
 * 沈黙する失敗になる)。app backend の `vocabulary_fixture_is_fresh` と同じ流儀。
 */
import { describe, expect, it } from "vitest";
// ライブラリの実ソースを文字列として読む。**node:fs を使わない** — vue-tsc がテストも型検査
// するので、@types/node を足さずに済む Vite の `?raw` を使う (実行は vitest、型は string)。
import searchSource from "@codemirror/search?raw";
import { searchPhrases } from "./editorPhrases";

/** `@codemirror/search` の dist が実際に `phrase()` へ渡すキーを抽出する。 */
function libraryPhraseKeys(): string[] {
  const src = searchSource;
  const keys = new Set<string>();
  // `state.phrase("X"` / `view.state.phrase("X"` / `phrase(view, "X")`
  for (const m of src.matchAll(/\.phrase\(\s*"([^"]+)"/g)) keys.add(m[1]);
  for (const m of src.matchAll(/\bphrase\(\s*\w+\s*,\s*"([^"]+)"/g)) keys.add(m[1]);
  return [...keys].sort();
}

describe("searchPhrases", () => {
  it("ライブラリが引く文言をすべて覆う", () => {
    const lib = libraryPhraseKeys();
    expect(lib.length).toBeGreaterThan(10); // 抽出そのものが壊れていないことの下限
    const ja = searchPhrases("ja");
    const missing = lib.filter((k) => !(k in ja));
    expect(missing).toEqual([]);
  });

  it("使われないキーを抱えない (綴り違いは黙って英語のまま出るので検出する)", () => {
    const lib = new Set(libraryPhraseKeys());
    const stray = Object.keys(searchPhrases("ja")).filter((k) => !lib.has(k));
    expect(stray).toEqual([]);
  });

  it("差し込み位置 $ を落とさない", () => {
    const ja = searchPhrases("ja");
    for (const key of Object.keys(ja)) {
      if (key.includes("$")) expect(ja[key]).toContain("$");
    }
  });

  it("英語は上書きしない (キー自身が英語なので写すとライブラリ側の更新から取り残される)", () => {
    expect(searchPhrases("en")).toEqual({});
  });
});
