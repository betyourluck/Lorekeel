<script setup lang="ts">
/**
 * マップパネル (spec 15 rev2) — **現在地と、そこから行ける場所のリスト**。右ペインの第4タブ。
 *
 * 2026-09-09 に有向グラフから作り替えた (ユーザー決定「マップにしてと言ったのは僕だったが、
 * やはりリストのほうが見やすい」)。元の要望は「どこへ行ける?」を GM に尋ねる往復を減らすこと
 * で、**その答えは常に現在地の隣接だけ**だった — 全体図は問いより広く、狭い右ペインで読むには
 * 重かった。奥は畳み、いま選べる道だけを縦に並べる。
 *
 * 未踏の場所も**名前は出す**が薄く描く (行けることが分かるのが主目的)。gate 未達は 🔒。
 * 中身 (説明・画像) は訪問済みだけ = backend が伏せている。
 *
 * engine 無改修の派生表示 — 状態の真実は backend。ここは game.map を描くだけ。
 */
import { computed, ref } from "vue";
// spec 23 Phase A: MapNode.image はアセット ID — store の prefetch 済みキャッシュから引く。
import { assetUrl, useGameStore } from "../stores/game";
import { mapList } from "../map";
import { t } from "../i18n";
import type { MapNode } from "../types/api";

const game = useGameStore();

const list = computed(() => mapList(game.map));

// 展開中の行き先 (クリックでトグル)。現在地は常に開いている。
const openId = ref<string | null>(null);
function toggle(id: string) {
  openId.value = openId.value === id ? null : id;
}

const imageOf = (n: MapNode) => (n.image ? assetUrl("images", n.image) : null);

// 場所の画像を参照ストックの枠へドラッグできる (2026-08-28 ユーザー要望)。顔アイコン
// (spec 27 追補) と**同じ経路**で、運ぶのはアセット ID だけ — bytes は backend が読む。
// 渡すのは生の ID であって表示用の `asset://` URL ではない。未踏の場所は image を持たない
// (backend が伏せる) ので、そもそも掴めない。
function onImageDragStart(e: DragEvent, n: MapNode) {
  if (!n.image || !e.dataTransfer) return;
  e.dataTransfer.setData("application/x-kataribe-asset", n.image);
  e.dataTransfer.effectAllowed = "copy";
}
</script>

<template>
  <div class="h-full flex flex-col overflow-y-auto">
    <p v-if="!list.current" class="text-parchment/50 text-xs py-4">{{ t("map.empty") }}</p>

    <template v-else>
      <!-- 現在地 (常に開いている) -->
      <div class="mp-here">
        <div class="mp-label text-parchment/50">{{ t("map.legendCurrent") }}</div>
        <div class="text-sm font-bold text-ember leading-tight">
          {{ list.current.title || list.current.id }}
        </div>
        <img
          v-if="imageOf(list.current)"
          :src="imageOf(list.current) as string"
          class="w-full rounded mt-1.5 border border-ash/50 cursor-grab"
          draggable="true"
          :title="t('map.dragToRef')"
          @dragstart="(e) => onImageDragStart(e, list.current as MapNode)"
        />
        <p
          v-if="list.current.description"
          class="mt-1.5 text-[11px] leading-relaxed text-parchment/70 whitespace-pre-line"
        >
          {{ list.current.description }}
        </p>
      </div>

      <!-- ここから行ける場所 -->
      <div class="mt-3">
        <div class="mp-label text-parchment/50 flex items-center gap-1.5">
          {{ t("map.exits") }}
          <span v-if="list.exits.length" class="mp-count">{{ list.exits.length }}</span>
        </div>

        <p v-if="!list.exits.length" class="text-[11px] text-parchment/50 py-1">
          {{ t("map.noExits") }}
        </p>

        <ul v-else class="mt-1 space-y-0.5">
          <li v-for="x in list.exits" :key="x.node.id">
            <button
              class="mp-row"
              :class="[
                openId === x.node.id ? 'open' : '',
                x.locked ? 'text-parchment/50' : x.node.visited ? 'text-parchment/90' : 'text-parchment/55',
              ]"
              :title="x.node.visited ? x.node.id : t('map.legendFrontier')"
              @click="toggle(x.node.id)"
            >
              <span
                class="mp-arrow"
                :class="x.locked ? 'text-parchment/35' : 'text-ember'"
                aria-hidden="true"
              >→</span>
              <span class="flex-1 min-w-0 truncate text-left">{{ x.node.title || x.node.id }}</span>
              <span v-if="x.locked" class="shrink-0" :title="t('map.legendLocked')">🔒</span>
            </button>

            <!-- 展開: 訪問済みは中身、未踏は「まだ訪れていない」 -->
            <div v-if="openId === x.node.id" class="mp-detail">
              <template v-if="x.node.visited">
                <img
                  v-if="imageOf(x.node)"
                  :src="imageOf(x.node) as string"
                  class="w-full rounded mb-1.5 border border-ash/50 cursor-grab"
                  draggable="true"
                  :title="t('map.dragToRef')"
                  @dragstart="(e) => onImageDragStart(e, x.node)"
                />
                <p
                  v-if="x.node.description"
                  class="text-[11px] leading-relaxed text-parchment/70 whitespace-pre-line"
                >
                  {{ x.node.description }}
                </p>
                <p v-else class="text-[11px] text-parchment/50">{{ t("map.noDescription") }}</p>
              </template>
              <p v-else class="text-[11px] text-parchment/50">{{ t("map.undiscovered") }}</p>
            </div>
          </li>
        </ul>
      </div>

      <!-- 凡例 (薄い = 未踏 / 🔒 = いまは行けない) -->
      <div
        class="mt-auto pt-2 border-t border-ash/50 flex flex-wrap gap-x-3 gap-y-1 text-[10px] text-parchment/50"
      >
        <span>{{ t("map.legendFrontierHint") }}</span>
        <span>🔒 {{ t("map.legendLocked") }}</span>
      </div>
    </template>
  </div>
</template>

<style scoped>
/* 色は template 側の Tailwind ユーティリティで当てる — scoped CSS に
   `rgb(var(--parchment) / N)` と直書きすると **2026-09-08 の減光カーブを通らず**
   ライトテーマだけ薄いまま残る (カーブは textColor スケールに掛かっている)。 */
.mp-label {
  font-size: 10px;
  letter-spacing: 0.08em;
  margin-bottom: 0.15rem;
}
.mp-here {
  border-left: 2px solid rgb(var(--ember));
  padding-left: 0.55rem;
}
.mp-count {
  font-size: 10px;
  line-height: 1;
  padding: 0.1rem 0.3rem;
  border-radius: 9999px;
  background: rgb(var(--ember) / 0.18);
  color: rgb(var(--ember));
}
.mp-row {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  width: 100%;
  padding: 0.3rem 0.45rem;
  border-radius: 0.375rem;
  border: 1px solid transparent;
  font-size: 12px;
  transition:
    background-color 0.12s,
    border-color 0.12s;
}
.mp-row:hover {
  background: rgb(var(--ash) / 0.45);
}
.mp-row.open {
  border-color: rgb(var(--ash));
  background: rgb(var(--ash) / 0.35);
}
/* 矢印は通れる道だけ熾火 (「ここは通れる」が一目で分かる = 要望の核心)。
   通れない道は矢印を落として 🔒 に語らせる。色は template 側。 */
.mp-arrow {
  font-size: 11px;
}
.mp-detail {
  padding: 0.35rem 0.45rem 0.5rem 1.35rem;
}
</style>
