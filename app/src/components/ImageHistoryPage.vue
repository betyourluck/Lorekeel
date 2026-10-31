<script setup lang="ts">
/**
 * 挿絵の履歴 (spec 37)。タイトルバーの下の本体全体を差し替える 1 ページ —
 * 現在のパッケージで生成した絵のサムネイルを新しい順に並べ、クリックで大きく見せ、削除できる。
 *
 * 一覧はフォルダを読んで組む (backend `list_generated_images`、索引は持たない)。サムネイルと
 * プレビューは `asset://` (backend が履歴フォルダをスコープへ許す)。開いている間もターンは止めない
 * (store の状態はそのまま、戻るで元の画面へ)。
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { t } from "../i18n";
import { useGameStore } from "../stores/game";
import Icon from "./Icon.vue";

const game = useGameStore();

/** プレビュー中のファイル名 (null = 一覧)。 */
const previewing = ref<string | null>(null);
const previewEntry = computed(() =>
  game.imageHistory.entries.find((e) => e.file_name === previewing.value) ?? null,
);

// asset:// へ。Tauri の外 (ブラウザだけの dev サーバ) では convertFileSrc が例外を投げ、
// 描画ごと止まるのでパスをそのまま返す (配置の確認に data URL を流せる)。
function urlOf(path: string): string {
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
}
function whenOf(ms: number): string {
  return new Date(ms).toLocaleString();
}
function sizeOf(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

function openPreview(name: string): void {
  previewing.value = name;
}
function closePreview(): void {
  previewing.value = null;
}
async function remove(name: string): Promise<void> {
  const ok = await game.askConfirm(t("image.historyDeleteConfirm"), t("image.historyDeleteOk"));
  if (!ok) return;
  if (previewing.value === name) previewing.value = null;
  await game.deleteImageHistoryEntry(name);
}

// Esc: プレビューを閉じる → 一覧も閉じる (2 段)。確認ダイアログが出ている間は触らない。
function onKey(e: KeyboardEvent): void {
  if (e.key !== "Escape" || game.confirmDialog) return;
  if (previewing.value) {
    closePreview();
  } else {
    game.closeImageHistory();
  }
  e.preventDefault();
}
onMounted(() => {
  window.addEventListener("keydown", onKey);
  void game.refreshImageHistory();
});
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
// ヘッダのコンボリストで別のパッケージが選ばれたら一覧を畳んでゲームの画面へ戻る (ユーザー要望,
// 2026-11-01)。一覧は「いま選んでいる盤面」の絵なので、選択が替わった時点で中身の前提が消える。
// 監視はこのページが開いている間だけ生きる (閉じれば unmount で外れる)。
watch(
  () => game.packagePath,
  () => game.closeImageHistory(),
);
</script>

<template>
  <section class="relative flex-1 flex flex-col min-w-0 min-h-0 bg-ink" data-testid="image-history">
    <!-- 見出し行: 戻る / タイトル / 枚数 -->
    <header class="flex items-center gap-3 px-4 py-2 border-b border-glow/10 shrink-0">
      <button
        type="button"
        class="flex items-center gap-1 px-2 py-1 rounded text-sm text-parchment/80 hover:text-ember hover:bg-glow/10 transition-colors"
        data-testid="history-back"
        :title="t('image.historyBack')"
        @click="game.closeImageHistory()"
      >
        <Icon name="chevron" :size="14" class="rotate-90" />
        {{ t("image.historyBack") }}
      </button>
      <h2 class="text-sm font-medium text-parchment/90">{{ t("image.historyTitle") }}</h2>
      <span class="text-xs text-parchment/60" data-testid="history-count">
        {{ t("image.historyCount", { count: game.imageHistory.entries.length }) }}
      </span>
      <span v-if="game.imageHistory.loading" class="text-xs text-parchment/60">{{ t("image.historyLoading") }}</span>
    </header>

    <!-- 一覧 -->
    <div class="flex-1 min-h-0 overflow-y-auto p-4">
      <p v-if="game.imageHistory.error" class="text-sm text-warn" data-testid="history-error">
        {{ game.imageHistory.error }}
      </p>
      <p
        v-else-if="!game.imageHistory.loading && game.imageHistory.entries.length === 0"
        class="text-sm text-parchment/60"
        data-testid="history-empty"
      >
        {{ t("image.historyEmpty") }}
      </p>
      <ul v-else class="grid gap-3" style="grid-template-columns: repeat(auto-fill, minmax(11rem, 1fr))">
        <li
          v-for="e in game.imageHistory.entries"
          :key="e.file_name"
          class="group/card relative rounded-md overflow-hidden bg-black/30 ring-1 ring-glow/10"
          data-testid="history-item"
        >
          <button
            type="button"
            class="block w-full aspect-square"
            :title="t('image.historyPreview')"
            :data-name="e.file_name"
            data-testid="history-thumb"
            @click="openPreview(e.file_name)"
          >
            <img :src="urlOf(e.path)" alt="" class="w-full h-full object-cover" loading="lazy" />
          </button>
          <div class="flex items-center justify-between gap-2 px-2 py-1 text-[11px] text-parchment/70">
            <span class="truncate" :title="e.file_name">{{ whenOf(e.created_ms) }}</span>
            <span class="shrink-0">{{ sizeOf(e.bytes) }}</span>
          </div>
          <!-- 削除はホバーで浮き出る (常設すると誤爆しやすい)。確認を挟む。 -->
          <button
            type="button"
            class="absolute top-1 right-1 p-1 rounded-full bg-ink/70 text-parchment/70 hover:text-warn
                   opacity-0 group-hover/card:opacity-100 focus:opacity-100 transition-opacity"
            :title="t('image.historyDelete')"
            :aria-label="t('image.historyDelete')"
            data-testid="history-delete"
            @click.stop="remove(e.file_name)"
          >
            <Icon name="trash" :size="13" />
          </button>
        </li>
      </ul>
    </div>

    <!-- プレビュー (大きく見せる)。幕のクリックか Esc か ✕ で一覧へ戻る。 -->
    <div
      v-if="previewEntry"
      class="absolute inset-0 z-30 flex flex-col bg-black/90"
      data-testid="history-preview"
      @click.self="closePreview"
    >
      <div class="flex items-center justify-between px-4 py-2 text-xs text-parchment/80 shrink-0">
        <span>{{ whenOf(previewEntry.created_ms) }} · {{ sizeOf(previewEntry.bytes) }} · {{ previewEntry.file_name }}</span>
        <div class="flex items-center gap-1">
          <button
            type="button"
            class="p-1.5 rounded-full hover:text-warn hover:bg-glow/10"
            :title="t('image.historyDelete')"
            data-testid="history-preview-delete"
            @click="remove(previewEntry.file_name)"
          >
            <Icon name="trash" :size="15" />
          </button>
          <button
            type="button"
            class="px-2 py-1 rounded hover:text-ember hover:bg-glow/10"
            data-testid="history-preview-close"
            @click="closePreview"
          >
            ✕
          </button>
        </div>
      </div>
      <div class="flex-1 min-h-0 flex items-center justify-center p-4" @click.self="closePreview">
        <img :src="urlOf(previewEntry.path)" alt="" class="max-w-full max-h-full object-contain" />
      </div>
    </div>
  </section>
</template>
