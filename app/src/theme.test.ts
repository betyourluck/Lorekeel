/**
 * テーマ配色の可読性テスト (2026-09-08、ユーザー報告「ライトで文字の色が薄い」)。
 *
 * 守るのは**主張そのもの**: 「同じ `text-parchment/40` が、ライトでもダークと同等に読める」。
 * 見た目は測れないがコントラスト比は配色の実値から決まるので、提示層でも機械が検算できる
 * (`editorPath` / `tour` と同じ「DOM に依らない純粋関数」の枠)。
 *
 * 素材は**実際にビルドされた CSS** から採る — `?inline` は postcss + tailwind を通った後の
 * 文字列なので、パレット (main.css の変数) と生成されたユーティリティ (減光カーブが実際に
 * 掛かっているか) を同時に見られる。設定ファイルの字面ではなく**出荷される物**を測る。
 * (vitest は既定で CSS を空に潰すので `test.css = true` を vite.config.ts で入れてある。)
 *
 * 検査する alpha は .vue が実際に書いている値を集める。表を手で置くと、次に誰かが
 * `text-parchment/15` を足したときに黙って射程から外れる。
 */
import { describe, expect, it } from "vitest";
import compiledCss from "./assets/main.css?inline";

const vueSources = import.meta.glob("./**/*.vue", { query: "?raw", import: "default", eager: true }) as Record<
  string,
  string
>;

type Rgb = [number, number, number];

/** `marker` を含むセレクタの宣言ブロックを返す。 */
function block(marker: string): string {
  const at = compiledCss.indexOf(marker);
  expect(at, `${marker} が CSS に無い`).toBeGreaterThan(-1);
  const open = compiledCss.indexOf("{", at);
  const close = compiledCss.indexOf("}", open);
  return compiledCss.slice(open, close);
}
function readVar(src: string, name: string): string {
  const m = src.match(new RegExp(`--${name}:\s*([^;]+);`));
  if (!m) throw new Error(`--${name} が見つからない`);
  return m[1].trim();
}
function readRgb(src: string, name: string): Rgb {
  const parts = readVar(src, name).split(/\s+/).map(Number);
  expect(parts, `--${name}`).toHaveLength(3);
  return parts as Rgb;
}

const linear = (c: number) => {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
};
const luminance = ([r, g, b]: Rgb) => 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
/** WCAG コントラスト比 (1〜21)。 */
function contrast(fg: Rgb, bg: Rgb): number {
  const [hi, lo] = [luminance(fg), luminance(bg)].sort((a, b) => b - a);
  return (hi + 0.05) / (lo + 0.05);
}
/** alpha 合成。文字は背景の上に載るので、減光は必ず背景の色へ寄る。 */
const over = (fg: Rgb, bg: Rgb, a: number): Rgb => [0, 1, 2].map((i) => a * fg[i] + (1 - a) * bg[i]) as Rgb;

/** .vue が実際に書いている文字色の alpha 修飾子 (`text-parchment/40` 等)。 */
function usedTextAlphas(): number[] {
  const found = new Set<number>();
  for (const src of Object.values(vueSources)) {
    for (const m of src.matchAll(/\b(?:text|placeholder)-parchment\/(\d+)\b/g)) found.add(Number(m[1]) / 100);
  }
  return [...found].sort((a, b) => a - b);
}

const dark = block('[data-theme="dark"]');
const light = block('[data-theme="light"]');
const curveOf = (src: string) => ({
  floor: Number(readVar(src, "text-alpha-floor")),
  span: Number(readVar(src, "text-alpha-span")),
});
const apply = (c: { floor: number; span: number }, a: number) => Math.min(1, c.floor + c.span * a);

describe("文字色の減光カーブ", () => {
  it("ダークは恒等 = 従来の描画に一切触れない", () => {
    const c = curveOf(dark);
    expect(c.floor).toBe(0);
    expect(c.span).toBe(1);
    for (const a of usedTextAlphas()) expect(apply(c, a)).toBeCloseTo(a, 10);
  });

  it("修飾子なしの本文は両テーマとも不透明のまま", () => {
    for (const src of [dark, light]) expect(apply(curveOf(src), 1)).toBe(1);
  });

  it("ライトの減光文字がダークと同等以上のコントラストになる", () => {
    const alphas = usedTextAlphas();
    expect(alphas.length, "alpha の抽出が壊れている").toBeGreaterThan(5);
    const c = curveOf(light);
    // 地は 2 種類: 素の背景と、パネル (bg-ash) の上。
    for (const ground of ["ink", "ash"] as const) {
      const dBg = readRgb(dark, ground);
      const lBg = readRgb(light, ground);
      const dFg = readRgb(dark, "parchment");
      const lFg = readRgb(light, "parchment");
      for (const a of alphas) {
        const d = contrast(over(dFg, dBg, a), dBg);
        const l = contrast(over(lFg, lBg, apply(c, a)), lBg);
        // 5% の緩みは、カーブが線形近似 (厳密には a^0.7) であることの許容。
        expect(l, `text-parchment/${a * 100} on bg-${ground}`).toBeGreaterThanOrEqual(d * 0.95);
      }
    }
  });

  it("カーブは文字だけに掛かる (面の色は素の alpha のまま)", () => {
    // 出荷される CSS で確かめる — 設定の書き方ではなく、生成されたユーティリティを見る。
    const rule = (sel: string) => {
      const at = compiledCss.indexOf(sel);
      expect(at, `${sel} が生成されていない`).toBeGreaterThan(-1);
      return compiledCss.slice(at, compiledCss.indexOf("}", at));
    };
    expect(rule(String.raw`.text-parchment\/40`)).toContain("--text-alpha-floor");
    expect(rule(String.raw`.placeholder-parchment\/30`)).toContain("--text-alpha-floor");
    expect(rule(String.raw`.bg-parchment\/30`)).not.toContain("--text-alpha-floor");
  });
});

describe("ライトのアクセント色", () => {
  const bg = readRgb(light, "ink");
  it("文字として使う色が小さい文字の AA (4.5) に乗る", () => {
    for (const name of ["parchment", "ember", "glow", "warn", "ok"] as const) {
      expect(contrast(readRgb(light, name), bg), `text-${name}`).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("主ボタン (bg-ember/80 の上の ink 文字) が 3:1 を超える", () => {
    // ember は面としても文字としても使うが、濃くすると**両方**が良くなる向きなので衝突しない。
    const face = over(readRgb(light, "ember"), bg, 0.8);
    expect(contrast(bg, face)).toBeGreaterThanOrEqual(3);
  });
});

describe("初回ナビゲーション", () => {
  it("幕と札に暗色の直書きが残っていない", () => {
    const tour = vueSources["./components/FirstRunTour.vue"];
    expect(tour, "FirstRunTour.vue が読めていない").toBeTruthy();
    // コメントは剥がす — 検査したいのは宣言であって、経緯を説明する散文ではない。
    const styles = tour.slice(tour.indexOf("<style")).replace(/\/\*[\s\S]*?\*\//g, "");
    // ダークのインク値をそのまま書くと、ライトで「暗い幕に暗い字」に戻る (2026-09-08 の実害)。
    expect([...styles.matchAll(/rgb\(\s*21\s+17\s+14/g)]).toHaveLength(0);
  });
});
