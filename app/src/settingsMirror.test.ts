/**
 * UI 設定ミラーの純粋部のテスト (2026-08-31)。
 *
 * 対象は DOM に依らない部分だけ — snapshot の採取/検証/復元判定と debounce 機構。
 * `Storage.prototype` の実フックと reload は実機の層 (GUI 目視で確認する)。
 */
import { describe, expect, it } from "vitest";
import {
  applySnapshot,
  collectSnapshot,
  createMirror,
  parseSnapshot,
  shouldRestore,
  type StorageLike,
} from "./settingsMirror";

/** テスト用の素朴な Storage 実装。 */
function fakeStorage(init: Record<string, string> = {}): StorageLike {
  const map = new Map(Object.entries(init));
  return {
    get length() {
      return map.size;
    },
    key: (i) => [...map.keys()][i] ?? null,
    getItem: (k) => map.get(k) ?? null,
    setItem: (k, v) => void map.set(k, v),
    removeItem: (k) => void map.delete(k),
  };
}

describe("collectSnapshot", () => {
  it("kataribe.* だけを写す (他アプリのキーや素のキーは混ぜない)", () => {
    const s = fakeStorage({
      "kataribe.theme": "dark",
      "kataribe.fontScale": "18",
      "other.app": "x",
      loose: "y",
    });
    expect(collectSnapshot(s)).toEqual({
      "kataribe.theme": "dark",
      "kataribe.fontScale": "18",
    });
  });
});

describe("parseSnapshot", () => {
  it("backend と同じ検証則 — 接頭辞つき文字列 map だけを通し、壊れは null", () => {
    expect(parseSnapshot('{"kataribe.theme":"dark"}')).toEqual({ "kataribe.theme": "dark" });
    expect(parseSnapshot("{}")).toEqual({});
    expect(parseSnapshot("not json")).toBeNull();
    expect(parseSnapshot('["kataribe.theme"]')).toBeNull();
    expect(parseSnapshot('{"kataribe.fontScale":18}')).toBeNull();
    expect(parseSnapshot('{"evil.key":"x"}')).toBeNull();
  });
});

describe("shouldRestore", () => {
  it("手元が空 + file 非空のときだけ真 (手元に 1 つでもあれば localStorage が正)", () => {
    const snap = { "kataribe.theme": "light" };
    expect(shouldRestore(fakeStorage(), snap)).toBe(true);
    expect(shouldRestore(fakeStorage({ "kataribe.lang": "ja" }), snap)).toBe(false);
    expect(shouldRestore(fakeStorage(), {})).toBe(false);
    expect(shouldRestore(fakeStorage(), null)).toBe(false);
    // 接頭辞の無いキーは「手元の設定」に数えない — 復元を妨げない。
    expect(shouldRestore(fakeStorage({ unrelated: "x" }), snap)).toBe(true);
  });

  it("applySnapshot で復元すると次からは復元しない (一度きり性)", () => {
    const s = fakeStorage();
    const snap = { "kataribe.theme": "light", "kataribe.fontScale": "20" };
    applySnapshot(s, snap);
    expect(s.getItem("kataribe.theme")).toBe("light");
    expect(shouldRestore(s, snap)).toBe(false);
  });
});

describe("createMirror", () => {
  /** 手回しタイマー: schedule された仕事を tick() で発火させる。 */
  function manualTimer() {
    let queued: (() => void) | null = null;
    return {
      schedule: (fn: () => void) => {
        queued = fn;
        return {};
      },
      cancel: () => void (queued = null),
      tick: () => {
        const fn = queued;
        queued = null;
        fn?.();
      },
      get pending() {
        return queued !== null;
      },
    };
  }

  it("kataribe.* の変更を debounce して全量 snapshot を保存する (接頭辞外は無反応)", () => {
    const s = fakeStorage();
    const saved: string[] = [];
    const t = manualTimer();
    const m = createMirror(s, (json) => saved.push(json), 800, t.schedule, t.cancel);

    s.setItem("kataribe.theme", "dark");
    m.onChange("kataribe.theme");
    s.setItem("kataribe.fontScale", "18");
    m.onChange("kataribe.fontScale"); // 2 回目は 1 回目の予約を置き換える (debounce)
    expect(saved).toHaveLength(0); // まだ発火していない
    t.tick();
    expect(saved).toHaveLength(1); // 2 変更で保存は 1 回
    expect(JSON.parse(saved[0])).toEqual({
      "kataribe.theme": "dark",
      "kataribe.fontScale": "18",
    });

    // 接頭辞外のキーは予約を作らない。
    s.setItem("unrelated", "x");
    m.onChange("unrelated");
    expect(t.pending).toBe(false);
  });

  it("removeItem も全量 dump で自然に収束し、flush は待ち中だけ発火する", () => {
    const s = fakeStorage({ "kataribe.msgColor": "#fff", "kataribe.theme": "dark" });
    const saved: string[] = [];
    const t = manualTimer();
    const m = createMirror(s, (json) => saved.push(json), 800, t.schedule, t.cancel);

    s.removeItem("kataribe.msgColor");
    m.onChange("kataribe.msgColor");
    m.flush(); // beforeunload 相当 — タイマーを待たず今すぐ書く
    expect(saved).toHaveLength(1);
    expect(JSON.parse(saved[0])).toEqual({ "kataribe.theme": "dark" }); // 消えたキーは載らない
    m.flush(); // 待ちが無ければ何もしない
    expect(saved).toHaveLength(1);
  });
});
