<script setup lang="ts">
/**
 * 長い説明文を畳む (2026-09-13 ユーザーFB「説明文字が多いととっつきにくい」)。
 * 既定は畳み、「説明を見る」で開く。開閉は**その場限り** (localStorage に覚えない) —
 * 初見の人は畳まれた画面を見て、必要なときだけ開く、が狙いなので、覚えると二度目以降は
 * 常に開いた画面 = 元の「多い」状態へ戻ってしまう。短い注記 (1〜2 行) は畳まない
 * (畳む手間の方が読む手間より大きい)。
 */
import { ref } from "vue";

import { t } from "../i18n";

const props = defineProps<{ open?: boolean }>();
const shown = ref(!!props.open);
</script>

<template>
  <div class="text-xs">
    <button
      type="button"
      class="text-parchment/50 hover:text-parchment/80 underline decoration-dotted underline-offset-2"
      :aria-expanded="shown"
      @click="shown = !shown"
    >
      {{ shown ? t("settings.help.hideNote") : t("settings.help.showNote") }}
    </button>
    <p v-if="shown" class="text-parchment/40 mt-1">
      <slot />
    </p>
  </div>
</template>
