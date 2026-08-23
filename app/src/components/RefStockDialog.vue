<script setup lang="ts">
/**
 * 参照ストックの入れ替えダイアログ (spec 27 Phase D)。
 *
 * 枠は `MAX_REFS` から生成する (3 固定で組まない = プロバイダ別に割れたとき UI が追従する)。
 * **枠番号 = ファイル名番号 = ワイヤ番号** が backend の前詰め不変条件で保証されているので、
 * ここは番号の対応を気にせず現物を並べるだけでよい。
 *
 * サムネイルは `asset://` (`convertFileSrc`) — data URL を IPC に流さない。
 */
import { computed, onMounted } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useGameStore } from "../stores/game";
import { t } from "../i18n";
import Icon from "./Icon.vue";

const game = useGameStore();
const emit = defineEmits<{ (e: "close"): void }>();

onMounted(() => game.loadRefStock());

/** 枠 1..max。埋まっていれば現物、空なら null。 */
const slots = computed(() => {
  const stock = game.refStock;
  const max = stock?.max ?? 3;
  return Array.from({ length: max }, (_, i) => {
    const entry = stock?.picked[i];
    return {
      slot: i + 1,
      name: entry?.[0] ?? null,
      size: entry?.[1] ?? 0,
      url: entry && stock ? convertFileSrc(`${stock.dir}/${entry[0]}`) : null,
    };
  });
});

/** 空きは「末尾の 1 つ」だけが押せる (前詰め不変条件を UI 側でも壊さない)。 */
const nextEmpty = computed(() => slots.value.find((s) => !s.name)?.slot ?? null);
const canPut = computed(() => !!game.generatedImage);
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60" @click.self="emit('close')">
    <div class="w-[42rem] max-h-[85vh] overflow-y-auto rounded-lg bg-ink ring-1 ring-ash p-5 space-y-4">
      <div class="flex items-center justify-between">
        <h2 class="text-lg font-bold text-parchment">{{ t("refStock.heading") }}</h2>
        <button class="text-parchment/50 hover:text-parchment" @click="emit('close')">✕</button>
      </div>
      <p class="text-xs text-parchment/50">{{ t("refStock.intro") }}</p>

      <div class="grid grid-cols-3 gap-3">
        <div v-for="s in slots" :key="s.slot" class="space-y-1">
          <div
            class="relative aspect-square rounded border border-ash/70 bg-ash/20 overflow-hidden flex items-center justify-center"
          >
            <img v-if="s.url" :src="s.url" class="h-full w-full object-contain" :alt="s.name ?? ''" />
            <span v-else class="text-parchment/25 text-xs">{{ t("refStock.empty") }}</span>
            <span class="absolute top-1 left-1 rounded bg-ink/70 px-1.5 text-[10px] text-parchment/60">
              {{ s.slot }}
            </span>
          </div>
          <div class="flex items-center gap-1">
            <!-- 入れ替え: 埋まっている枠は置き換え、空きは末尾の 1 つだけ (前詰めを壊さない)。 -->
            <button
              class="flex-1 rounded bg-ash/40 hover:bg-ash/70 px-2 py-1 text-xs text-parchment/80 disabled:opacity-30"
              :disabled="!canPut || (!s.name && s.slot !== nextEmpty)"
              :title="canPut ? t('refStock.put') : t('refStock.needImage')"
              @click="game.putRefSlot(s.slot)"
            >
              {{ s.name ? t("refStock.replace") : t("refStock.put") }}
            </button>
            <button
              v-if="s.name"
              class="rounded bg-ash/40 hover:bg-ash/70 px-2 py-1 text-xs text-parchment/60 hover:text-warn"
              :title="t('refStock.delete')"
              @click="game.deleteRefSlot(s.slot)"
            >
              <Icon name="trash" :size="13" />
            </button>
          </div>
          <p v-if="s.name" class="truncate text-[10px] text-parchment/35" :title="s.name">
            {{ s.name }} ({{ Math.round(s.size / 1024) }} KB)
          </p>
        </div>
      </div>

      <!-- 送られなかったものは理由つきで出す (沈黙を作らない)。 -->
      <ul v-if="game.refStock?.skipped.length" class="text-[11px] text-parchment/40 space-y-0.5">
        <li v-for="[name, reason] in game.refStock.skipped" :key="name">
          ✗ {{ name }} — {{ t(`refStock.skip.${reason}`) }}
        </li>
      </ul>

      <div class="flex items-center gap-2 border-t border-ash/50 pt-3">
        <button
          class="rounded bg-ash/40 hover:bg-ash/70 px-3 py-1 text-sm text-parchment/80"
          :title="t('refStock.reseedHint')"
          @click="game.reseedRefStock()"
        >
          {{ t("refStock.reseed") }}
        </button>
        <span class="text-[11px] text-parchment/40">{{ t("refStock.reseedHint") }}</span>
      </div>
    </div>
  </div>
</template>
