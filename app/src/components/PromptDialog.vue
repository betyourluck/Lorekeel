<script setup lang="ts">
/**
 * プロンプト工房 (spec 27 Phase D)。
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

const CodeEditor = defineAsyncComponent(() => import("./CodeEditor.vue"));

const game = useGameStore();
const emit = defineEmits<{ (e: "close"): void }>();

/** 編集中の最終プロンプト。開いた時点の送信文字列を種にする。 */
const draft = ref(game.generatedImage?.prompt ?? "");
// 生成が終わるたびに種を入れ替える (書き直させた結果をそのまま手で直せる)。
watch(
  () => game.generatedImage?.prompt,
  (p) => {
    if (p) draft.value = p;
  },
);

const dirty = computed(() => draft.value.trim() !== (game.generatedImage?.prompt ?? "").trim());

/** LLM に書き直させる (direction が効く。手書きは渡さない)。 */
function rewrite() {
  game.generateImage();
}
/** 手書きを verbatim で送る (プロンプト書きを呼ばない)。 */
function sendVerbatim() {
  game.generateImage(draft.value);
}
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60" @click.self="emit('close')">
    <div class="w-[46rem] max-h-[85vh] overflow-y-auto rounded-lg bg-ink ring-1 ring-ash p-5 space-y-4">
      <div class="flex items-center justify-between">
        <h2 class="text-lg font-bold text-parchment">{{ t("promptWorkshop.heading") }}</h2>
        <button class="text-parchment/50 hover:text-parchment" @click="emit('close')">✕</button>
      </div>

      <div>
        <div class="flex items-baseline justify-between">
          <span class="text-sm text-parchment/70">{{ t("promptWorkshop.final") }}</span>
          <span v-if="dirty" class="text-[11px] text-ember">{{ t("promptWorkshop.edited") }}</span>
        </div>
        <p class="text-[11px] text-parchment/40 mb-1">{{ t("promptWorkshop.finalHint") }}</p>
        <CodeEditor v-model="draft" language="text" height="11rem" :placeholder="t('promptWorkshop.finalPlaceholder')" />
      </div>

      <label class="block">
        <span class="text-sm text-parchment/70">{{ t("promptWorkshop.direction") }}</span>
        <p class="text-[11px] text-parchment/40 mb-1">{{ t("promptWorkshop.directionHint") }}</p>
        <textarea
          :value="game.imageDirection"
          rows="3"
          :placeholder="t('promptWorkshop.directionPlaceholder')"
          class="block w-full rounded bg-ash/40 px-2 py-1 text-sm text-parchment focus:outline-none"
          @input="game.imageDirection = ($event.target as HTMLTextAreaElement).value"
        ></textarea>
      </label>

      <div class="flex items-center gap-2 border-t border-ash/50 pt-3">
        <button
          class="rounded bg-ash/40 hover:bg-ash/70 px-3 py-1 text-sm text-parchment/80 disabled:opacity-40"
          :disabled="game.imageBusy || !game.started"
          @click="rewrite"
        >
          {{ t("promptWorkshop.rewrite") }}
        </button>
        <button
          class="rounded bg-ember/80 hover:bg-ember px-3 py-1 text-sm font-bold text-ink disabled:opacity-40"
          :disabled="game.imageBusy || !game.started || !draft.trim()"
          @click="sendVerbatim"
        >
          {{ t("promptWorkshop.sendVerbatim") }}
        </button>
        <span v-if="game.imageBusy" class="text-xs text-parchment/50">{{ t("image.generating") }}</span>
      </div>
    </div>
  </div>
</template>
