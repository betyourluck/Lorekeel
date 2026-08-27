<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount, defineAsyncComponent } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useGameStore } from "../stores/game";
import { t } from "../i18n";
import Icon from "./Icon.vue";
import MapPanel from "./MapPanel.vue";
import FactsPanel from "./FactsPanel.vue";

const game = useGameStore();

// 右ペインは縦タブ 5 枚 (progress=進行: ターン/目標/この場 ・ world=状態: 現在地/所持品/フラグ
// ・ map=マップ: 訪問済み+1歩先の有向グラフ、spec 15 ・ synopsis=あらすじ: 圧縮済み章 +
// 最近の出来事、spec 10 ・ facts=既成事実: GM とユーザーの覚え書き、spec 20)。
// 既成事実は末尾 (ユーザーFB 2026-07-21)。
const TABS = ["progress", "world", "map", "synopsis", "facts", "files"] as const;
type Tab = (typeof TABS)[number];
const activeTab = ref<Tab>("progress");

// facts_policy=locked の盤面では既成事実は GM 専用の内部記憶 — タブごと出さない (spec 20 Phase E)。
// 表示中に locked へ変わる (campaign 遷移) 場合に備え、選択中なら進行タブへ逃がす。
const factsVisible = computed(() => game.factsPolicy !== "locked");
watch(factsVisible, (v) => {
  if (!v && activeTab.value === "facts") activeTab.value = "progress";
});

// 編集モード (spec 28): ファイルタブは編集モード中だけ。ON で自動的に開き、OFF で
// **入る直前にいたタブへ戻す** (progress 固定に飛ばさない — 査読反映)。
const filesVisible = computed(() => game.editor.on);
let tabBeforeEdit: Tab = "progress";
watch(filesVisible, (v) => {
  if (v) {
    tabBeforeEdit = activeTab.value;
    activeTab.value = "files";
  } else if (activeTab.value === "files") {
    activeTab.value = tabBeforeEdit === "facts" && !factsVisible.value ? "progress" : tabBeforeEdit;
  }
});

// ファイル一覧は**固定カテゴリ順** (2026-08-27 ユーザーFB: + は各カテゴリのグループ内へ)。
// ファイルが 0 でも作成できるカテゴリはグループごと出す (フォルダ不在でも始められる)。
const CATEGORY_ORDER = ["package", "campaign", "scenario", "character", "memoria"] as const;
const editorGroups = computed(() =>
  CATEGORY_ORDER.map((category) => ({
    category: category as string,
    files: game.editor.files.filter((f) => f.category === category),
    // package は作成不可 (常に在る土台)。campaign は固定名 1 枚なので不在時だけ。
    creatable: category !== "package" && (category !== "campaign" || !game.editor.files.some((f) => f.category === "campaign")),
  })).filter((g) => g.files.length > 0 || g.creatable),
);

// --- メディア (spec 28 追補、2026-08-28) ---
// 参照専用の一覧 + ドラッグ投入 + 画像のクロップ。アセット ID = ファイル名そのもの。
const CropDialog = defineAsyncComponent(() => import("./CropDialog.vue"));
const cropping = ref<{ src: string; relPath: string } | null>(null);
const dropping = ref(false);

const mediaGroups = computed(() =>
  (["image", "audio"] as const)
    .map((category) => ({
      category: category as string,
      files: game.editor.media.filter((f) => f.category === category),
    }))
    .filter((g) => g.files.length > 0),
);

/** サムネイル/クロップ用の URL。**mediaRev を付ける** — クロップは名前を保ったまま
 *  中身を替えるので、付けないと WebView のキャッシュが古い絵を出す (failures #86 と同型)。 */
function mediaUrl(relPath: string): string {
  const sep = game.editor.absRoot.includes("\\") ? "\\" : "/";
  const abs = `${game.editor.absRoot}${sep}${relPath.split("/").join(sep)}`;
  return `${convertFileSrc(abs)}?v=${game.editor.mediaRev}`;
}

/** アセット ID をクリップボードへ (YAML の image/bgm/sound/icon 欄へ貼るため)。 */
async function copyAssetId(relPath: string) {
  const id = relPath.split("/").pop() ?? relPath;
  try {
    await navigator.clipboard.writeText(id);
    game.logToast = t("editor.assetCopied", { id });
  } catch {
    game.logToast = id; // クリップボードが使えない環境では名前だけ見せる
  }
}

function onDrop(e: DragEvent) {
  dropping.value = false;
  const files = Array.from(e.dataTransfer?.files ?? []);
  if (files.length) void game.putEditorMedia(files);
}

// 新規ファイル (spec 28 Phase D → 4 カテゴリへ一般化)。+ → inline 入力 → Enter/作成。
// campaign は固定名なので入力を出さず即作成。
const newFileCat = ref<string | null>(null);
const newFileStem = ref("");
function openNewFile(category: string) {
  if (category === "campaign") {
    void game.createEditorFile("campaign", "");
    return;
  }
  newFileCat.value = category;
  newFileStem.value = "";
}
async function submitNewFile() {
  const cat = newFileCat.value;
  const stem = newFileStem.value.trim();
  if (!cat || !stem) return;
  await game.createEditorFile(cat, stem);
  // 成功したら current が新ファイルを指す。失敗はトーストが出るので入力は残す。
  if (game.editor.current.endsWith(`/${stem}.yaml`)) {
    newFileCat.value = null;
    newFileStem.value = "";
  }
}

// 顔アイコンをクリックして詳細を見るキャラ (presence → クリックでプロフィール)。
const selectedId = ref<string | null>(null);
// 多人数 (spec 23): この entity を操作するプレイヤー (卓開始後のみ)。席色リングと
// 「プレイヤー: ○○」表示の素材。単騎では常に undefined = 何も出ない。
const assignmentOf = (id: string) => game.multi.assignments.find((a) => a.entityId === id);
// 席色リングは発話レベルで脈動する (spec 23 Phase D)。Discord の緑リングと同じ機構を
// ユーザーリストでなく**キャラ側**に載せる = プレイヤーの声で自分の手駒が脈打つ。
// 基底 2px + 音量で最大 +4px。レベルは ~12Hz 更新なので滑らかさは CSS の transition に任せる。
// マイク完全解放中は自分のレベルが来ないのでリングは静止する (OS 表示と一致)。
const seatRing = (id: string) => {
  const a = assignmentOf(id);
  if (!a) return {};
  const level = game.multi.voiceLevels[id] ?? 0;
  const spread = 2 + Math.round(level * 4);
  return {
    borderColor: a.color,
    boxShadow: `0 0 0 ${spread}px ${a.color}`,
    transition: "box-shadow 80ms linear",
  };
};
const selectedEntity = computed(
  () => game.state?.entities.find((e) => e.id === selectedId.value) ?? null,
);
const selectedName = computed(
  () => game.presentCharacters.find((c) => c.id === selectedId.value)?.name ?? selectedId.value ?? "",
);
// ダイアログヘッダの顔アイコン (presence 行と同じ解決済み URL を使い回す)。
const selectedIcon = computed(
  () => game.presentCharacters.find((c) => c.id === selectedId.value)?.icon ?? null,
);
const selectedIsEmpty = computed(() => {
  const e = selectedEntity.value;
  return (
    !!e &&
    !e.stats.length &&
    !e.attributes.length &&
    !e.skills.length &&
    !e.items.length &&
    !e.profile
  );
});
// profile 本文はもう 1 ステップ奥 (ダイアログ内のアイコンクリックで開く)。キャラ切替でリセット。
const showProfile = ref(false);
watch(selectedId, () => {
  showProfile.value = false;
});
function initials(name: string): string {
  return name.trim().slice(0, 2);
}
function onKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") selectedId.value = null;
  // IME 変換中はショートカットを発火させない (変換候補操作のキーを奪わない)。
  if (e.isComposing) return;
  if (!e.ctrlKey || e.altKey || e.metaKey) return;
  // locked 盤面では既成事実タブは、編集モード外ではファイルタブは、存在しない扱い
  // (巡回にも直接選択にも出さない)。
  const tabs = TABS.filter(
    (x) => (x !== "facts" || factsVisible.value) && (x !== "files" || filesVisible.value),
  );
  if (e.key === "Tab") {
    // Ctrl+Tab: タブ巡回 (Shift 併用で逆順)。
    e.preventDefault();
    const i = tabs.indexOf(activeTab.value);
    const step = e.shiftKey ? tabs.length - 1 : 1;
    activeTab.value = tabs[(i + step) % tabs.length];
  } else if (["1", "2", "3", "4", "5"].includes(e.key)) {
    // Ctrl+1..5: 直接選択 (5 枚巡回は遠いので直接選択が主導線)。
    e.preventDefault();
    const target = tabs[Number(e.key) - 1];
    if (target) activeTab.value = target;
  }
}
onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
// 顔アイコンを参照ストックの枠へドラッグできる (spec 27 追補、2026-08-26 ユーザー要望)。
// **spec 25 が捨てた「誰の参照か」を、機械でなく人が置く形で取り戻す経路** — 運ぶのは
// アセット ID だけで、bytes は backend が読む。ID を持たない (アイコン未設定の) キャラは
// draggable にしない = 掴めない見た目が「置けない」を先に伝える。
function onIconDragStart(c: { iconId?: string | null }, e: DragEvent) {
  if (!c.iconId || !e.dataTransfer) return;
  e.dataTransfer.setData("application/x-kataribe-asset", c.iconId);
  e.dataTransfer.effectAllowed = "copy";
}

</script>

<template>
  <aside
    class="shrink-0 border-l border-ash bg-ink/60 text-sm flex"
    :style="{ width: game.panelWidth + 'px' }"
  >
    <!-- 縦タブ rail: 全体スクロールを避けるため 2 枚に分ける (進行 / 状態)。 -->
    <!-- rail は背景・罫線なし (透明)、タブは普段半透明で控えめに。 -->
    <nav class="w-7 shrink-0 flex flex-col items-stretch pt-2 gap-0.5">
      <button
        class="flex flex-col items-center gap-1 py-2 border-l-2 transition-opacity focus:outline-none"
        :class="
          activeTab === 'progress'
            ? 'border-ember text-glow'
            : 'border-transparent text-parchment opacity-40 hover:opacity-90'
        "
        :title="t('state.tabProgressTitle')"
        @click="activeTab = 'progress'"
      >
        <Icon name="target" :size="12" />
        <span class="text-[9px] tracking-widest" style="writing-mode: vertical-rl">{{ t("state.tabProgress") }}</span>
      </button>
      <button
        class="flex flex-col items-center gap-1 py-2 border-l-2 transition-opacity focus:outline-none"
        :class="
          activeTab === 'world'
            ? 'border-ember text-glow'
            : 'border-transparent text-parchment opacity-40 hover:opacity-90'
        "
        :title="t('state.tabWorldTitle')"
        @click="activeTab = 'world'"
      >
        <Icon name="location" :size="12" />
        <span class="text-[9px] tracking-widest" style="writing-mode: vertical-rl">{{ t("state.tabWorld") }}</span>
      </button>
      <button
        class="flex flex-col items-center gap-1 py-2 border-l-2 transition-opacity focus:outline-none"
        :class="
          activeTab === 'map'
            ? 'border-ember text-glow'
            : 'border-transparent text-parchment opacity-40 hover:opacity-90'
        "
        :title="t('state.tabMapTitle')"
        @click="activeTab = 'map'"
      >
        <Icon name="map" :size="12" />
        <span class="text-[9px] tracking-widest" style="writing-mode: vertical-rl">{{ t("state.tabMap") }}</span>
      </button>
      <button
        class="flex flex-col items-center gap-1 py-2 border-l-2 transition-opacity focus:outline-none"
        :class="
          activeTab === 'synopsis'
            ? 'border-ember text-glow'
            : 'border-transparent text-parchment opacity-40 hover:opacity-90'
        "
        :title="t('state.tabSynopsisTitle')"
        @click="activeTab = 'synopsis'"
      >
        <Icon name="book" :size="12" />
        <span class="text-[9px] tracking-widest" style="writing-mode: vertical-rl">{{ t("state.tabSynopsis") }}</span>
      </button>
      <!-- 既成事実 (spec 20)。locked 盤面では GM 専用の内部記憶 = タブごと出さない。 -->
      <button
        v-if="factsVisible"
        class="flex flex-col items-center gap-1 py-2 border-l-2 transition-opacity focus:outline-none"
        :class="
          activeTab === 'facts'
            ? 'border-ember text-glow'
            : 'border-transparent text-parchment opacity-40 hover:opacity-90'
        "
        :title="t('state.tabFactsTitle')"
        @click="activeTab = 'facts'"
      >
        <Icon name="pencil" :size="12" />
        <span class="text-[9px] tracking-widest" style="writing-mode: vertical-rl">{{ t("state.tabFacts") }}</span>
      </button>
      <!-- ファイル (spec 28): 編集モード中のみ。パッケージ内の YAML 一覧。 -->
      <button
        v-if="filesVisible"
        class="flex flex-col items-center gap-1 py-2 border-l-2 transition-opacity focus:outline-none"
        :class="
          activeTab === 'files'
            ? 'border-ember text-glow'
            : 'border-transparent text-parchment opacity-40 hover:opacity-90'
        "
        :title="t('state.tabFilesTitle')"
        @click="activeTab = 'files'"
      >
        <Icon name="folder" :size="12" />
        <span class="text-[9px] tracking-widest" style="writing-mode: vertical-rl">{{ t("state.tabFiles") }}</span>
      </button>
    </nav>

    <div class="flex-1 min-w-0 p-4 overflow-y-auto flex flex-col">
      <!-- ファイル一覧 (spec 28)。**game.state に依存しない** — 編集はプレイしていなくてもできる。 -->
      <template v-if="activeTab === 'files'">
        <!-- テキスト / メディアの切替 (2026-08-28 ユーザーFB)。編集できるのはテキストだけで、
             メディアは参照 (アセット ID を見る) と管理 (追加・クロップ・削除) の面。 -->
        <div class="mb-3 flex rounded border border-ash text-xs">
          <button
            v-for="v in (['text', 'media'] as const)"
            :key="v"
            class="flex-1 py-1 transition-colors first:rounded-l last:rounded-r"
            :class="game.editor.view === v ? 'bg-ember/20 text-glow' : 'text-parchment/50 hover:text-parchment'"
            @click="game.editor.view = v"
          >
            {{ t(`editor.view_${v}`) }}
          </button>
        </div>

        <!-- メディア: 参照専用の一覧。クリックでアセット ID をコピー (YAML へ貼る)。 -->
        <template v-if="game.editor.view === 'media'">
          <div
            class="mb-3 rounded border border-dashed px-3 py-4 text-center text-xs transition-colors"
            :class="dropping ? 'border-ember bg-ember/10 text-glow' : 'border-ash text-parchment/40'"
            @dragover.prevent="dropping = true"
            @dragleave="dropping = false"
            @drop.prevent="onDrop"
          >
            {{ t("editor.mediaDropHint") }}
          </div>
          <div v-for="g in mediaGroups" :key="g.category" class="mb-3">
            <div class="text-parchment/40 mb-1.5 flex items-center gap-1.5 text-xs">
              <Icon name="folder" :size="12" />{{ t(`editor.cat_${g.category}`) }}
            </div>
            <ul class="space-y-0.5">
              <li v-for="f in g.files" :key="f.relPath" class="group/media flex items-center gap-1">
                <img
                  v-if="g.category === 'image'"
                  :src="mediaUrl(f.relPath)"
                  alt=""
                  class="h-6 w-6 shrink-0 rounded object-cover bg-ash/40"
                />
                <button
                  class="min-w-0 flex-1 text-left px-1.5 py-1 rounded text-xs font-mono truncate text-parchment/70 hover:bg-ash/40 hover:text-parchment transition-colors"
                  :title="t('editor.assetCopyTitle')"
                  @click="copyAssetId(f.relPath)"
                >
                  {{ f.relPath.split("/").pop() }}
                </button>
                <!-- クロップ (画像のみ)。同じ ID へ上書きするので YAML は書き直さなくてよい。 -->
                <button
                  v-if="g.category === 'image'"
                  class="shrink-0 px-1 py-1 text-[10px] text-parchment/25 opacity-0 group-hover/media:opacity-100 hover:text-ember transition-opacity"
                  :title="t('editor.cropTitle')"
                  @click="cropping = { src: mediaUrl(f.relPath), relPath: f.relPath }"
                >
                  ⧉
                </button>
                <button
                  class="shrink-0 px-1.5 py-1 text-xs text-parchment/25 opacity-0 group-hover/media:opacity-100 hover:text-red-400 transition-opacity"
                  :title="t('editor.deleteTitle', { file: f.relPath })"
                  @click="game.deleteEditorFile(f.relPath)"
                >
                  ✕
                </button>
              </li>
            </ul>
          </div>
          <p v-if="!mediaGroups.length" class="text-parchment/30 text-xs">{{ t("editor.mediaEmpty") }}</p>
          <p class="mt-auto pt-3 text-parchment/30 text-[10px] leading-relaxed">{{ t("editor.mediaNote") }}</p>
        </template>

        <template v-else>
        <div v-for="g in editorGroups" :key="g.category" class="mb-3">
          <div class="text-parchment/40 mb-1.5 flex items-center gap-1.5 text-xs">
            <Icon name="folder" :size="12" />{{ t(`editor.cat_${g.category}`) }}
          </div>
          <ul class="space-y-0.5">
            <li v-for="f in g.files" :key="f.relPath" class="group/file flex items-center">
              <button
                class="min-w-0 flex-1 text-left px-2 py-1 rounded text-xs font-mono truncate transition-colors"
                :class="
                  game.editor.current === f.relPath
                    ? 'bg-ember/15 text-glow'
                    : 'text-parchment/70 hover:bg-ash/40 hover:text-parchment'
                "
                @click="game.openEditorFile(f.relPath)"
              >
                {{ f.relPath.split("/").pop()
                }}<span
                  v-if="game.editor.current === f.relPath && game.editorDirty"
                  class="text-ember ml-1"
                  >●</span
                >
              </button>
              <!-- 削除 (2026-08-27 に v1 昇格)。package.yaml は土台なので出さない
                   (backend も拒否する = 二層)。hover で現れる小さな ×。 -->
              <button
                v-if="f.relPath !== 'package.yaml'"
                class="shrink-0 px-1.5 py-1 text-xs text-parchment/25 opacity-0 group-hover/file:opacity-100 hover:text-red-400 transition-opacity"
                :title="t('editor.deleteTitle', { file: f.relPath })"
                @click="game.deleteEditorFile(f.relPath)"
              >
                ✕
              </button>
            </li>
            <!-- + 新規 (カテゴリ内・2026-08-27 ユーザーFB)。campaign は固定名なので即作成。 -->
            <li v-if="g.creatable">
              <button
                v-if="newFileCat !== g.category"
                class="w-full text-left px-2 py-1 rounded text-xs text-parchment/40 hover:bg-ash/40 hover:text-parchment transition-colors"
                @click="openNewFile(g.category)"
              >
                <Icon name="plus" :size="11" /> {{ t(`editor.new_${g.category}`) }}
              </button>
              <div v-else class="flex items-center gap-1">
                <input
                  v-model="newFileStem"
                  class="min-w-0 flex-1 rounded border border-ash bg-ash/30 px-2 py-1 text-xs font-mono"
                  :placeholder="t('editor.newFilePlaceholder')"
                  :title="t(`editor.newTitle_${g.category}`)"
                  @keydown.enter.prevent="submitNewFile"
                  @keydown.esc="newFileCat = null"
                />
                <button
                  class="shrink-0 px-2 py-1 rounded border border-ash text-xs text-parchment/70 hover:border-ember/60 hover:text-parchment disabled:opacity-40"
                  :disabled="!newFileStem.trim()"
                  @click="submitNewFile"
                >
                  {{ t("editor.newFileCreate") }}
                </button>
              </div>
            </li>
          </ul>
        </div>
        <!-- 層 2 診断 (spec 28 Phase B): パッケージ全体の inspect。ファイル横断の破れは
             ここにしか出ない。file が引けた行はクリックでそのファイルへ。 -->
        <div v-if="game.editor.issues.length" class="mt-3 border-t border-ash pt-2">
          <div class="text-parchment/40 mb-1.5 text-xs">
            {{ t("editor.issuesTitle", { n: String(game.editor.issues.length) }) }}
          </div>
          <ul class="space-y-1">
            <li v-for="(iss, i) in game.editor.issues" :key="i">
              <button
                class="w-full text-left px-2 py-1 rounded text-[11px] leading-snug transition-colors"
                :class="iss.file ? 'hover:bg-ash/40' : 'cursor-default'"
                @click="iss.file && game.openEditorFile(iss.file)"
              >
                <span
                  class="font-mono"
                  :class="iss.severity === 'error' ? 'text-red-400/90' : 'text-ember/90'"
                >
                  {{ iss.severity === "error" ? "✗" : "⚠" }}
                  {{ iss.file ?? t("editor.issueWholePkg") }}
                </span>
                <span class="block text-parchment/70 whitespace-pre-wrap break-words">{{ iss.message }}</span>
              </button>
            </li>
          </ul>
        </div>
        <!-- 境界の常設 (spec 28 C.3): 緑は「読める」であって「遊べる」ではない。 -->
        <p class="mt-auto pt-3 text-parchment/30 text-[10px] leading-relaxed">
          {{ t("editor.reflectNote") }}
        </p>
        </template>
      </template>
      <template v-else-if="game.state">
        <!-- 1枚め「進行」: ターン / 目標 / この場にいる -->
        <template v-if="activeTab === 'progress'">
          <div class="mb-3 flex items-center">
            <span class="text-parchment/40 flex items-center gap-1.5"><Icon name="turn" />{{ t("state.turn") }}</span>
            <span class="ml-2 text-parchment">{{ game.state.turn }}</span>
          </div>

          <!-- 目標 (named goal) の一覧: 「何を目指せる盤面か」をプレイヤーに示す。 -->
          <!-- when/narration はネタバレゆえ出さず、hint (作者が意図的に開示する道しるべ) を添える。 -->
          <!-- 増えたら領域内で独立スクロール。バーは常時表示 (overflow-y-scroll) で
               ガター幅を確保し、出現/消滅による横のカクつきを防ぐ。 -->
          <div v-if="game.state.goals.length" class="mb-3 flex-1 min-h-0 flex flex-col">
            <div class="text-parchment/40 mb-2 flex items-center gap-1.5"><Icon name="target" />{{ t("state.goals") }}</div>
            <ul class="goal-list space-y-1.5 flex-1 min-h-0 overflow-y-scroll pr-1">
              <li
                v-for="g in game.state.goals"
                :key="g.id"
                class="rounded border px-2 py-1 text-xs"
                :class="
                  g.id === game.state.reached_goal
                    ? 'border-ember/60 bg-ember/15 text-glow'
                    : 'border-ash/60 bg-ash/20 text-parchment/70'
                "
              >
                <div class="flex items-center gap-2">
                  <span
                    class="w-1.5 h-1.5 rounded-full shrink-0"
                    :class="g.id === game.state.reached_goal ? 'bg-glow' : 'bg-parchment/30'"
                  ></span>
                  <span class="truncate">{{ g.title || g.id }}</span>
                  <span v-if="g.id === game.state.reached_goal" class="ml-auto shrink-0">{{ t("state.reached") }}</span>
                </div>
                <p v-if="g.hint" class="mt-0.5 pl-3.5 text-[11px] leading-snug text-parchment/50">
                  {{ g.hint }}
                </p>
              </li>
            </ul>
          </div>

          <div
            v-if="game.state.goal_reached"
            class="rounded bg-ember/20 border border-ember/50 px-3 py-2 text-center text-glow"
          >
            {{ t("state.goalReached") }}
          </div>

          <!-- この場にいる人物 (主人公 + NPC) の顔アイコン行。クリックでプロフィール。 -->
          <!-- 居ない人物のパラメータは出さない (presence のみ可視)。 -->
          <div v-if="game.presentCharacters.length" class="mt-auto pt-4 border-t border-ash/60">
            <div class="text-parchment/40 mb-2">{{ t("state.present") }}</div>
            <div class="flex flex-wrap gap-3">
              <button
                v-for="c in game.presentCharacters"
                :key="c.id"
                class="flex flex-col items-center gap-1 group focus:outline-none"
                :title="assignmentOf(c.id) ? `${c.name} — ${t('state.playedBy', { name: assignmentOf(c.id)!.displayName })}` : c.name"
                :draggable="!!c.iconId"
                @click="selectedId = c.id"
                @dragstart="onIconDragStart(c, $event)"
              >
                <!-- アイコンは CSS background で描画 (asset protocol の MIME に寛容)。無ければ initials。 -->
                <!-- 多人数: 操作プレイヤーの席色リング (青=1人目/赤=2人目/黄=3人目…) で誰の手駒かを可視化。 -->
                <span
                  class="w-12 h-12 rounded-full overflow-hidden border border-ash bg-ash/40 bg-cover bg-center flex items-center justify-center text-parchment/70 group-hover:border-ember transition-colors"
                  :style="[c.icon ? { backgroundImage: `url(${c.icon})` } : {}, seatRing(c.id)]"
                >
                  <span v-if="!c.icon" class="text-xs">{{ initials(c.name) }}</span>
                </span>
                <span class="text-[10px] text-parchment/60 max-w-[3.5rem] truncate">{{ c.name }}</span>
              </button>
            </div>
          </div>
        </template>

        <!-- 2枚め「状態」: 現在地 / 所持品 / フラグ -->
        <template v-else-if="activeTab === 'world'">
          <div class="mb-3">
            <div class="text-parchment/40 flex items-center gap-1.5"><Icon name="location" />{{ t("state.location") }}</div>
            <!-- 表示は authored title を優先、無ければ id (機械用セレクタ) へフォールバック。hover で id。 -->
            <div class="text-parchment" :title="game.state.location">
              {{ game.state.location_title || game.state.location }}
            </div>
          </div>

          <div class="mb-3">
            <div class="text-parchment/40 flex items-center gap-1.5"><Icon name="bag" />{{ t("state.inventory") }}</div>
            <div v-if="game.state.inventory.length" class="text-parchment">
              {{ game.state.inventory.join(t("state.listSep")) }}
            </div>
            <div v-else class="text-parchment/30">{{ t("state.none") }}</div>
          </div>

          <div class="mb-3">
            <div class="text-parchment/40 flex items-center gap-1.5"><Icon name="flag" />{{ t("state.flags") }}</div>
            <!-- 表示名 (title || key) のチップ。hover で「いつ・何をして立ったか」(chronicle join) を出す。 -->
            <div v-if="game.state.flags.length" class="flex flex-wrap gap-1.5 mt-1">
              <span
                v-for="f in game.state.flags"
                :key="f.key"
                class="px-2 py-0.5 rounded bg-ash/40 border border-ash text-xs text-parchment/80"
                :title="f.cause ? `T${f.turn}: ${f.cause}` : f.turn ? t('state.flagSetAt', { turn: f.turn }) : ''"
              >
                {{ f.title || f.key }}
              </span>
            </div>
            <div v-else class="text-parchment/30">{{ t("state.none") }}</div>
          </div>

          <!-- シードリセット (プレイヤーの meta 操作)。セーブ地点からやり直しても出目が
               同じ = 決定論の裏返しへの逃げ道。誤爆すると「この先の運命」が黙って変わる
               のに見た目は何も動かないので、確認ダイアログを挟む。 -->
          <div class="mt-4 pt-3 border-t border-ash/40 flex justify-end">
            <button
              type="button"
              class="px-2 py-1 rounded text-[11px] text-parchment/40 hover:text-ember hover:bg-ash/40 transition-colors"
              :title="t('state.resetSeedHint')"
              @click="game.resetSeed()"
            >
              {{ t("state.resetSeed") }}
            </button>
          </div>
        </template>

        <!-- 3枚め「マップ」(spec 15): 訪問済み+1歩先の有向グラフ。 -->
        <template v-else-if="activeTab === 'map'">
          <MapPanel />
        </template>

        <!-- 5枚め「既成事実」(spec 20): 既成事実 (GM とユーザーの覚え書き)。 -->
        <template v-else-if="activeTab === 'facts'">
          <FactsPanel />
        </template>

        <!-- 4枚め「あらすじ」(spec 10): 圧縮済み章 (append-only) + 最近の出来事 (未圧縮 chronicle)。
             GM が prompt で見ている長期記憶と同じもの = 要約ドリフトの観測装置でもある。 -->
        <template v-else>
          <!-- 圧縮済みの章 (古い順)。key は upto_turn (title は表示専用で衝突し得る)。 -->
          <section v-if="game.synopsis.length" class="space-y-3 mb-4">
            <article
              v-for="s in game.synopsis"
              :key="s.upto_turn"
              class="rounded border border-ash/60 bg-ash/15 px-3 py-2"
            >
              <h4 class="flex items-baseline gap-2 mb-1">
                <span class="text-glow text-xs font-bold truncate">{{ s.title }}</span>
                <span class="ml-auto shrink-0 text-[10px] text-parchment/35">{{ t("state.uptoTurn", { turn: s.upto_turn }) }}</span>
              </h4>
              <p class="text-[12px] leading-relaxed text-parchment/75 whitespace-pre-line">{{ s.text }}</p>
            </article>
          </section>

          <!-- 最近の出来事 = 未圧縮 chronicle のターン別 1 行 (章が確定すると呑まれて消える)。 -->
          <section>
            <div class="text-parchment/40 mb-2 flex items-center gap-1.5">
              <Icon name="turn" />{{ t("state.recentEvents") }}
            </div>
            <ul v-if="game.recentLog.length" class="space-y-1">
              <li
                v-for="l in game.recentLog"
                :key="l.turn"
                class="text-[12px] leading-snug text-parchment/70"
              >
                <span class="text-parchment/35 tabular-nums mr-1.5">T{{ l.turn }}</span>{{ l.summary }}
              </li>
            </ul>
            <p v-else-if="!game.synopsis.length" class="text-parchment/30 text-xs">
              {{ t("state.synopsisEmpty") }}
            </p>
            <p v-else class="text-parchment/30 text-xs">{{ t("state.synopsisInChapter") }}</p>
          </section>
        </template>
      </template>

      <p v-else class="text-parchment/30">{{ t("state.notStarted") }}</p>
    </div>

    <!-- 顔アイコンクリックで開くプロフィールカード -->
    <Transition name="profile">
      <div
        v-if="selectedEntity"
        class="fixed inset-0 z-40 flex items-center justify-center bg-black/60 backdrop-blur-[2px]"
        @click.self="selectedId = null"
      >
        <div
          class="profile-card w-[30rem] max-w-[92vw] max-h-[80vh] overflow-y-auto rounded-xl border border-ash bg-gradient-to-b from-ash/50 via-ink to-ink shadow-2xl"
        >
          <!-- ヘッダ: 顔アイコン (クリックで profile 本文を開閉) + 名前 + 属性チップ -->
          <header class="relative flex items-center gap-3 p-4 pb-3 border-b border-ash/60">
            <button
              class="relative w-16 h-16 rounded-full shrink-0 focus:outline-none transition-shadow"
              :class="
                selectedEntity.profile
                  ? 'cursor-pointer ring-2 ring-ember/50 ring-offset-2 ring-offset-ink hover:ring-glow'
                  : 'cursor-default ring-2 ring-ash ring-offset-2 ring-offset-ink'
              "
              :title="selectedEntity.profile ? t('state.viewProfile') : ''"
              :aria-expanded="showProfile"
              @click="selectedEntity.profile && (showProfile = !showProfile)"
            >
              <span
                class="w-full h-full rounded-full bg-ash/40 bg-cover bg-center flex items-center justify-center text-parchment/70"
                :style="selectedIcon ? { backgroundImage: `url(${selectedIcon})` } : {}"
              >
                <span v-if="!selectedIcon" class="text-lg">{{ initials(selectedName) }}</span>
              </span>
              <!-- profile がある印: 右下の小さなバッジ -->
              <span
                v-if="selectedEntity.profile"
                class="absolute -bottom-1 -right-1 w-5 h-5 rounded-full bg-ink border border-ember/60 flex items-center justify-center text-ember text-[10px] leading-none"
                aria-hidden="true"
              >
                {{ showProfile ? "−" : "…" }}
              </span>
            </button>
            <div class="min-w-0">
              <h3 class="text-glow font-bold text-lg leading-tight truncate">{{ selectedName }}</h3>
              <div v-if="selectedEntity.attributes.length || assignmentOf(selectedEntity.id)" class="mt-1.5 flex flex-wrap gap-1">
                <!-- 多人数: 操作プレイヤーの chip (席色ドット + 名前)。属性と同列 = 「この場の事実」として見せる。 -->
                <span
                  v-if="assignmentOf(selectedEntity.id)"
                  class="px-2 py-0.5 rounded-full border text-[11px] leading-4 flex items-center gap-1"
                  :style="{ borderColor: assignmentOf(selectedEntity.id)!.color, backgroundColor: `${assignmentOf(selectedEntity.id)!.color}22` }"
                >
                  <span class="inline-block w-2 h-2 rounded-full" :style="{ backgroundColor: assignmentOf(selectedEntity.id)!.color }" />
                  <span class="text-glow">{{ t("state.playedBy", { name: assignmentOf(selectedEntity.id)!.displayName }) }}</span>
                </span>
                <span
                  v-for="a in selectedEntity.attributes"
                  :key="a.key"
                  class="px-2 py-0.5 rounded-full bg-ember/15 border border-ember/40 text-[11px] leading-4"
                  :title="a.key"
                >
                  <span class="text-parchment/50">{{ a.key }}</span>
                  <span class="text-glow ml-1">{{ a.value }}</span>
                </span>
              </div>
            </div>
            <button
              class="absolute top-2 right-2 w-7 h-7 rounded-full flex items-center justify-center text-parchment/50 hover:text-parchment hover:bg-ash/60 transition-colors"
              :aria-label="t('state.close')"
              @click="selectedId = null"
            >
              ✕
            </button>
          </header>

          <!-- プロフィール本文 (authored の語り素材)。初期は畳み、顔アイコンクリックで開く。 -->
          <Transition name="reveal">
            <p
              v-if="showProfile && selectedEntity.profile"
              class="mx-4 mt-3 pl-3 border-l-2 border-ember/40 text-[13px] leading-relaxed text-parchment/75 whitespace-pre-line"
            >
              {{ selectedEntity.profile }}
            </p>
          </Transition>

          <div class="p-4 pt-3 space-y-4">
            <!-- ステータス: 3列グリッド -->
            <section v-if="selectedEntity.stats.length">
              <h4 class="flex items-center gap-1.5 text-parchment/40 text-xs tracking-wider mb-2">
                <Icon name="gauge" />{{ t("state.stats") }}
              </h4>
              <div class="grid grid-cols-3 gap-x-5 gap-y-1.5">
                <div
                  v-for="s in selectedEntity.stats"
                  :key="s.key"
                  class="flex items-baseline justify-between border-b border-ash/40 pb-0.5"
                >
                  <span class="text-parchment/60 text-xs truncate mr-2">{{ s.key }}</span>
                  <span class="text-glow font-semibold tabular-nums">{{ s.value }}</span>
                </div>
              </div>
            </section>

            <!-- 能力: チップ -->
            <section v-if="selectedEntity.skills.length">
              <h4 class="flex items-center gap-1.5 text-parchment/40 text-xs tracking-wider mb-2">
                <Icon name="sparkle" />{{ t("state.skills") }}
              </h4>
              <div class="flex flex-wrap gap-1.5">
                <span
                  v-for="sk in selectedEntity.skills"
                  :key="sk"
                  class="px-2 py-0.5 rounded bg-glow/10 border border-glow/30 text-xs text-glow"
                >
                  {{ sk }}
                </span>
              </div>
            </section>

            <!-- 所持: チップ -->
            <section v-if="selectedEntity.items.length">
              <h4 class="flex items-center gap-1.5 text-parchment/40 text-xs tracking-wider mb-2">
                <Icon name="bag" />{{ t("state.items") }}
              </h4>
              <div class="flex flex-wrap gap-1.5">
                <span
                  v-for="it in selectedEntity.items"
                  :key="it"
                  class="px-2 py-0.5 rounded bg-ash/40 border border-ash text-xs text-parchment/80"
                >
                  {{ it }}
                </span>
              </div>
            </section>

            <p v-if="selectedIsEmpty" class="text-parchment/40 text-sm">
              {{ t("state.entityEmpty") }}
            </p>
          </div>
        </div>
      </div>
    </Transition>

    <!-- クロップ (spec 28 追補)。動的 import — 触らないセッションで読み込まない。 -->
    <CropDialog v-if="cropping" :src="cropping.src" :rel-path="cropping.relPath" @close="cropping = null" />
  </aside>
</template>

<style scoped>
/* プロフィールカードの入退場: 幕はフェード、カードは軽く浮き上がる */
.profile-enter-active,
.profile-leave-active {
  transition: opacity 0.18s ease;
}
.profile-enter-from,
.profile-leave-to {
  opacity: 0;
}
.profile-enter-active .profile-card,
.profile-leave-active .profile-card {
  transition: transform 0.18s ease;
}
.profile-enter-from .profile-card,
.profile-leave-to .profile-card {
  transform: scale(0.96) translateY(8px);
}

/* 目標一覧の常時表示スクロールバー: 細身・ash でテーマに馴染ませる */
.goal-list::-webkit-scrollbar {
  width: 6px;
}
.goal-list::-webkit-scrollbar-track {
  background: transparent;
}
.goal-list::-webkit-scrollbar-thumb {
  background: rgba(58, 50, 43, 0.9); /* ash */
  border-radius: 3px;
}
.goal-list::-webkit-scrollbar-thumb:hover {
  background: rgba(217, 138, 74, 0.5); /* ember */
}

/* profile 本文の開閉: ふわっと開く */
.reveal-enter-active,
.reveal-leave-active {
  transition:
    opacity 0.16s ease,
    transform 0.16s ease;
}
.reveal-enter-from,
.reveal-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>
