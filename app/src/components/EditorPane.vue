<script setup lang="ts">
/**
 * 編集モードの主領域 (spec 28 Phase A)。会話ペイン + 入力欄の代わりに <main> へ出る。
 * ファイルの選択は右ペインの「ファイル」タブ (StatePanel) — ここは開いている 1 枚の
 * エディタと保存操作だけを持つ。
 */
import { invoke } from "@tauri-apps/api/core";
import { onBeforeUnmount, onMounted } from "vue";

import { t } from "../i18n";
import { useGameStore } from "../stores/game";
import CodeEditor, { type EditorLintIssue } from "./CodeEditor.vue";

const game = useGameStore();

// 層 1 診断 (spec 28 Phase B): 開いているファイルの kind で backend の既存 lint を呼ぶ。
// 失敗は空 (診断が出ないだけ — 保存やエディタ本体を止めない)。
const KIND: Record<string, string> = {
  package: "manifest",
  campaign: "campaign",
  scenario: "scenario",
  character: "character",
  memoria: "memoria",
};
async function lintProvider(text: string): Promise<EditorLintIssue[]> {
  const f = game.editor.files.find((x) => x.relPath === game.editor.current);
  if (!f) return [];
  try {
    return await invoke<EditorLintIssue[]>("lint_editor_text", {
      kind: KIND[f.category] ?? "scenario",
      text,
    });
  } catch {
    return [];
  }
}

// Ctrl+S = 保存。ブラウザ既定 (ページ保存ダイアログ) は常に無効化し、
// ファイルを開いている時だけ保存を飛ばす。
function onKeydown(e: KeyboardEvent) {
  if (!(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== "s") return;
  e.preventDefault();
  void game.saveEditorFile();
}
onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <div class="flex-1 flex flex-col min-h-0 bg-ink">
    <!-- ヘッダ行: 開いているファイル + ● (未保存) + 保存。 -->
    <div class="flex items-center gap-2 px-3 py-1.5 border-b border-ash text-xs">
      <span v-if="game.editor.current" class="font-mono text-parchment/80 truncate min-w-0">
        {{ game.editor.current }}<span v-if="game.editorDirty" class="text-ember ml-1" :title="t('editor.dirty')">●</span>
      </span>
      <span v-else class="text-parchment/40">{{ t("editor.noFile") }}</span>
      <span class="flex-1"></span>
      <!-- 書庫由来バッジ (spec 28 A.4): フォーク確認が唐突にならない事前認知。 -->
      <span
        v-if="game.editor.fromSite"
        class="px-1.5 py-0.5 rounded border border-ember/40 text-ember/80"
        :title="t('editor.fromSiteTitle')"
      >
        {{ t("editor.fromSiteBadge") }}
      </span>
      <button
        class="px-2 py-0.5 rounded border border-ash text-parchment/70 hover:text-parchment hover:border-ember/60 disabled:opacity-40"
        :disabled="!game.editor.current || !game.editorDirty || game.editor.saving"
        :title="t('editor.saveTitle')"
        @click="game.saveEditorFile()"
      >
        {{ game.editor.saving ? t("editor.saving") : t("editor.save") }}
      </button>
    </div>

    <!-- 本文。ファイル未選択なら案内 (選ぶ場所は右ペインのファイルタブ)。 -->
    <div v-if="game.editor.current" class="flex-1 min-h-0 p-2">
      <CodeEditor v-model="game.editor.text" language="yaml" height="100%" :lint-provider="lintProvider" />
    </div>
    <div v-else class="flex-1 flex items-center justify-center text-parchment/40 px-6 text-center text-sm">
      {{ t("editor.pickHint") }}
    </div>
  </div>
</template>
