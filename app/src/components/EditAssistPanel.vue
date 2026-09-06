<script setup lang="ts">
/**
 * AI 編集の浮遊パネル (spec 29 Phase D)。プロンプト工房と同じ器 (FloatingPanel = 幕を張らない
 * ガラス) をエディタの上に置く — 直された本文を見ながら指示を打つ作業なので、暗幕で
 * エディタを覆わない。
 *
 * 上段 = この 1 依頼の指示 (自由文・**揮発** = 保存しない)。実行中は進行ログ (backend の
 * `edit-assist-progress` を 1 呼び出し 1 行) が流れ、終わると報告文・周回とトークン・
 * 残った診断が出る。本文の差し替えは store (`runEditAssist`) が CodeMirror へ**未保存**として
 * 入れる (undo 1 回で戻る) — ここは表示だけ。
 */
import { computed, ref } from "vue";

import { t } from "../i18n";
import { useGameStore } from "../stores/game";
import FloatingPanel from "./FloatingPanel.vue";
import Icon from "./Icon.vue";

const game = useGameStore();
const emit = defineEmits<{ (e: "close"): void }>();
const box = ref<HTMLTextAreaElement | null>(null);

const assist = computed(() => game.editor.assist);
const canRun = computed(
  () => !!game.editor.current && !assist.value.running && assist.value.instruction.trim().length > 0,
);
const stoppedText = computed(() => {
  const s = assist.value.result?.stopped;
  if (!s) return "";
  return t(`editAssist.stopped_${s}`);
});

function onKey(e: KeyboardEvent) {
  // Ctrl+Enter で実行 (Enter は改行 — 指示は複数行で書ける)。
  if ((e.ctrlKey || e.metaKey) && e.key === "Enter" && canRun.value) {
    e.preventDefault();
    void game.runEditAssist();
  }
}
</script>

<template>
  <FloatingPanel :title="t('editAssist.heading')" width="40rem" @close="emit('close')">
    <div class="space-y-3">
      <p class="text-[11px] leading-relaxed text-parchment/45">{{ t("editAssist.hint") }}</p>
      <textarea
        ref="box"
        v-model="assist.instruction"
        class="w-full h-24 rounded-lg bg-black/30 p-2 text-sm text-parchment outline-none ring-1 ring-parchment/10 focus:ring-ember/50 disabled:opacity-50"
        :placeholder="t('editAssist.placeholder')"
        :disabled="assist.running"
        @keydown="onKey"
      />
      <div class="flex items-center gap-2">
        <button
          class="flex items-center gap-1.5 rounded-lg bg-ember/80 px-3 py-1.5 text-sm text-ink hover:bg-ember disabled:opacity-40"
          :disabled="!canRun"
          @click="game.runEditAssist()"
        >
          <Icon :name="assist.running ? 'spinner' : 'sparkle'" :size="14" />
          {{ assist.running ? t("editAssist.running") : t("editAssist.run") }}
        </button>
        <button
          v-if="assist.running"
          class="rounded-lg px-3 py-1.5 text-sm text-parchment/70 ring-1 ring-parchment/20 hover:bg-parchment/10"
          @click="game.cancelEditAssist()"
        >
          {{ t("editAssist.cancel") }}
        </button>
        <span class="flex-1" />
        <span class="text-[10px] text-parchment/40">{{ t("editAssist.shortcut") }}</span>
      </div>

      <!-- 進行ログ: 1 呼び出し 1 行。実行中は最後の行が生きている合図。 -->
      <div
        v-if="assist.log.length"
        class="max-h-40 overflow-y-auto rounded-lg bg-black/30 p-2 font-mono text-[11px] leading-relaxed text-parchment/70"
      >
        <div v-for="(line, i) in assist.log" :key="i" class="whitespace-pre-wrap break-all">{{ line }}</div>
      </div>

      <p v-if="assist.error" class="text-xs text-red-300 whitespace-pre-wrap">{{ assist.error }}</p>

      <div v-if="assist.result" class="space-y-2 rounded-lg bg-black/20 p-3 text-xs">
        <p class="whitespace-pre-wrap text-parchment/85">{{ assist.result.summary }}</p>
        <p class="text-ember/90" v-if="assist.result.changed">{{ t("editAssist.replaced") }}</p>
        <p class="text-parchment/50" v-else>{{ t("editAssist.unchanged") }}</p>
        <p v-if="stoppedText" class="text-amber-200/80">{{ stoppedText }}</p>
        <ul v-if="assist.result.diagnostics.length" class="space-y-0.5">
          <li
            v-for="(d, i) in assist.result.diagnostics"
            :key="i"
            :class="d.severity === 'error' ? 'text-red-300' : 'text-amber-200/80'"
          >
            [{{ d.severity }}]<template v-if="d.line"> L{{ d.line }}</template><template v-else-if="d.path"> {{ d.path }}</template> {{ d.message }}
          </li>
        </ul>
        <p class="text-[10px] text-parchment/40">
          {{
            t("editAssist.stats", {
              model: assist.result.model,
              iterations: assist.result.iterations,
              prompt: assist.result.prompt_tokens,
              completion: assist.result.completion_tokens,
              cached: assist.result.cache_read,
            })
          }}
        </p>
      </div>
    </div>
  </FloatingPanel>
</template>
