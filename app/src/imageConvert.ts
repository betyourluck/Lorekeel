/**
 * ローカル画像 → WebP (spec 27 追補、2026-08-24)。
 *
 * **変換は WebView (Chromium) で行う** — `image` crate は足さない (spec 27 スコープ外の線は
 * 保つ)。Chromium は `canvas.toBlob("image/webp", q)` で lossy WebP を**ネイティブに**書けるので、
 * Rust 側に libwebp (C ビルド・3 OS の CI) を持ち込まずに済む。ついでに長辺を詰める —
 * スマホ写真 (4000px・数 MB) をそのまま参照に入れると 8MB の上限と送信合計 12MB に当たる上、
 * 画像モデル側の入力解像度 (1024〜1536) を超えた分は捨てられるだけ。
 *
 * 返すのは bytes だけ。書く先 (枠) は backend の `put_reference_bytes` が raw body で受ける。
 */

/** 長辺の上限。Shape の最大辺 (1536) に合わせる — それ以上は参照としても使われない。 */
export const REF_MAX_SIDE = 1536;
/** lossy WebP の品質。0.88 で写真 1536px が概ね 150〜400KB。 */
export const REF_WEBP_QUALITY = 0.88;

/** 縮小後の寸法 (純関数)。長辺が `maxSide` 以下ならそのまま。 */
export function fitWithin(w: number, h: number, maxSide = REF_MAX_SIDE): { w: number; h: number } {
  const long = Math.max(w, h);
  if (long <= maxSide) return { w, h };
  const k = maxSide / long;
  return { w: Math.max(1, Math.round(w * k)), h: Math.max(1, Math.round(h * k)) };
}

/**
 * File → WebP bytes。デコードできない (対応外の形式・壊れたファイル) は reject。
 * `toBlob` が WebP を返せない環境 (非 Chromium) では `image/png` にフォールバックする —
 * backend は先頭バイトで mime を嗅ぎ分けるので、どちらでも正しい拡張子で入る。
 */
export async function fileToWebp(
  file: File,
  maxSide = REF_MAX_SIDE,
  quality = REF_WEBP_QUALITY,
): Promise<Uint8Array> {
  const bitmap = await createImageBitmap(file);
  try {
    const { w, h } = fitWithin(bitmap.width, bitmap.height, maxSide);
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("canvas 2d context unavailable");
    ctx.drawImage(bitmap, 0, 0, w, h);
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/webp", quality));
    const out = blob && blob.type === "image/webp" ? blob : await new Promise<Blob | null>((r) => canvas.toBlob(r, "image/png"));
    if (!out) throw new Error("encode failed");
    return new Uint8Array(await out.arrayBuffer());
  } finally {
    bitmap.close();
  }
}
