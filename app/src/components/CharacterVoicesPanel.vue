<script setup lang="ts">
/**
 * キャラクターの声 (spec 35 Phase D) — 会話ペイン右下の読み上げの操作から開く浮遊パネル。
 *
 * 声は「いま喋っている人物を見ながら」選ぶものなので、設定画面の奥ではなく、読み上げの操作の
 * 隣に置く (ユーザーFB 2026-10-15)。器は参照ストックやプロンプト工房と同じ FloatingPanel
 * (幕を張らない = 会話を見ながら触れる)。
 *
 * 一覧はいま遊んでいるパッケージの登場人物 (GameView.cast)。割り当てはエンジン + URL と
 * パッケージの鍵で保存する (`tts.saveCharacterVoice`)。エンジンや速度は設定 > サウンドのまま。
 */
import { computed, ref, watch } from "vue";
import { useGameStore } from "../stores/game";
import { t } from "../i18n";
import * as tts from "../tts";
import FloatingPanel from "./FloatingPanel.vue";

const emit = defineEmits<{ (e: "close"): void }>();
const game = useGameStore();

// 開いた時点の読み上げ設定 (エンジン・URL・語り手)。変えるのは設定 > サウンド。
const settings = ref<tts.TtsSettings>(tts.loadSettings());
const voiceList = ref<tts.VoiceOption[]>(tts.loadVoiceList(settings.value));
const hasList = computed(() => tts.supportsVoiceList(settings.value.engine));
const pkg = computed(() => game.activePackagePath);
const assigned = ref<Record<string, string>>(tts.loadCharacterVoices(settings.value, pkg.value));
watch(pkg, () => {
  assigned.value = tts.loadCharacterVoices(settings.value, pkg.value);
});
const status = ref("");
const busy = ref(false);

const ENGINE_LABEL: Record<tts.TtsEngine, string> = {
  webSpeech: "settings.voice.engineWebSpeech",
  voicevox: "settings.voice.engineVoicevox",
  aivisSpeech: "settings.voice.engineAivis",
  openaiCompatible: "settings.voice.engineOpenai",
};
const engineLabel = computed(() => t(ENGINE_LABEL[settings.value.engine]));

function setVoice(entity: string, voiceId: string) {
  tts.saveCharacterVoice(settings.value, pkg.value, entity, voiceId);
  assigned.value = tts.loadCharacterVoices(settings.value, pkg.value);
}

/** 一覧に無い割り当て (別のサーバーで取った ID 等) も選択肢として見せる (空白の select を作らない)。 */
function orphan(entity: string): string {
  const id = assigned.value[entity] ?? "";
  return id && !voiceList.value.some((v) => v.id === id) ? id : "";
}

/** 声の一覧の取得。ローカルエンジンはサーバーへ問い合わせるので失敗を表に出す。 */
async function loadList() {
  busy.value = true;
  status.value = "";
  try {
    voiceList.value = await tts.listVoices(settings.value);
    tts.saveVoiceList(settings.value, voiceList.value);
    status.value = t("settings.voice.ok");
  } catch (e) {
    status.value = t("settings.voice.failed", { msg: String(e) });
  } finally {
    busy.value = false;
  }
}

/** 試聴。失敗をそのまま見せる (サーバー未起動を無音で誤魔化さない)。 */
async function testVoice(entity: string) {
  busy.value = true;
  status.value = "";
  try {
    await tts.test(t("settings.voice.testSample"), assigned.value[entity] || undefined);
    status.value = t("settings.voice.ok");
  } catch (e) {
    status.value = t("settings.voice.failed", { msg: String(e) });
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <FloatingPanel :title="t('settings.voice.characters')" width="30rem" @close="emit('close')">
    <div class="space-y-3 text-sm" data-testid="character-voices">
      <p class="text-parchment/60 text-xs leading-relaxed">{{ t("settings.voice.charactersNote") }}</p>
      <p class="text-parchment/50 text-xs">{{ t("settings.voice.panelEngine", { engine: engineLabel }) }}</p>
      <p v-if="!game.cast.length" class="text-parchment/50 text-xs">{{ t("settings.voice.charactersEmpty") }}</p>
      <div
        v-for="c in game.cast"
        :key="c.id"
        class="flex items-center gap-2"
        data-testid="character-voice-row"
      >
        <span class="w-24 shrink-0 truncate text-parchment/85" :title="`${c.name} (${c.id})`">{{ c.name }}</span>
        <input
          v-if="!hasList"
          :value="assigned[c.id] ?? ''"
          type="text"
          :placeholder="t('settings.voice.sameAsNarrator')"
          class="min-w-0 flex-1 rounded bg-ink/70 border border-ash/60 px-2 py-1 text-parchment"
          @change="setVoice(c.id, ($event.target as HTMLInputElement).value)"
        />
        <select
          v-else
          :value="assigned[c.id] ?? ''"
          class="min-w-0 flex-1 truncate rounded bg-ink/70 border border-ash/60 px-2 py-1 text-parchment"
          @change="setVoice(c.id, ($event.target as HTMLSelectElement).value)"
        >
          <option value="">{{ t("settings.voice.sameAsNarrator") }}</option>
          <option v-if="orphan(c.id)" :value="orphan(c.id)">{{ orphan(c.id) }}</option>
          <option v-for="v in voiceList" :key="v.id" :value="v.id" :title="v.label">{{ v.label }}</option>
        </select>
        <button
          type="button"
          class="px-2 py-1 text-xs rounded bg-ash/40 text-parchment/80 hover:bg-ash/60 disabled:opacity-40"
          :disabled="busy"
          @click="testVoice(c.id)"
        >
          {{ t("settings.voice.testCharacter") }}
        </button>
      </div>
      <div class="flex items-center gap-2 pt-1">
        <button
          v-if="hasList"
          type="button"
          class="px-2 py-1 text-xs rounded bg-ash/40 text-parchment/80 hover:bg-ash/60 disabled:opacity-40"
          :disabled="busy"
          @click="loadList"
        >
          {{ t("settings.voice.loadVoices") }}
        </button>
        <span v-if="status" class="text-xs text-parchment/60 break-all">{{ status }}</span>
      </div>
    </div>
  </FloatingPanel>
</template>
