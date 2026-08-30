<script setup lang="ts">
// 挿絵 (画像生成、spec 24) の操作列。読み上げ操作 (TtsControls) と同じ「会話ペインの右下に
// ホバーで浮き出る」流儀 — 常設すると物語の邪魔になるが、寄せれば届く。
//
// 表示条件は親 (App.vue): 設定「画像生成」で有効にした人にだけ出る (作者ゲートは作らない —
// 挿絵は語りに触れないプレイヤー側の鑑賞物で、押すのもプレイヤー)。
// 読み上げ操作と同居するときは左へ寄せる (両方 absolute right なので重なる)。
import { computed, defineAsyncComponent, ref } from "vue";
import { useGameStore } from "../stores/game";
import { t } from "../i18n";
import Icon from "./Icon.vue";

// spec 27 Phase D: 参照ストックとプロンプト工房は**別々のダイアログ**にする (決定 13) —
// プロンプトは文字が主・参照は絵が主で、1 枚に混ぜると縦が伸びて会話ペインを覆う。
// どちらも開いたときだけ読む (プロンプト側は CodeMirror を抱えるので特に)。
const RefStockDialog = defineAsyncComponent(() => import("./RefStockDialog.vue"));
const PromptDialog = defineAsyncComponent(() => import("./PromptDialog.vue"));

const game = useGameStore();
const showRefStock = ref(false);
const showPrompt = ref(false);
// ボタンは開閉のトグル (ユーザーFB 2026-08-26 — ✕ を探すより押した所で閉じる方が自然)。
// **排他**にするのは、浮遊パネルが 2 枚とも会話ペイン右下の同じ位置に出るから — 両方 ON に
// できると 1 枚しか見えないのにボタンは 2 つとも「開いている」と映り、状態表示が嘘になる。
function toggleRefStock() {
  showPrompt.value = false;
  showRefStock.value = !showRefStock.value;
}
function togglePrompt() {
  showRefStock.value = false;
  showPrompt.value = !showPrompt.value;
}

const genLabel = computed(() => (game.imageBusy ? t("image.generating") : t("image.generate")));
const imageToggleLabel = computed(() =>
  game.showGeneratedImage ? t("image.hideImage") : t("image.showImage"),
);
const textToggleLabel = computed(() => (game.showText ? t("image.hideText") : t("image.showText")));
</script>

<template>
  <div
    class="absolute bottom-3 z-20 flex items-center gap-1 rounded-full bg-ink/70 backdrop-blur-sm
           px-1.5 py-1 shadow-lg ring-1 ring-glow/10
           opacity-0 group-hover:opacity-100 focus-within:opacity-100 transition-opacity duration-200"
    :class="game.ttsFeature ? 'right-28' : 'right-4'"
  >
    <!-- 生成。処理中は押せない (スピナー)。気に入らなければ何度でも押せる (差し替え)。 -->
    <button
      type="button"
      class="p-1.5 rounded-full text-glow/60 hover:text-ember hover:bg-glow/10 transition-colors
             disabled:opacity-50 disabled:hover:text-glow/60 disabled:hover:bg-transparent"
      :disabled="game.imageBusy || !game.started"
      :title="genLabel"
      :aria-label="genLabel"
      @click="game.generateImage()"
    >
      <Icon v-if="game.imageBusy" name="spinner" :size="15" class="animate-spin" />
      <Icon v-else name="image" :size="15" />
    </button>
    <!-- 参照ストック (spec 27)。画像が無くても開ける (削除・入れ直しはいつでも要る)。 -->
    <button
      type="button"
      class="p-1.5 rounded-full text-glow/60 hover:text-ember hover:bg-glow/10 transition-colors
             disabled:opacity-50"
      :disabled="!game.started"
      :class="{ 'text-ember bg-glow/10': showRefStock }"
      :title="t('refStock.heading')"
      :aria-label="t('refStock.heading')"
      :aria-expanded="showRefStock"
      @click="toggleRefStock"
    >
      <Icon name="folder" :size="15" />
    </button>
    <!-- プロンプト工房 (spec 27)。 -->
    <button
      type="button"
      class="p-1.5 rounded-full text-glow/60 hover:text-ember hover:bg-glow/10 transition-colors
             disabled:opacity-50"
      :disabled="!game.started"
      :class="{ 'text-ember bg-glow/10': showPrompt }"
      :title="t('promptWorkshop.heading')"
      :aria-label="t('promptWorkshop.heading')"
      :aria-expanded="showPrompt"
      @click="togglePrompt"
    >
      <Icon name="pencil" :size="15" />
    </button>
    <!-- 保存。画像が在るときだけ出る。 -->
    <button
      v-if="game.generatedImage"
      type="button"
      class="p-1.5 rounded-full text-glow/60 hover:text-ember hover:bg-glow/10 transition-colors"
      :title="t('image.save')"
      :aria-label="t('image.save')"
      @click="game.saveGeneratedImage()"
    >
      <Icon name="save" :size="15" />
    </button>
    <!-- 挿絵の表示/非表示 (画像が在るときだけ意味がある)。 -->
    <button
      v-if="game.generatedImage"
      type="button"
      class="p-1.5 rounded-full text-glow/60 hover:text-ember hover:bg-glow/10 transition-colors"
      :class="{ 'text-ember': !game.showGeneratedImage }"
      :title="imageToggleLabel"
      :aria-label="imageToggleLabel"
      @click="game.toggleGeneratedImage()"
    >
      <Icon :name="game.showGeneratedImage ? 'eye' : 'eye-off'" :size="15" />
    </button>
    <!-- 文字の表示/非表示 (挿絵を鑑賞するため。入力欄と右ペインは残る)。 -->
    <button
      type="button"
      class="p-1.5 rounded-full text-glow/60 hover:text-ember hover:bg-glow/10 transition-colors"
      :class="{ 'text-ember': !game.showText }"
      :title="textToggleLabel"
      :aria-label="textToggleLabel"
      @click="game.toggleText()"
    >
      <Icon name="text" :size="15" />
    </button>
  </div>
  <RefStockDialog v-if="showRefStock" @close="showRefStock = false" />
  <PromptDialog v-if="showPrompt" @close="showPrompt = false" />
</template>
