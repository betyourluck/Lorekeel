<script setup lang="ts">
/**
 * 参照ストック (spec 27 Phase D、2026-08-24 に浮遊パネルへ)。
 *
 * 枠は `MAX_REFS` から生成する (3 固定で組まない)。**枠番号 = ファイル名番号 = ワイヤ番号** が
 * backend の前詰め不変条件で保証されているので、ここは番号の対応を気にせず現物を並べるだけ。
 *
 * 入れ方は 3 つ: いまの挿絵 (backend が原本 bytes を書く) / ローカルファイル (WebView が WebP へ
 * 変換・縮小してから raw body で送る) / ドロップ (ファイルと同じ経路)。
 *
 * サムネイルは `asset://` だが、**前詰めでファイル名が固定のまま中身が入れ替わる**ので URL に
 * 版 (`?v=`) を付けてキャッシュを割る (failures #86)。
 */
import { computed, onMounted, ref } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useGameStore } from "../stores/game";
import { t } from "../i18n";
import FloatingPanel from "./FloatingPanel.vue";
import Icon from "./Icon.vue";

const game = useGameStore();
const emit = defineEmits<{ (e: "close"): void }>();

onMounted(() => game.loadRefStock());

const slots = computed(() => {
  const stock = game.refStock;
  const max = stock?.max ?? 3;
  return Array.from({ length: max }, (_, i) => {
    const entry = stock?.picked[i];
    return {
      slot: i + 1,
      name: entry?.[0] ?? null,
      size: entry?.[1] ?? 0,
      url: entry && stock ? `${convertFileSrc(`${stock.dir}/${entry[0]}`)}?v=${game.refStockRev}` : null,
      // 写真を机に置いたような、わずかな傾き (交互)。hover で正す。
      tilt: [-2, 1.2, -1][i % 3],
    };
  });
});

/** 空きは「末尾の 1 つ」だけが受ける (前詰め不変条件を UI 側でも壊さない)。 */
const nextEmpty = computed(() => slots.value.find((s) => !s.name)?.slot ?? null);
const canPut = computed(() => !!game.generatedImage);
function accepts(slot: { slot: number; name: string | null }): boolean {
  return !!slot.name || slot.slot === nextEmpty.value;
}

// --- ローカルファイル ---------------------------------------------------------------------
const picker = ref<HTMLInputElement | null>(null);
const targetSlot = ref(1);
const dragOver = ref<number | null>(null);

function pickFor(slot: number) {
  targetSlot.value = slot;
  picker.value?.click();
}
function onPicked(e: Event) {
  const input = e.target as HTMLInputElement;
  const file = input.files?.[0];
  input.value = ""; // 同じファイルをもう一度選べるように
  if (file) game.putRefFile(targetSlot.value, file);
}
/**
 * 「入れ直す」は自分で入れた絵を**全部**パッケージの種で置き換える不可逆操作なのに、隣が
 * よく押す操作 (削除・取り込み) — 誤爆すると育てた参照が黙って消えるので確認を挟む
 * (シードリセットと同じ判断: 不可逆で、押した瞬間に画面上の異変が小さい操作には確認)。
 */
async function onReseed() {
  if (await game.askConfirm(t("refStock.reseedConfirm"), t("refStock.reseed"))) {
    game.reseedRefStock();
  }
}
function onDrop(slot: number, e: DragEvent) {
  dragOver.value = null;
  const file = e.dataTransfer?.files?.[0];
  if (file && accepts(slots.value[slot - 1])) game.putRefFile(slot, file);
}
</script>

<template>
  <FloatingPanel :title="t('refStock.heading')" width="38rem" @close="emit('close')">
    <p class="text-[11px] leading-relaxed text-parchment/45 mb-4">{{ t("refStock.intro") }}</p>

    <input ref="picker" type="file" accept="image/png,image/jpeg,image/webp,image/gif,image/bmp" class="hidden" @change="onPicked" />

    <div class="grid grid-cols-3 gap-4 px-1">
      <div
        v-for="s in slots"
        :key="s.slot"
        class="group/card relative"
        @dragover.prevent="accepts(s) && (dragOver = s.slot)"
        @dragleave="dragOver = null"
        @drop.prevent="onDrop(s.slot, $event)"
      >
        <!-- 写真カード。埋まっていれば現物、空なら点線の枠 (末尾の 1 つだけが受ける)。 -->
        <div
          class="relative aspect-square overflow-hidden rounded-xl transition-all duration-200
                 group-hover/card:rotate-0 group-hover/card:scale-[1.03]"
          :class="[
            s.name
              ? 'bg-parchment/5 ring-1 ring-parchment/15 shadow-[0_10px_24px_-8px_rgba(0,0,0,0.7)]'
              : accepts(s)
                ? 'ring-1 ring-dashed ring-ember/40 bg-ember/5 cursor-pointer'
                : 'ring-1 ring-dashed ring-parchment/10 opacity-40',
            dragOver === s.slot ? 'ring-2 ring-ember bg-ember/15' : '',
          ]"
          :style="{ transform: `rotate(${s.tilt}deg)` }"
          @click="!s.name && accepts(s) && pickFor(s.slot)"
        >
          <img v-if="s.url" :src="s.url" class="h-full w-full object-cover" :alt="s.name ?? ''" />
          <div v-else-if="accepts(s)" class="flex h-full flex-col items-center justify-center gap-1 text-ember/70">
            <Icon name="plus" :size="22" />
            <span class="text-[10px]">{{ t("refStock.dropHint") }}</span>
          </div>
          <span class="absolute left-2 top-2 rounded-full bg-ink/70 px-1.5 text-[10px] text-parchment/60 backdrop-blur-sm">
            {{ s.slot }}
          </span>
          <!-- 埋まった枠の操作は hover で浮き出る (常設すると写真でなく業務の表になる)。 -->
          <div
            v-if="s.name"
            class="absolute inset-x-0 bottom-0 flex items-center justify-center gap-1 bg-gradient-to-t from-ink/90 to-transparent
                   px-2 pb-2 pt-6 opacity-0 transition-opacity group-hover/card:opacity-100"
          >
            <button
              class="rounded-full bg-ink/70 p-1.5 text-parchment/80 hover:text-ember disabled:opacity-30"
              :disabled="!canPut"
              :title="canPut ? t('refStock.fromImage') : t('refStock.needImage')"
              @click.stop="game.putRefSlot(s.slot)"
            >
              <Icon name="image" :size="13" />
            </button>
            <button
              class="rounded-full bg-ink/70 p-1.5 text-parchment/80 hover:text-ember"
              :title="t('refStock.fromFile')"
              @click.stop="pickFor(s.slot)"
            >
              <Icon name="folder" :size="13" />
            </button>
            <button
              class="rounded-full bg-ink/70 p-1.5 text-parchment/80 hover:text-warn"
              :title="t('refStock.delete')"
              @click.stop="game.deleteRefSlot(s.slot)"
            >
              <Icon name="trash" :size="13" />
            </button>
          </div>
        </div>
        <!-- 空きの末尾にだけ「いまの挿絵を」の小さな導線 (ファイルはカードのクリック)。 -->
        <div v-if="!s.name && accepts(s)" class="mt-1.5 flex justify-center">
          <button
            class="rounded-full px-2 py-0.5 text-[10px] text-parchment/50 hover:text-ember hover:bg-parchment/5 disabled:opacity-30"
            :disabled="!canPut"
            :title="canPut ? '' : t('refStock.needImage')"
            @click="game.putRefSlot(s.slot)"
          >
            {{ t("refStock.fromImage") }}
          </button>
        </div>
        <p v-else-if="s.name" class="mt-1.5 truncate text-center text-[10px] text-parchment/30" :title="s.name">
          {{ Math.round(s.size / 1024) }} KB
        </p>
      </div>
    </div>

    <p v-if="game.refStockBusy" class="mt-3 text-[11px] text-ember/80">{{ t("refStock.converting") }}</p>

    <!-- 送られなかったものは理由つきで出す (沈黙を作らない)。 -->
    <ul v-if="game.refStock?.skipped.length" class="mt-3 space-y-0.5 text-[11px] text-parchment/40">
      <li v-for="[name, reason] in game.refStock.skipped" :key="name">✗ {{ name }} — {{ t(`refStock.skip.${reason}`) }}</li>
    </ul>

    <div class="mt-4 flex items-center gap-3 border-t border-parchment/10 pt-3">
      <button
        class="rounded-full px-3 py-1 text-[11px] text-parchment/60 ring-1 ring-parchment/15 hover:text-parchment hover:ring-parchment/40 transition-colors"
        @click="onReseed()"
      >
        {{ t("refStock.reseed") }}
      </button>
      <span class="text-[10px] text-parchment/35">{{ t("refStock.reseedHint") }}</span>
    </div>
  </FloatingPanel>
</template>
