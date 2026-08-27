<script setup lang="ts">
/**
 * 画像のクロップ (spec 28 追補、2026-08-28 ユーザー要望「画像はクロップで削れる」)。
 *
 * **保存先を知らない部品** — 切り抜いたバイト列を `apply` で返すだけで、どこへ
 * 書くかは親が決める。使い手は 2 つ:
 * - エディタのメディア (spec 28): **同じアセット ID へ上書き** — 名前が変わると
 *   参照している YAML を全部書き直す羽目になるため。
 * - 参照ストックの枠 (spec 27、2026-08-28 ユーザー要望): **その枠を差し替え** —
 *   「たくさんキャラのいる設定画集から、今の場面に居ない人物を削る」ため。
 * 保存先の知識をここに持たせないのは、二つ目が来たときに分岐が増えるのを避けるため
 * (最初の実装は `replaceEditorMedia` を直に呼んでいた = 一般化はこの要望で入った)。
 *
 * 切り抜きは canvas で完結する (Rust に image crate を足さない — spec 27 の
 * ローカル取り込みと同じ規律)。出力形式は親が `mime` で決める: エディタは元の
 * 形式に合わせ (backend が中身を嗅ぎ分けて置き場を決めるので拡張子と食い違わせない)、
 * 参照ストックは WebP 固定 (枠は拡張子を選ばない)。
 */
import { computed, onMounted, ref } from "vue";

import { t } from "../i18n";
import { useGameStore } from "../stores/game";

const props = withDefaults(
  defineProps<{
    /** 切り抜く元画像 (asset:// URL)。 */
    src: string;
    /** 確認ダイアログに出す対象名 (ファイル名 / 「参照 2」など)。 */
    label: string;
    /** 出力形式。親が保存先の作法に合わせて決める。 */
    mime?: string;
    /** 下段の注記。**保存先ごとに言うべきことが違う**ので親が渡す
     *  (エディタ = ID が変わらない / 参照ストック = 枠を差し替える)。 */
    note?: string;
  }>(),
  { mime: "image/webp", note: "" },
);
const emit = defineEmits<{ (e: "close"): void; (e: "apply", bytes: Uint8Array): void }>();

const game = useGameStore();
const img = ref<HTMLImageElement | null>(null);
const frame = ref<HTMLElement | null>(null);
const natural = ref({ w: 0, h: 0 });
const busy = ref(false);

/** 選択範囲 (表示座標の割合 0..1 — 表示サイズが変わっても保つ)。 */
const sel = ref({ x: 0.1, y: 0.1, w: 0.8, h: 0.8 });

/** 実ピクセルでの切り抜き寸法 (表示用)。 */
const outSize = computed(() => ({
  w: Math.max(1, Math.round(sel.value.w * natural.value.w)),
  h: Math.max(1, Math.round(sel.value.h * natural.value.h)),
}));

function onLoad() {
  const el = img.value;
  if (!el) return;
  natural.value = { w: el.naturalWidth, h: el.naturalHeight };
}

type Handle = "move" | "nw" | "ne" | "sw" | "se";

/** ドラッグで選択範囲を動かす/広げる。座標は frame の割合で扱う。 */
function startDrag(e: PointerEvent, handle: Handle) {
  e.preventDefault();
  e.stopPropagation();
  const box = frame.value?.getBoundingClientRect();
  if (!box) return;
  const start = { ...sel.value };
  const px = (e.clientX - box.left) / box.width;
  const py = (e.clientY - box.top) / box.height;
  const clamp = (v: number) => Math.min(1, Math.max(0, v));

  const move = (ev: PointerEvent) => {
    const dx = (ev.clientX - box.left) / box.width - px;
    const dy = (ev.clientY - box.top) / box.height - py;
    const s = { ...start };
    if (handle === "move") {
      s.x = clamp(Math.min(start.x + dx, 1 - start.w));
      s.y = clamp(Math.min(start.y + dy, 1 - start.h));
    } else {
      // 角のハンドル: 対角を固定して伸縮 (最小 2% は残す = 潰れて消えない)。
      const right = handle === "ne" || handle === "se";
      const bottom = handle === "sw" || handle === "se";
      const x0 = right ? start.x : clamp(start.x + dx);
      const x1 = right ? clamp(start.x + start.w + dx) : start.x + start.w;
      const y0 = bottom ? start.y : clamp(start.y + dy);
      const y1 = bottom ? clamp(start.y + start.h + dy) : start.y + start.h;
      s.x = Math.min(x0, x1 - 0.02);
      s.y = Math.min(y0, y1 - 0.02);
      s.w = Math.max(0.02, x1 - s.x);
      s.h = Math.max(0.02, y1 - s.y);
    }
    sel.value = s;
  };
  const up = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
}

function reset() {
  sel.value = { x: 0, y: 0, w: 1, h: 1 };
}

async function apply() {
  const el = img.value;
  if (!el || busy.value) return;
  if (!(await game.askConfirm(t("editor.cropConfirm", { file: props.label }), t("editor.cropOk")))) return;
  busy.value = true;
  try {
    const { w: nw, h: nh } = natural.value;
    const sx = Math.round(sel.value.x * nw);
    const sy = Math.round(sel.value.y * nh);
    const sw = outSize.value.w;
    const sh = outSize.value.h;
    const canvas = document.createElement("canvas");
    canvas.width = sw;
    canvas.height = sh;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("canvas 2d context unavailable");
    ctx.drawImage(el, sx, sy, sw, sh, 0, 0, sw, sh);
    const blob = await new Promise<Blob | null>((r) => canvas.toBlob(r, props.mime, 0.92));
    if (!blob) throw new Error("encode failed");
    // 保存は親の責務 (この部品は保存先を知らない)。閉じるのも親が決める —
    // 保存に失敗したときに枠を閉じてしまうと、切った範囲がやり直しになる。
    emit("apply", new Uint8Array(await blob.arrayBuffer()));
  } catch (e) {
    game.logToast = String(e);
  } finally {
    busy.value = false;
  }
}

onMounted(() => {
  if (img.value?.complete) onLoad();
});
</script>

<template>
  <!-- 幕を張る (挿絵の浮遊パネルと違い、これは「切る」作業で、下を見ながら触る必要が無い)。 -->
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-6" @click.self="emit('close')">
    <div class="flex max-h-full w-full max-w-3xl flex-col rounded border border-ash bg-ink p-4">
      <div class="mb-2 flex items-center gap-2 text-sm">
        <span class="font-mono text-parchment/80">{{ label }}</span>
        <span class="text-parchment/40 text-xs">{{ outSize.w }} × {{ outSize.h }} px</span>
        <span class="flex-1"></span>
        <button class="px-2 py-0.5 rounded border border-ash text-xs text-parchment/70 hover:text-parchment" @click="reset">
          {{ t("editor.cropReset") }}
        </button>
      </div>

      <div class="relative min-h-0 flex-1 overflow-hidden bg-black/40">
        <div ref="frame" class="relative inline-block max-h-full">
          <!-- **crossorigin が要る**: asset:// の画像をそのまま canvas に描くと
               「Tainted canvases may not be exported」で toBlob が落ちる (実機で発覚)。
               Tauri の asset protocol は `Access-Control-Allow-Origin: <window_origin>` を
               返す (tauri/src/protocol/asset.rs) ので、CORS を宣言して取れば汚染されない。
               読み込み失敗は沈黙させない — 出ない絵を黙って見せるより理由を出す。 -->
          <img
            ref="img"
            :src="src"
            alt=""
            crossorigin="anonymous"
            class="max-h-[60vh] max-w-full select-none"
            draggable="false"
            @load="onLoad"
            @error="game.logToast = t('editor.cropLoadFailed')"
          />
          <!-- 外側の暗幕 (4 枚) — 残る範囲だけが明るい。 -->
          <div class="pointer-events-none absolute inset-0">
            <div class="absolute bg-black/60" :style="{ left: 0, top: 0, right: 0, height: `${sel.y * 100}%` }"></div>
            <div class="absolute bg-black/60" :style="{ left: 0, bottom: 0, right: 0, height: `${(1 - sel.y - sel.h) * 100}%` }"></div>
            <div class="absolute bg-black/60" :style="{ left: 0, top: `${sel.y * 100}%`, width: `${sel.x * 100}%`, height: `${sel.h * 100}%` }"></div>
            <div class="absolute bg-black/60" :style="{ right: 0, top: `${sel.y * 100}%`, width: `${(1 - sel.x - sel.w) * 100}%`, height: `${sel.h * 100}%` }"></div>
          </div>
          <!-- 選択枠 + 角ハンドル。 -->
          <div
            class="absolute cursor-move border border-ember"
            :style="{ left: `${sel.x * 100}%`, top: `${sel.y * 100}%`, width: `${sel.w * 100}%`, height: `${sel.h * 100}%` }"
            @pointerdown="startDrag($event, 'move')"
          >
            <span
              v-for="h in (['nw', 'ne', 'sw', 'se'] as const)"
              :key="h"
              class="absolute h-3 w-3 rounded-sm bg-ember"
              :class="{
                'left-0 top-0 -translate-x-1/2 -translate-y-1/2 cursor-nwse-resize': h === 'nw',
                'right-0 top-0 translate-x-1/2 -translate-y-1/2 cursor-nesw-resize': h === 'ne',
                'left-0 bottom-0 -translate-x-1/2 translate-y-1/2 cursor-nesw-resize': h === 'sw',
                'right-0 bottom-0 translate-x-1/2 translate-y-1/2 cursor-nwse-resize': h === 'se',
              }"
              @pointerdown="startDrag($event, h)"
            ></span>
          </div>
        </div>
      </div>

      <div class="mt-3 flex items-center gap-2">
        <p class="flex-1 text-xs text-parchment/40">{{ note }}</p>
        <button class="px-3 py-1 rounded border border-ash text-sm text-parchment/70 hover:text-parchment" @click="emit('close')">
          {{ t("editor.cropCancel") }}
        </button>
        <button
          class="rounded bg-ember/80 px-3 py-1 text-sm font-bold text-ink hover:bg-ember disabled:opacity-40"
          :disabled="busy"
          @click="apply"
        >
          {{ busy ? t("editor.cropApplying") : t("editor.cropApply") }}
        </button>
      </div>
    </div>
  </div>
</template>
