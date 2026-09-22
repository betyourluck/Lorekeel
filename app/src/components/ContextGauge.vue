<script setup lang="ts">
/**
 * コンテキストウィンドウの使用率 (2026-09-23、利用者要望「今どのくらい使っているのかわかりづらい」)。
 *
 * **警告の道具ではなく「どれだけ余っているか」の可視化。** 実測では muse-spark 14,123/1,048,576
 * = 1.3%・opus 30,549/1,000,000 = 3.1% で、**輪はほぼ空のまま**になる。だから数値を主・輪を従に
 * 置く (輪だけでは 1.3% が視認できない)。色の 3 段は上側に近づいたときのためで、普段は熾火の控えめ。
 *
 * 分母が無い登録モデルでは **% を出さない** — 既定値を番兵に置くと「実は 1M の窓なのに 23%」という
 * 嘘になる (prices.ts の「候補が複数あり食い違えば埋めない」と同じ規律)。
 */
import { computed, ref, watch } from "vue";

import { loadAiProfiles } from "../aiProfiles";
import { contextUsage, formatPercent, formatTokens, windowFor } from "../contextGauge";
import { t } from "../i18n";
import { useGameStore } from "../stores/game";

const game = useGameStore();
// 登録簿は localStorage 側。モデルが変わったとき (= 設定を保存したとき) に読み直せば足りる。
const profiles = ref(loadAiProfiles());
watch(() => game.llmModel, () => (profiles.value = loadAiProfiles()));

const windowTokens = computed(() => windowFor(game.llmModel, profiles.value));
const usage = computed(() => contextUsage(game.lastPrompt, windowTokens.value));

/** 輪の周長 (r=6)。使用分だけ dashoffset を詰める。 */
const R = 6;
const C = 2 * Math.PI * R;

const toneClass = computed(() => {
  switch (usage.value?.tone) {
    case "critical":
      return "text-warn font-semibold";
    case "warn":
      // 中段は glow (炎の明 = 黄橙)。warn を薄めた色だと critical と見分けが付かない
      // (実機で確認: どちらも rgb(217,100,90) になり差が alpha だけだった)。
      return "text-glow";
    default:
      return "text-ember/70";
  }
});

const title = computed(() =>
  usage.value
    ? t("action.contextTitle", {
        used: formatTokens(usage.value.used),
        window: formatTokens(usage.value.window),
        pct: formatPercent(usage.value.ratio),
      })
    : t("action.contextNoWindow"),
);
</script>

<template>
  <div
    v-if="usage"
    class="flex items-center gap-1.5 text-[10px] tabular-nums select-none"
    :title="title"
  >
    <svg viewBox="0 0 16 16" class="h-3.5 w-3.5 -rotate-90" aria-hidden="true">
      <circle cx="8" cy="8" :r="R" fill="none" stroke="currentColor" stroke-width="2" class="text-parchment/15" />
      <circle
        cx="8"
        cy="8"
        :r="R"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        :stroke-dasharray="C"
        :stroke-dashoffset="C * (1 - usage.ratio)"
        :class="toneClass"
      />
    </svg>
    <span class="text-parchment/45">{{ formatTokens(usage.used) }} / {{ formatTokens(usage.window) }}</span>
    <span :class="toneClass">{{ formatPercent(usage.ratio) }}</span>
  </div>
</template>
