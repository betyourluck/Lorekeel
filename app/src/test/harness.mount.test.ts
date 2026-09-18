/**
 * マウントテストの土台そのものを固定する (spec 31 Phase 0)。
 *
 * ここで守るのは「部品のテストが何を信じてよいか」:
 * - この project は本当に DOM の上で走っている
 * - 偽装していない command は例外で落ちる / イベントの内部 command は落ちない (決定 3 の二つの主張)
 * - ウィンドウ系はコールバックへ届く = 使うテストが表に書く必要がある
 * - happy-dom の素のメディア API がどう振る舞うか (スタブにする理由の記録)
 * - 後始末が IPC・localStorage・スタブを実際に戻す
 */
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { describe, expect, it, vi } from "vitest";

import HelpNote from "../components/HelpNote.vue";
import { allowUnmocked, ipcCalls, mountWith, prepare, teardown, unmockedMessage } from "./mount";

describe("環境", () => {
  it("mount project は DOM の上で走る", () => {
    expect(typeof document).toBe("object");
    expect(typeof window.localStorage.getItem).toBe("function");
  });

  it("happy-dom の素の Audio.play() は例外を投げず解決する — ただし mediaDevices は存在しない", async () => {
    // 準備 (prepare) の前に測る = スタブの掛かっていない素の挙動。
    // スタブにする理由は「play が落ちるから」ではなく、①呼んだかをスパイで見るため
    // ②マイク一覧を読む部品 (SettingsDialog の refreshMicDevices) が mediaDevices の不在で落ちないため。
    const audio = new Audio("x.ogg");
    await expect(audio.play()).resolves.toBeUndefined();
    expect("mediaDevices" in navigator).toBe(false);
  });

  it("準備のあとは play がスパイになり、mediaDevices が生える", async () => {
    prepare();
    const audio = new Audio("x.ogg");
    await audio.play();
    expect(HTMLMediaElement.prototype.play).toHaveBeenCalledTimes(1);
    await expect(navigator.mediaDevices.enumerateDevices()).resolves.toEqual([]);
  });
});

describe("IPC の偽装 (決定 3)", () => {
  it("表に無い command は例外で落ちる", async () => {
    prepare();
    allowUnmocked(); // 偽装の性質そのものを見るテストなので、後始末の検出からは外す
    await expect(invoke("no_such_command")).rejects.toThrow(unmockedMessage("no_such_command"));
  });

  it("表に在る command は返り値と引数が通る", async () => {
    prepare({ echo: (args) => ({ got: args?.value }) });
    await expect(invoke("echo", { value: 3 })).resolves.toEqual({ got: 3 });
    expect(ipcCalls).toEqual([{ cmd: "echo", args: { value: 3 } }]);
  });

  it("listen / emit は偽装の表が空でも落ちない (plugin:event|* はコールバックより前に処理される)", async () => {
    prepare();
    const seen: unknown[] = [];
    const unlisten = await listen<string>("synopsis-compacting", (e) => seen.push(e.payload));
    await emit("synopsis-compacting", "hello");
    expect(seen).toEqual(["hello"]);
    unlisten();
    // イベントの内部 command は利用者のコールバックへ届いていない
    expect(ipcCalls.filter((c) => c.cmd.startsWith("plugin:event|"))).toEqual([]);
  });

  it("ウィンドウ系はコールバックへ届く — 表に無ければ落ち、在れば通る", async () => {
    prepare();
    allowUnmocked();
    await expect(getCurrentWindow().setTitle("t")).rejects.toThrow(/unmocked command: plugin:window\|/);

    teardown();
    const setTitle = vi.fn(() => null);
    prepare({ "plugin:window|set_title": setTitle });
    // setTitle は偽装の返り値をそのまま返す (ここでは null)
    await expect(getCurrentWindow().setTitle("t")).resolves.toBeNull();
    expect(setTitle).toHaveBeenCalledTimes(1);
  });
});

describe("後始末", () => {
  it("IPC の偽装・localStorage・スタブを戻す", async () => {
    prepare({ echo: () => 1 });
    localStorage.setItem("kataribe.lang", "en");
    teardown();
    expect(localStorage.getItem("kataribe.lang")).toBeNull();
    expect(vi.isMockFunction(HTMLMediaElement.prototype.play)).toBe(false);
    expect("mediaDevices" in navigator).toBe(false);
    // 表が空に戻る = さっきまで在った command も偽装し忘れと同じく落ちる
    // (Tauri の内部は消さない: transport.ts が import 時に登録した購読を生かすため)
    allowUnmocked();
    await expect(invoke("echo")).rejects.toThrow(unmockedMessage("echo"));
  });

  it("表に無い command は、呼び出し側が握り潰していても後始末で落ちる (Phase B)", async () => {
    prepare();
    // SettingsDialog のローダーと同じ形 = 失敗を catch で捨てる
    try {
      await invoke("get_forgotten");
    } catch {
      /* 画面は空欄で出る */
    }
    expect(() => teardown()).toThrow(/偽装の表に無い command が呼ばれた.*get_forgotten/);
    // 一度落としたら記録は空に戻る (次のテストへ持ち越さない)
    expect(() => teardown()).not.toThrow();
  });
});

describe("煙のテスト (store も IPC も使わない最小の部品)", () => {
  it("HelpNote は既定で畳まれ、押すと開き、もう一度押すと閉じる", async () => {
    const wrapper = mountWith(HelpNote, { slots: { default: "本文です" } });
    const button = wrapper.get("button");
    expect(button.attributes("aria-expanded")).toBe("false");
    expect(wrapper.text()).not.toContain("本文です");

    await button.trigger("click");
    expect(button.attributes("aria-expanded")).toBe("true");
    expect(wrapper.text()).toContain("本文です");

    await button.trigger("click");
    expect(wrapper.text()).not.toContain("本文です");
  });

  it("open を渡すと開いた状態で立つ", () => {
    const wrapper = mountWith(HelpNote, { props: { open: true }, slots: { default: "本文です" } });
    expect(wrapper.text()).toContain("本文です");
  });
});
