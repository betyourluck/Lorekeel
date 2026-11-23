<script setup lang="ts">
/**
 * AI でパッケージを組む (spec 38 Phase D)。AI 編集パネルの「パッケージ全体」側の中身。
 *
 * 依頼 → 計画 (行ごとに 新規/変更・パス・指示を直す・外す) → 実行 (進行ログ) → 変更セット
 * (ファイルを選ぶと差分・選んで適用 / すべて適用 / 破棄)。**ディスクに書くのは適用を押したとき**
 * だけで、それまで変更は backend の一時フォルダにある。状態は store (`editor.build`) が持ち、
 * ここは表示と操作の受け口だけ — パネルを閉じても進行は止まらない。
 */
import { computed } from "vue";

import { t } from "../i18n";
import { useGameStore } from "../stores/game";
import type { BuiltFileView } from "../types/api";
import Icon from "./Icon.vue";

const game = useGameStore();
const b = computed(() => game.editor.build);
const busy = computed(() => game.editorBuildBusy);

const canPlan = computed(
  () => !busy.value && !game.editor.assist.running && b.value.request.trim().length > 0,
);
const changedFiles = computed(() => (b.value.result?.files ?? []).filter((f) => f.changed));

function onKey(e: KeyboardEvent) {
  // Ctrl+Enter で計画 (Enter は改行 — 依頼は複数行で書ける)。
  if ((e.ctrlKey || e.metaKey) && e.key === "Enter" && canPlan.value) {
    e.preventDefault();
    void game.planBuild();
  }
}

function actionLabel(a: "create" | "edit"): string {
  return a === "create" ? t("build.actionCreate") : t("build.actionEdit");
}

function toggleDiff(path: string) {
  b.value.expanded = b.value.expanded === path ? "" : path;
}

/** 差分の行の色 (追加 / 削除 / 見出し)。 */
function diffLineClass(line: string): string {
  if (line.startsWith("+++") || line.startsWith("---")) return "text-parchment/40";
  if (line.startsWith("@@")) return "text-glow/70";
  if (line.startsWith("+")) return "text-ok";
  if (line.startsWith("-")) return "text-warn";
  return "text-parchment/60";
}

function fileStopped(f: BuiltFileView): string {
  return f.stopped ? t(`editAssist.stopped_${f.stopped}`) : "";
}
</script>

<template>
  <div class="space-y-3 text-sm">
    <p class="text-[11px] leading-relaxed text-parchment/45">{{ t("build.hint") }}</p>

    <!-- 依頼 -->
    <textarea
      v-model="b.request"
      data-testid="build-request"
      class="w-full h-20 rounded-lg bg-ink/70 p-2 text-sm text-parchment outline-none ring-1 ring-parchment/10 focus:ring-ember/50 disabled:opacity-50"
      :placeholder="t('build.placeholder')"
      :disabled="busy"
      @keydown="onKey"
    />
    <div class="flex items-center gap-2">
      <button
        data-testid="build-plan"
        class="flex items-center gap-1.5 rounded-lg bg-ember/80 px-3 py-1.5 text-sm text-ink hover:bg-ember disabled:opacity-40"
        :disabled="!canPlan"
        @click="game.planBuild()"
      >
        <Icon
          :name="b.phase === 'planning' ? 'spinner' : 'sparkle'"
          :size="14"
          :class="{ 'animate-spin': b.phase === 'planning' }"
        />
        {{
          b.phase === "planning"
            ? t("build.planning")
            : b.phase === "idle"
              ? t("build.plan")
              : t("build.replan")
        }}
      </button>
      <span class="flex-1" />
      <span class="text-[10px] text-parchment/40">{{ t("build.shortcut") }}</span>
    </div>

    <p v-if="b.error" class="text-xs text-warn whitespace-pre-wrap">{{ b.error }}</p>

    <!-- 計画 (直している間と実行中だけ。結果が出たらファイルの一覧は変更セットが持つ — 両方を
         並べるとパネルが縦に倍の長さになり、変更セットまでスクロールが要った = dev サーバで実測) -->
    <div
      v-if="b.phase === 'planned' || b.phase === 'running'"
      class="space-y-2 rounded-lg bg-parchment/5 p-3"
      data-testid="build-plan-list"
    >
      <div class="flex items-baseline gap-2 text-xs">
        <span class="font-serif text-glow">{{ t("build.planHeading") }}</span>
        <span class="text-parchment/45">{{ t("build.planMeta", { n: b.plan.length, model: b.model }) }}</span>
      </div>
      <p v-if="b.rejected.length" class="text-[11px] text-glow/70" :title="b.rejected.join('\n')" data-testid="build-rejected">
        {{ t("build.rejected", { n: b.rejected.length }) }}
      </p>
      <p v-if="!b.plan.length" class="text-xs text-parchment/50">{{ t("build.planEmpty") }}</p>
      <div v-for="(item, i) in b.plan" :key="item.path" class="space-y-1" data-testid="build-plan-row">
        <div class="flex items-center gap-2 text-xs">
          <span
            class="rounded px-1.5 py-0.5 text-[10px]"
            :class="item.action === 'create' ? 'bg-ok/15 text-ok' : 'bg-glow/15 text-glow'"
          >
            {{ actionLabel(item.action) }}
          </span>
          <span class="font-mono text-parchment/85 truncate min-w-0">{{ item.path }}</span>
          <span class="flex-1" />
          <button
            v-if="b.phase === 'planned'"
            class="rounded px-1.5 text-parchment/40 hover:text-warn"
            :title="t('build.remove')"
            :aria-label="t('build.remove')"
            data-testid="build-plan-remove"
            @click="game.removePlanItem(i)"
          >
            ✕
          </button>
        </div>
        <textarea
          v-model="item.instruction"
          rows="2"
          class="w-full rounded bg-ink/70 p-1.5 text-xs text-parchment/85 outline-none ring-1 ring-parchment/10 focus:ring-ember/50 disabled:opacity-60"
          :disabled="b.phase !== 'planned'"
          data-testid="build-plan-instruction"
        />
      </div>
      <div class="flex items-center gap-2 pt-1">
        <button
          data-testid="build-run"
          class="flex items-center gap-1.5 rounded-lg bg-ember/80 px-3 py-1.5 text-sm text-ink hover:bg-ember disabled:opacity-40"
          :disabled="b.phase !== 'planned' || !b.plan.length || game.editor.assist.running"
          @click="game.runBuild()"
        >
          <Icon
            :name="b.phase === 'running' ? 'spinner' : 'sparkle'"
            :size="14"
            :class="{ 'animate-spin': b.phase === 'running' }"
          />
          {{ b.phase === "running" ? t("build.running") : t("build.run") }}
        </button>
        <button
          v-if="b.phase === 'running'"
          class="rounded-lg px-3 py-1.5 text-sm text-parchment/70 ring-1 ring-parchment/20 hover:bg-parchment/10"
          data-testid="build-cancel"
          @click="game.cancelBuild()"
        >
          {{ t("build.cancel") }}
        </button>
        <button
          v-if="b.phase === 'planned'"
          class="rounded-lg px-3 py-1.5 text-sm text-parchment/60 hover:text-parchment"
          @click="game.discardBuild()"
        >
          {{ t("build.discardPlan") }}
        </button>
      </div>
    </div>

    <!-- 進行ログ: ▶ がファイルの切り替わり。実行中は最後の行が生きている合図。 -->
    <div
      v-if="b.log.length || b.phase === 'running'"
      class="max-h-40 overflow-y-auto rounded-lg bg-ink/70 p-2 font-mono text-[11px] leading-relaxed text-parchment/70"
    >
      <div
        v-for="(line, i) in b.log"
        :key="i"
        class="whitespace-pre-wrap break-all"
        :class="{ 'text-glow/90 pt-1': line.startsWith('▶') }"
      >
        {{ line }}
      </div>
      <div v-if="b.phase === 'running'" class="animate-pulse text-ember/80">{{ t("editAssist.thinking") }}</div>
    </div>

    <!-- 変更セット -->
    <div v-if="b.result" class="space-y-2 rounded-lg bg-parchment/5 p-3 text-xs" data-testid="build-result">
      <div class="flex items-baseline gap-2">
        <span class="font-serif text-glow">{{ t("build.resultHeading") }}</span>
        <span class="text-parchment/45">
          {{ t("build.resultMeta", { files: b.result.files.length, changed: changedFiles.length }) }}
        </span>
      </div>
      <p v-if="b.result.stopped" class="text-glow/80">{{ t(`build.stopped_${b.result.stopped}`) }}</p>

      <div v-for="f in b.result.files" :key="f.path" class="rounded bg-parchment/5" data-testid="build-file">
        <div class="flex items-center gap-2 px-2 py-1">
          <input
            type="checkbox"
            class="accent-ember"
            :checked="b.selected.includes(f.path)"
            :disabled="!f.changed || b.phase !== 'done'"
            :aria-label="f.path"
            data-testid="build-file-select"
            @change="game.toggleBuildSelected(f.path)"
          />
          <span
            class="rounded px-1.5 py-0.5 text-[10px]"
            :class="f.action === 'create' ? 'bg-ok/15 text-ok' : 'bg-glow/15 text-glow'"
          >
            {{ actionLabel(f.action) }}
          </span>
          <button
            class="font-mono text-parchment/85 truncate min-w-0 text-left hover:text-parchment"
            :title="t('build.diffTitle')"
            data-testid="build-file-path"
            @click="toggleDiff(f.path)"
          >
            {{ f.path }}
          </button>
          <span class="flex-1" />
          <span v-if="!f.changed" class="text-parchment/40">{{ t("build.unchanged") }}</span>
          <span v-if="f.template_only" class="text-warn" :title="t('build.templateOnlyTitle')">{{ t("build.templateOnly") }}</span>
          <span v-if="f.repaired" class="text-glow/70">{{ t("build.repaired") }}</span>
          <span v-if="f.stopped" class="text-glow/80" :title="fileStopped(f)">{{ t("build.fileStopped") }}</span>
        </div>
        <div v-if="b.expanded === f.path" class="space-y-1.5 border-t border-parchment/10 px-2 py-1.5">
          <p class="whitespace-pre-wrap text-parchment/80">{{ f.summary }}</p>
          <ul v-if="f.diagnostics.length" class="space-y-0.5">
            <li v-for="(d, i) in f.diagnostics" :key="i" :class="d.severity === 'error' ? 'text-warn' : 'text-glow/80'">
              [{{ d.severity }}]<template v-if="d.line"> L{{ d.line }}</template> {{ d.message }}
            </li>
          </ul>
          <div
            v-if="f.diff"
            class="max-h-64 overflow-auto rounded bg-ink/85 p-2 font-mono text-[11px] leading-snug"
            data-testid="build-diff"
          >
            <div v-for="(line, i) in f.diff.split('\n')" :key="i" class="whitespace-pre" :class="diffLineClass(line)">{{ line || " " }}</div>
          </div>
        </div>
      </div>

      <p v-for="f in b.result.failed" :key="`failed-${f.path}`" class="text-warn">
        {{ t("build.failedFile", { file: f.path }) }} — {{ f.message }}
      </p>
      <p v-if="b.result.not_run.length" class="text-glow/80">
        {{ t("build.notRun", { files: b.result.not_run.join(", ") }) }}
      </p>
      <div v-if="b.result.package_issues.length" class="space-y-0.5">
        <p class="text-parchment/60">{{ t("build.issuesLeft") }}</p>
        <p
          v-for="(i, k) in b.result.package_issues"
          :key="`issue-${k}`"
          :class="i.severity === 'error' ? 'text-warn' : 'text-glow/80'"
        >
          [{{ i.severity }}] {{ i.file ?? t("editor.issueWholePkg") }} — {{ i.message }}
        </p>
      </div>
      <p v-if="b.result.preexisting_issues" class="text-parchment/45">
        {{ t("build.preexisting", { n: b.result.preexisting_issues }) }}
      </p>
      <p class="text-[10px] text-parchment/40">
        {{
          t("editAssist.stats", {
            model: b.result.model,
            iterations: b.result.iterations,
            prompt: b.result.prompt_tokens,
            completion: b.result.completion_tokens,
            cached: b.result.cache_read,
          })
        }}
      </p>

      <!-- 適用 -->
      <div v-if="b.phase === 'done' || b.phase === 'applying'" class="flex flex-wrap items-center gap-2 pt-1">
        <button
          data-testid="build-apply-selected"
          class="rounded-lg bg-ember/80 px-3 py-1.5 text-sm text-ink hover:bg-ember disabled:opacity-40"
          :disabled="b.phase !== 'done' || !b.selected.length"
          @click="game.applyBuild(false)"
        >
          {{ b.phase === "applying" ? t("build.applying") : t("build.applySelected", { n: b.selected.length }) }}
        </button>
        <button
          data-testid="build-apply-all"
          class="rounded-lg px-3 py-1.5 text-sm text-parchment/80 ring-1 ring-parchment/20 hover:bg-parchment/10 disabled:opacity-40"
          :disabled="b.phase !== 'done' || !changedFiles.length"
          @click="game.applyBuild(true)"
        >
          {{ t("build.applyAll") }}
        </button>
        <span class="flex-1" />
        <button
          data-testid="build-discard"
          class="rounded-lg px-3 py-1.5 text-sm text-parchment/60 hover:text-warn disabled:opacity-40"
          :disabled="b.phase !== 'done'"
          @click="game.discardBuild()"
        >
          {{ t("build.discard") }}
        </button>
      </div>
    </div>

    <!-- 適用の結果 -->
    <div v-if="b.applied" class="space-y-1 rounded-lg bg-parchment/5 p-3 text-xs" data-testid="build-applied">
      <p class="text-ok">{{ t("build.applied", { n: b.applied.written.length }) }}</p>
      <p v-if="b.applied.conflicts.length" class="text-warn">
        {{ t("build.conflicts", { files: b.applied.conflicts.join(", ") }) }}
      </p>
      <p v-for="e in b.applied.errors" :key="`apply-${e.path}`" class="text-warn">
        {{ t("build.failedFile", { file: e.path }) }} — {{ e.message }}
      </p>
      <button
        class="mt-1 rounded-lg px-3 py-1.5 text-sm text-parchment/70 ring-1 ring-parchment/20 hover:bg-parchment/10"
        data-testid="build-again"
        @click="game.discardBuild()"
      >
        {{ t("build.again") }}
      </button>
    </div>
  </div>
</template>
