/** @type {import('tailwindcss').Config} */

/*
 * 文字色の減光カーブ (2026-09-08)。
 *
 * `text-parchment/40` のような alpha 修飾子は、**同じ数字でもテーマによって別の強さになる**。
 * 減光は背景へ向かう合成なので、暗い背景では輝度が急に落ち、明るい背景では落ちない —
 * 実測 (パレットの実値・WCAG 比) で `/40` はダーク 3.14 / ライト 2.34、`/30` は 2.29 / 1.85。
 * しかも **`--parchment` を黒にしてもライトの `/40` は 2.65 が上限**なので、パレットの調整では
 * 原理的に届かない (合成の 60% が背景の明るさで埋まる)。
 *
 * ゆえに alpha 自体をテーマごとに写す: `a' = floor + span * a`。
 * ダークは floor=0 / span=1 = **恒等** (従来の描画と数値まで同一)、ライトは 0.30 + 0.70a で
 * ダークと同等のコントラストに乗る (0.4→0.58 で 3.78、0.7→0.79 で 7.18)。修飾子なしは
 * `<alpha-value>` が 1 に置換され calc(0.3 + 0.7) = 1 なので、通常の本文は両テーマとも不透明。
 *
 * **文字だけに掛ける** — 面 (bg-parchment/10 の下地・ring/border) に同じ curve を当てると
 * 薄い色付けが濃い帯になるので、`textColor` と `placeholderColor` のスケールにだけ差し込む。
 */
const dimText = (v) =>
  `rgb(var(${v}) / calc(var(--text-alpha-floor) + var(--text-alpha-span) * <alpha-value>))`;

export default {
  content: ["./index.html", "./src/**/*.{vue,ts}"],
  theme: {
    extend: {
      colors: {
        // 「語り部」: 暖炉の残り火を思わせる暖色テーマ。値は CSS 変数 (main.css) で定義し
        // data-theme (ライト/ダーク) で入れ替える。<alpha-value> で opacity 修飾子 (bg-ash/30 等) を保つ。
        ink: "rgb(var(--ink) / <alpha-value>)",             // 背景
        parchment: "rgb(var(--parchment) / <alpha-value>)", // 本文
        ember: "rgb(var(--ember) / <alpha-value>)",         // アクセント (熾火)
        ash: "rgb(var(--ash) / <alpha-value>)",             // 罫線・パネル
        glow: "rgb(var(--glow) / <alpha-value>)",           // 強調 (炎の明)
        warn: "rgb(var(--warn) / <alpha-value>)",           // 注意 (自己修復の ⚠ 等)
        ok: "rgb(var(--ok) / <alpha-value>)",               // 成功・接続 (卓の ● 等)
      },
      // 文字として使う色は減光カーブを通す (上のコメント)。ink/ash/ok は面か点なので素のまま。
      textColor: {
        parchment: dimText("--parchment"),
        ember: dimText("--ember"),
        glow: dimText("--glow"),
        warn: dimText("--warn"),
      },
      placeholderColor: {
        parchment: dimText("--parchment"),
      },
      fontFamily: {
        serif: ['"Noto Serif JP"', "serif"],
      },
    },
  },
  plugins: [],
};
