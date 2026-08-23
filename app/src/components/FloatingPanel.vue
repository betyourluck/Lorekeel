<script setup lang="ts">
/**
 * 会話ペインの右下に**浮かぶ**パネル (spec 27 Phase D 改)。参照ストックとプロンプト工房の器。
 *
 * モーダルの箱ではなく、場面の上に**半透明のガラス**として置く — 挿絵や参照を触る作業は
 * 「いま出ている絵を見ながら」やるもので、画面を暗幕で覆ってしまうと比べる相手が消える。
 * だから幕は張らず、閉じるのは ✕ と Esc だけ (場面のクリックは奪わない)。
 *
 * 親 (ImageControls) は `group relative` な会話ペインの中にいるので `absolute` で右下に寄せる。
 * 色はペインの data-theme (既定 dark) のトークンを引く = 暗幕の上では常に暗いガラスになる。
 */
import { onBeforeUnmount, onMounted } from "vue";

const props = withDefaults(defineProps<{ title: string; width?: string }>(), { width: "40rem" });
const emit = defineEmits<{ (e: "close"): void }>();

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") {
    e.stopPropagation();
    emit("close");
  }
}
onMounted(() => window.addEventListener("keydown", onKey));
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <Transition name="float" appear>
    <section
      class="absolute bottom-14 right-4 z-30 max-w-[calc(100%-2rem)] max-h-[calc(100%-5rem)] overflow-y-auto
             rounded-2xl bg-ink/65 backdrop-blur-md ring-1 ring-ember/25 text-parchment
             shadow-[0_18px_50px_-12px_rgba(0,0,0,0.6),0_0_0_1px_rgba(0,0,0,0.25)]"
      :style="{ width: props.width }"
      role="dialog"
      :aria-label="props.title"
    >
      <!-- 上辺の熾火の線: 箱の縁でなく、灯りが落ちている感じを出す装飾。 -->
      <div class="pointer-events-none absolute inset-x-6 top-0 h-px bg-gradient-to-r from-transparent via-ember/70 to-transparent" />
      <header class="flex items-center justify-between px-5 pt-4 pb-2">
        <h2 class="flex items-center gap-2 font-serif text-base tracking-wide text-glow">
          <span class="inline-block h-1.5 w-1.5 rounded-full bg-ember shadow-[0_0_8px_rgb(var(--ember))]" />
          {{ props.title }}
        </h2>
        <button
          class="rounded-full p-1 text-parchment/40 hover:text-parchment hover:bg-parchment/10 transition-colors"
          :aria-label="'close'"
          @click="emit('close')"
        >
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round">
            <path d="M3 3l8 8M11 3l-8 8" />
          </svg>
        </button>
      </header>
      <div class="px-5 pb-5">
        <slot />
      </div>
    </section>
  </Transition>
</template>

<style scoped>
/* 下から 10px 浮き上がりながら現れる。消えるときは逆。 */
.float-enter-active,
.float-leave-active {
  transition: opacity 180ms ease, transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
}
.float-enter-from,
.float-leave-to {
  opacity: 0;
  transform: translateY(10px) scale(0.98);
}
</style>
