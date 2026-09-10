/**
 * 動いている OS の判別 (提示層の見た目を OS の作法に合わせるため)。
 *
 * **UA を見るのは、同期で決まるから** — Tauri の command で聞くと 1 往復ぶん遅れ、
 * その間だけ Windows 風の並びが見えてから Mac 風に入れ替わる (起動のたびにちらつく)。
 * タイトルバーは最初の 1 フレームから正しい形で描きたいので、判定は module load 時に済ませる。
 *
 * 判定そのものは純関数にしてある (提示層で唯一テストできる部分。`tour.ts` / `map.ts` と同じ枠)。
 */

/** macOS の WebView か (WKWebView の UA は "Macintosh; Intel Mac OS X …" を含む)。 */
export function isMacOs(ua: string): boolean {
  return /Macintosh|Mac OS X/i.test(ua);
}

/** 起動時に一度だけ決める (以後は変わらない)。Tauri の外 (ブラウザ) でも動く。 */
export const IS_MAC = typeof navigator !== "undefined" && isMacOs(navigator.userAgent);
