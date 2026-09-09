<script setup lang="ts">
/**
 * 初回起動のナビゲーション (2026-09-07、ユーザー要望)。
 *
 * 「ゲームの始め方」を 4 歩で示す。挨拶の幕 (welcome) のあと、**実物の UI 要素**を
 * スポットライトが順に照らし (対象の間を滑って移動する)、横に番号つきのカードが出る。
 * 対象は `data-tour="…"` 属性で印を付けた要素で、この部品は `document.querySelector` で
 * 測るだけ = 対象側の部品はこの部品を知らない。
 *
 * 軽さの線引き: 依存ゼロ・CSS のトランジションと keyframes だけ・動的 import で初回にしか
 * 読まれない (App.vue)。見たかどうかの印は localStorage (`tour.ts` の TOUR_DONE_KEY)。
 *
 * 案内の中から設定やパッケージ一覧を開くボタンは置かない (2026-09-07 ユーザー決定) —
 * 開いた瞬間に案内は終わるので、案内の中に置く意味が薄い。読み終えてから自分で開く。
 */
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { t } from "../i18n";
import { placeCard, spotlightBox, TOUR_DONE_KEY, TOUR_STEPS, type Box, type CardPlacement } from "../tour";

const emit = defineEmits<{ (e: "close"): void }>();

const phase = ref<"welcome" | "tour">("welcome");
const idx = ref(0);
const step = computed(() => TOUR_STEPS[idx.value]);
const total = TOUR_STEPS.length;

// スポットライトの矩形とカードの置き場。対象が見つからないときは画面中央に小さく置く
// (要素が無い = レイアウトが変わった、でも案内は続けられる)。
const spot = ref<Box>({ left: 0, top: 0, width: 0, height: 0 });
const card = ref<CardPlacement>({ left: 0, top: 0, side: "below", arrow: 18 });
const cardEl = ref<HTMLElement | null>(null);
const measured = ref(false);

/** カードの幅は CSS (22rem / 最大 92vw) から決まるので、実寸を測れない瞬間もこの値で置ける
 *  (2026-09-07 実機で、既定 352px のまま置いて実寸 396px が右に切れた)。 */
function cardWidthFromCss(vpWidth: number): number {
  const rem = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
  return Math.min(22 * rem, vpWidth * 0.92);
}

/** `sizeEl` = 寸法を測るカード要素。Transition の enter 中は ref がまだ新しい要素を
 *  指していないことがあるので、フックから渡された要素を優先する。 */
function measure(sizeEl?: HTMLElement | null) {
  const el = document.querySelector<HTMLElement>(`[data-tour="${step.value}"]`);
  const vp = { width: window.innerWidth, height: window.innerHeight };
  if (el) {
    const r = el.getBoundingClientRect();
    spot.value = spotlightBox({ left: r.left, top: r.top, width: r.width, height: r.height }, 8);
  } else {
    spot.value = { left: vp.width / 2 - 24, top: vp.height / 2 - 24, width: 48, height: 48 };
  }
  const src = sizeEl ?? cardEl.value;
  const size =
    src && src.offsetWidth > 0
      ? { width: src.offsetWidth, height: src.offsetHeight }
      : { width: cardWidthFromCss(vp.width), height: 180 };
  card.value = placeCard(spot.value, vp, size, 14);
  measured.value = true;
}

async function remeasure() {
  await nextTick();
  measure();
  // カードの中身 (文言の長さ) で高さが変わるので、描画後にもう一度置き直す。
  await nextTick();
  measure();
}

function begin() {
  phase.value = "tour";
  idx.value = 0;
  void remeasure();
}
function next() {
  if (idx.value + 1 >= total) return finish();
  idx.value += 1;
  void remeasure();
}
function back() {
  if (idx.value === 0) return;
  idx.value -= 1;
  void remeasure();
}
function finish() {
  try {
    localStorage.setItem(TOUR_DONE_KEY, "1");
  } catch {
    /* storage が書けない環境でも案内自体は閉じる */
  }
  emit("close");
}
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") {
    e.preventDefault();
    finish();
  } else if (e.key === "Enter" || e.key === "ArrowRight") {
    e.preventDefault();
    if (phase.value === "welcome") begin();
    else next();
  } else if (e.key === "ArrowLeft" && phase.value === "tour") {
    e.preventDefault();
    back();
  }
}

function onResize() {
  measure();
}
onMounted(() => {
  window.addEventListener("keydown", onKey);
  window.addEventListener("resize", onResize);
});
onUnmounted(() => {
  window.removeEventListener("keydown", onKey);
  window.removeEventListener("resize", onResize);
});
watch(phase, () => void remeasure());

const spotStyle = computed(() => ({
  left: `${spot.value.left}px`,
  top: `${spot.value.top}px`,
  width: `${spot.value.width}px`,
  height: `${spot.value.height}px`,
}));
const cardStyle = computed(() => ({
  left: `${card.value.left}px`,
  top: `${card.value.top}px`,
  "--arrow": `${card.value.arrow}px`,
}));
</script>

<template>
  <div class="fixed inset-0 z-[70] tour-root" role="dialog" aria-modal="true" :aria-label="t('tour.welcomeTitle')">
    <!-- ===== 挨拶の幕 ===== -->
    <Transition name="tour-fade">
      <div v-if="phase === 'welcome'" class="absolute inset-0 tour-welcome flex items-center justify-center" @click.self="begin">
        <div class="tour-welcome-card w-[34rem] max-w-[92vw] text-center px-8 py-10">
          <p class="tour-brand text-3xl font-bold tracking-wide">
            <span class="outcasts-word">Outcasts</span> Lorekeel
          </p>
          <h2 class="mt-4 text-xl font-bold text-parchment tour-rise" style="--d: 0.35s">{{ t("tour.welcomeTitle") }}</h2>
          <p class="mt-2 text-sm text-parchment/70 leading-relaxed tour-rise" style="--d: 0.55s">{{ t("tour.welcomeLead") }}</p>
          <ol class="mt-6 grid grid-cols-4 gap-2 text-left">
            <li
              v-for="(s, i) in TOUR_STEPS"
              :key="s"
              class="tour-mini rounded-lg border border-ash/70 bg-ink/60 p-3"
              :style="{ '--d': `${0.8 + i * 0.18}s` }"
            >
              <span class="tour-num">{{ i + 1 }}</span>
              <span class="mt-1.5 block text-[11px] leading-snug text-parchment/80">{{ t(`tour.steps.${s}.title`) }}</span>
            </li>
          </ol>
          <div class="mt-7 flex items-center justify-center gap-3 tour-rise" style="--d: 1.6s">
            <button class="tour-btn-primary" autofocus @click="begin">{{ t("tour.welcomeStart") }}</button>
            <button class="tour-btn-ghost" @click="finish">{{ t("tour.skip") }}</button>
          </div>
          <p class="mt-3 text-[10px] text-parchment/35 tour-rise" style="--d: 1.8s">{{ t("tour.keys") }}</p>
        </div>
      </div>
    </Transition>

    <!-- ===== コーチマーク ===== -->
    <template v-if="phase === 'tour'">
      <!-- スポットライト: 巨大な box-shadow で周囲を暗くし、対象だけ素の UI が見える。
           left/top/width/height にトランジション = 対象の間を滑って移動する。 -->
      <div class="tour-spot" :class="{ 'tour-spot-ready': measured }" :style="spotStyle" @click.stop="next">
        <span class="tour-pulse" aria-hidden="true" />
      </div>
      <!-- 暗幕のどこをクリックしても次へ (対象以外は操作させない) -->
      <div class="absolute inset-0" @click="next" />

      <!-- out-in なので新しいカードは古いのが消えてから入る = 入った要素の実寸で置き直す
           (ref はこの時点でまだ古い要素を指しうるので、フックの要素を渡す) -->
      <Transition name="tour-card" mode="out-in" @enter="(el) => measure(el as HTMLElement)">
        <div
          :key="step"
          ref="cardEl"
          class="tour-card w-[22rem] max-w-[92vw]"
          :class="`tour-card-${card.side}`"
          :style="cardStyle"
          @click.stop
        >
          <div class="flex items-center gap-3">
            <span class="tour-num tour-num-lg">{{ idx + 1 }}</span>
            <h3 class="text-base font-bold text-parchment leading-tight">{{ t(`tour.steps.${step}.title`) }}</h3>
            <span class="ml-auto text-[10px] text-parchment/40 tabular-nums">{{ idx + 1 }} / {{ total }}</span>
          </div>
          <p class="mt-2 text-sm text-parchment/80 leading-relaxed">{{ t(`tour.steps.${step}.body`) }}</p>
          <div class="mt-4 flex items-center gap-2">
            <button v-if="idx + 1 < total" class="tour-btn-primary" @click="next">{{ t("tour.next") }}</button>
            <button v-else class="tour-btn-primary" @click="finish">{{ t("tour.finish") }}</button>
            <button v-if="idx > 0" class="tour-btn-ghost" @click="back">{{ t("tour.back") }}</button>
            <button class="ml-auto text-[11px] text-parchment/40 hover:text-parchment" @click="finish">{{ t("tour.skip") }}</button>
          </div>
          <!-- 進み具合の点 -->
          <div class="mt-3 flex gap-1.5">
            <span v-for="(s, i) in TOUR_STEPS" :key="s" class="tour-dot" :class="{ on: i <= idx }" />
          </div>
        </div>
      </Transition>
    </template>
  </div>
</template>

<style scoped>
/* 挨拶の幕: 焦げ茶の黒に、下から熾火の光がゆっくり立ちのぼる。 */
.tour-welcome {
  background:
    radial-gradient(ellipse 70% 55% at 50% 110%, rgb(var(--ember) / 0.28), transparent 70%),
    rgb(var(--ink) / 0.94);
  animation: tour-breathe 6s ease-in-out infinite alternate;
}
@keyframes tour-breathe {
  from {
    filter: brightness(1);
  }
  to {
    filter: brightness(1.12);
  }
}
.tour-welcome-card {
  border-radius: 1rem;
  border: 1px solid rgb(var(--ash) / 0.7);
  background: linear-gradient(to bottom, rgb(var(--ash) / 0.35), rgb(var(--ink) / 0.85));
  box-shadow: 0 30px 80px rgb(0 0 0 / 0.5);
  animation: tour-card-in 0.6s cubic-bezier(0.2, 0.8, 0.2, 1) both;
}
.tour-brand {
  color: rgb(var(--glow));
  text-shadow: 0 0 14px rgb(var(--ember) / 0.55);
  animation: tour-card-in 0.8s cubic-bezier(0.2, 0.8, 0.2, 1) both;
}
/* 段階的に浮き上がる (--d で遅らせる) */
.tour-rise,
.tour-mini {
  animation: tour-rise 0.55s cubic-bezier(0.2, 0.8, 0.2, 1) both;
  animation-delay: var(--d, 0s);
}
@keyframes tour-rise {
  from {
    opacity: 0;
    transform: translateY(10px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
@keyframes tour-card-in {
  from {
    opacity: 0;
    transform: translateY(14px) scale(0.98);
  }
  to {
    opacity: 1;
    transform: none;
  }
}

/* 番号の玉 */
.tour-num {
  display: inline-grid;
  place-items: center;
  width: 1.5rem;
  height: 1.5rem;
  border-radius: 9999px;
  background: rgb(var(--ember));
  color: rgb(var(--ink));
  font-weight: 700;
  font-size: 0.75rem;
  box-shadow: 0 0 10px rgb(var(--ember) / 0.6);
}
.tour-num-lg {
  width: 1.9rem;
  height: 1.9rem;
  font-size: 0.9rem;
}

/* スポットライト: 巨大な影で周囲を暗くする (別の暗幕要素を重ねない = 穴が正確)。 */
.tour-spot {
  position: fixed;
  border-radius: 0.75rem;
  box-shadow:
    0 0 0 200vmax rgb(8 6 4 / 0.74),
    0 0 0 2px rgb(var(--ember) / 0.9),
    0 0 24px 4px rgb(var(--ember) / 0.45);
  transition:
    left 0.55s cubic-bezier(0.2, 0.8, 0.2, 1),
    top 0.55s cubic-bezier(0.2, 0.8, 0.2, 1),
    width 0.55s cubic-bezier(0.2, 0.8, 0.2, 1),
    height 0.55s cubic-bezier(0.2, 0.8, 0.2, 1);
  cursor: pointer;
  opacity: 0;
}
.tour-spot-ready {
  opacity: 1;
  transition:
    opacity 0.3s ease,
    left 0.55s cubic-bezier(0.2, 0.8, 0.2, 1),
    top 0.55s cubic-bezier(0.2, 0.8, 0.2, 1),
    width 0.55s cubic-bezier(0.2, 0.8, 0.2, 1),
    height 0.55s cubic-bezier(0.2, 0.8, 0.2, 1);
}
/* 対象の周りで脈打つ輪 (注意を引く。無限だが 1 要素なので軽い) */
.tour-pulse {
  position: absolute;
  inset: -4px;
  border-radius: inherit;
  border: 2px solid rgb(var(--glow) / 0.8);
  animation: tour-pulse 1.6s ease-out infinite;
  pointer-events: none;
}
@keyframes tour-pulse {
  from {
    opacity: 0.9;
    transform: scale(1);
  }
  to {
    opacity: 0;
    transform: scale(1.18);
  }
}

/* カード */
.tour-card {
  position: fixed;
  border-radius: 0.9rem;
  border: 1px solid rgb(var(--ash));
  background: rgb(var(--ink));
  color: rgb(var(--parchment));
  padding: 1rem 1.1rem;
  box-shadow: 0 24px 60px rgb(0 0 0 / 0.55);
}
/* 対象を指す小さな三角 */
.tour-card::before {
  content: "";
  position: absolute;
  width: 12px;
  height: 12px;
  background: rgb(var(--ink));
  border: 1px solid rgb(var(--ash));
  transform: rotate(45deg);
}
/* 三角の位置は --arrow (placeCard が対象の中心から計算) */
.tour-card-below::before {
  top: -7px;
  left: calc(var(--arrow, 22px) - 6px);
  border-right: 0;
  border-bottom: 0;
}
.tour-card-above::before {
  bottom: -7px;
  left: calc(var(--arrow, 22px) - 6px);
  border-left: 0;
  border-top: 0;
}
.tour-card-right::before {
  left: -7px;
  top: calc(var(--arrow, 22px) - 6px);
  border-right: 0;
  border-top: 0;
}
.tour-card-left::before {
  right: -7px;
  top: calc(var(--arrow, 22px) - 6px);
  border-left: 0;
  border-bottom: 0;
}
.tour-card-enter-active,
.tour-card-leave-active {
  transition:
    opacity 0.22s ease,
    transform 0.22s ease;
}
.tour-card-enter-from {
  opacity: 0;
  transform: translateY(8px);
}
.tour-card-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}
.tour-fade-leave-active {
  transition: opacity 0.35s ease;
}
.tour-fade-leave-to {
  opacity: 0;
}

.tour-dot {
  width: 1.4rem;
  height: 3px;
  border-radius: 9999px;
  background: rgb(var(--ash));
  transition: background-color 0.3s ease;
}
.tour-dot.on {
  background: rgb(var(--ember));
}

.tour-btn-primary {
  border-radius: 0.5rem;
  background: rgb(var(--ember));
  color: rgb(var(--ink));
  font-weight: 700;
  font-size: 0.85rem;
  padding: 0.4rem 0.9rem;
  box-shadow: 0 0 14px rgb(var(--ember) / 0.45);
  transition:
    transform 0.15s ease,
    box-shadow 0.15s ease;
}
.tour-btn-primary:hover {
  transform: translateY(-1px);
  box-shadow: 0 0 22px rgb(var(--ember) / 0.7);
}
.tour-btn-ghost {
  border-radius: 0.5rem;
  border: 1px solid rgb(var(--ash));
  /* 減光は @apply で通す (手書きの alpha はテーマ間で同じ強さにならない)。 */
  @apply text-parchment/80;
  font-size: 0.85rem;
  padding: 0.4rem 0.9rem;
}
.tour-btn-ghost:hover {
  background: rgb(var(--ash) / 0.5);
}

/*
 * ライトテーマ (2026-09-08、ユーザー報告「ナビゲーションの文字がダークと同じで読めない」)。
 *
 * 真因は色ではなく**層の食い違い**だった — 幕と札の背景が暗色の直書き (`rgb(21 17 14 / …)`)
 * で、文字だけが `--parchment` (ライトでは焦げ茶) を引いていた = 暗い幕に暗い字。上で背景を
 * 変数へ寄せたので字は読めるが、それだけだと**幕も札もクリームで境目が消える** (ダークでは
 * 半透明の札が暗い幕から浮くという作りだった)。ライトでは浮かせ方を変える:
 * 札は不透明・罫線をはっきり・影は黒でなく焦げ茶の薄い影。
 */
[data-theme="light"] .tour-welcome {
  /* 熾火は残すが量を落とす (濃い ember を厚く敷くとクリームの上では濁る) */
  background:
    radial-gradient(ellipse 70% 55% at 50% 110%, rgb(var(--ember) / 0.16), transparent 70%),
    rgb(var(--ink) / 0.96);
}
[data-theme="light"] .tour-welcome-card {
  background: linear-gradient(to bottom, rgb(var(--ember) / 0.07), rgb(var(--ink)));
  border-color: rgb(var(--ash));
  box-shadow: 0 24px 60px rgb(var(--parchment) / 0.18);
}
[data-theme="light"] .tour-mini {
  /* クリームの札の上にクリームの升だと沈むので、升の側を一段濃くする */
  background: rgb(var(--ash) / 0.45);
  border-color: rgb(var(--ash));
}
[data-theme="light"] .tour-card {
  box-shadow: 0 24px 60px rgb(var(--parchment) / 0.28);
}
[data-theme="light"] .tour-brand {
  /* 発光は暗地でしか効かない (明地では滲んで太って見えるだけ) */
  text-shadow: none;
}

/* 動きを減らす設定の人には滑走と脈動を止める (位置は即時に決まる) */
@media (prefers-reduced-motion: reduce) {
  .tour-spot,
  .tour-spot-ready,
  .tour-rise,
  .tour-mini,
  .tour-welcome-card,
  .tour-brand,
  .tour-welcome {
    transition: none;
    animation: none;
  }
  .tour-pulse {
    animation: none;
    opacity: 0.6;
  }
}
</style>
