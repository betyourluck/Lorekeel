<script setup lang="ts">
/**
 * プロンプト工房 (spec 27 Phase D、2026-08-24 に浮遊パネルへ)。
 *
 * 上段 = **最終送信文字列**（スタイル原文 + プロンプト書きの出力の合成結果）。ここを直して
 * 「そのまま生成」を押すと `prompt_override` として **verbatim** で送る — 見えているものが
 * 送られるもの。下段 = この一枚への指示（LLM 経由・**揮発**）。
 *
 * CodeMirror は動的 import（設定ダイアログと同じ規律 = 開かないセッションでは読まない）。
 */
import { computed, defineAsyncComponent, ref, watch } from "vue";
import { useGameStore } from "../stores/game";
import { t } from "../i18n";
import FloatingPanel from "./FloatingPanel.vue";

const CodeEditor = defineAsyncComponent(() => import("./CodeEditor.vue"));

const game = useGameStore();
const emit = defineEmits<{ (e: "close"): void }>();

const draft = ref(game.generatedImage?.prompt ?? "");
watch(
  () => game.generatedImage?.prompt,
  (p) => {
    if (p) draft.value = p;
  },
);
const dirty = computed(() => draft.value.trim() !== (game.generatedImage?.prompt ?? "").trim());
</script>

<template>
  <FloatingPanel :title="t('promptWorkshop.heading')" width="42rem" @close="emit('close')">
    <div class="space-y-4">
      <div>
        <div class="mb-1 flex items-baseline justify-between">
          <span class="font-serif text-sm text-parchment/80">{{ t("promptWorkshop.final") }}</span>
          <span v-if="dirty" class="text-[10px] tracking-wide text-ember">{{ t("promptWorkshop.edited") }}</span>
        </div>
        <p class="mb-2 text-[11px] leading-relaxed text-parchment/45">{{ t("promptWorkshop.finalHint") }}</p>
        <CodeEditor v-model="draft" language="text" height="10rem" :placeholder="t('promptWorkshop.finalPlaceholder')" />
      </div>

      <label class="block">
        <span class="font-serif text-sm text-parchment/80">{{ t("promptWorkshop.direction") }}</span>
        <p class="mb-2 text-[11px] leading-relaxed text-parchment/45">{{ t("promptWorkshop.directionHint") }}</p>
        <textarea
          :value="game.imageDirection"
          rows="2"
          :placeholder="t('promptWorkshop.directionPlaceholder')"
          class="block w-full resize-none rounded-lg bg-parchment/5 px-3 py-2 text-sm text-parchment
                 ring-1 ring-parchment/10 placeholder:text-parchment/30 focus:outline-none focus:ring-ember/50"
          @input="game.imageDirection = ($event.target as HTMLTextAreaElement).value"
        ></textarea>
      </label>

      <div class="flex items-center justify-end gap-2 pt-1">
        <span v-if="game.imageBusy" class="mr-auto text-[11px] text-parchment/50">{{ t("image.generating") }}</span>
        <button
          class="rounded-full px-3 py-1 text-xs text-parchment/70 ring-1 ring-parchment/15 hover:text-parchment hover:ring-parchment/40
                 transition-colors disabled:opacity-40"
          :disabled="game.imageBusy || !game.started"
          @click="game.generateImage()"
        >
          {{ t("promptWorkshop.rewrite") }}
        </button>
        <button
          class="rounded-full bg-ember/85 px-4 py-1 text-xs font-bold text-ink shadow-[0_0_14px_rgb(var(--ember)/0.35)]
                 hover:bg-ember transition-colors disabled:opacity-40 disabled:shadow-none"
          :disabled="game.imageBusy || !game.started || !draft.trim()"
          @click="game.generateImage(draft)"
        >
          {{ t("promptWorkshop.sendVerbatim") }}
        </button>
      </div>
    </div>
  </FloatingPanel>
</template>
