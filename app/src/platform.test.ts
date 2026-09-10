/**
 * OS 判別の純関数テスト。実 UA の形で固定する (手で縮めた文字列だけを見ると、
 * 実機の UA が変わったときに気づけない)。
 */
import { describe, expect, it } from "vitest";
import { isMacOs } from "./platform";

const MAC_WKWEBVIEW =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";
const WINDOWS_WEBVIEW2 =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36 Edg/128.0.0.0";
const LINUX_WEBKITGTK =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

describe("isMacOs", () => {
  it("macOS の WebView だけ真になる", () => {
    expect(isMacOs(MAC_WKWEBVIEW)).toBe(true);
    expect(isMacOs(WINDOWS_WEBVIEW2)).toBe(false);
    expect(isMacOs(LINUX_WEBKITGTK)).toBe(false);
  });

  it("空文字でも落ちない (UA が取れない環境は Mac でない側へ倒す)", () => {
    expect(isMacOs("")).toBe(false);
  });
});
