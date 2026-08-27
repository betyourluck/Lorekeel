<script setup lang="ts">
/**
 * 編集モードの主領域 (spec 28 Phase A)。会話ペイン + 入力欄の代わりに <main> へ出る。
 * ファイルの選択は右ペインの「ファイル」タブ (StatePanel) — ここは開いている 1 枚の
 * エディタと保存操作だけを持つ。
 */
import { invoke } from "@tauri-apps/api/core";
import { onBeforeUnmount, onMounted } from "vue";

import { makeCompletionSource } from "../editorCompletion";
import { t } from "../i18n";
import { EDITOR_FONT_SIZES, useGameStore } from "../stores/game";
import CodeEditor, { type EditorLintIssue } from "./CodeEditor.vue";
import Icon from "./Icon.vue";

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
function currentKind(): string {
  const f = game.editor.files.find((x) => x.relPath === game.editor.current);
  return f ? (KIND[f.category] ?? "scenario") : "scenario";
}
async function lintProvider(text: string): Promise<EditorLintIssue[]> {
  if (!game.editor.current) return [];
  try {
    return await invoke<EditorLintIssue[]>("lint_editor_text", { kind: currentKind(), text });
  } catch {
    return [];
  }
}
// 補完 (spec 28 Phase C)。vocab と kind は getter で渡す — 保存後の語彙更新・
// ファイル切替にソースを作り直さず追従する。
const completionSource = makeCompletionSource(() => game.editor.vocab, currentKind);

/** 開いているファイルの改名 (F2 / ヘッダのダブルクリック)。VS Code と同じ契機。
 *  一覧の行内編集と違いここはプロンプトを持たないので、簡易に window.prompt は使わず
 *  ファイル一覧側へ委ねる — 名前を打つ場所は一箇所に保つ (二つあると流儀が割れる)。 */
function renameCurrent() {
  if (!game.editor.current) return;
  game.editorRenameRequest = game.editor.current; // StatePanel が拾って行内編集を開く
}

// Ctrl+S = 保存 / F2 = 改名。ブラウザ既定 (ページ保存ダイアログ) は常に無効化し、
// ファイルを開いている時だけ保存を飛ばす。
function onKeydown(e: KeyboardEvent) {
  if (e.key === "F2") {
    e.preventDefault();
    renameCurrent();
    return;
  }
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
      <!-- 開いているファイル名。ダブルクリックで改名 (ファイル一覧の行と同じ流儀)。 -->
      <span
        v-if="game.editor.current"
        class="font-mono text-parchment/80 truncate min-w-0 cursor-text"
        :title="t('editor.headerTitle')"
        @dblclick="renameCurrent"
      >
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
      <!-- 文字サイズ (3 段: 小/中/大)。既定は中 — 従来の 13px は「小」に相当する。 -->
      <label class="flex items-center gap-1.5 text-parchment/40" :title="t('editor.fontSizeTitle')">
        <span class="text-[10px]">A</span>
        <input
          type="range"
          min="0"
          max="2"
          step="1"
          class="w-16 accent-ember"
          :value="game.editorFontStep"
          @input="game.setEditorFontStep(Number(($event.target as HTMLInputElement).value))"
        />
        <span class="text-sm leading-none">A</span>
      </label>
      <!-- 保存: フロッピー (ユーザーFB 2026-08-28 — 文字ボタンからアイコンへ)。
           処理中は spinner に差し替えて、押せない理由を形で見せる。 -->
      <button
        class="grid h-6 w-6 place-items-center rounded text-parchment/60 hover:bg-ash/60 hover:text-parchment disabled:opacity-30"
        :disabled="!game.editor.current || !game.editorDirty || game.editor.saving"
        :title="t('editor.saveTitle')"
        :aria-label="t('editor.save')"
        @click="game.saveEditorFile()"
      >
        <Icon :name="game.editor.saving ? 'spinner' : 'floppy'" :size="15" />
      </button>
    </div>

    <!-- 本文。ファイル未選択なら案内 (選ぶ場所は右ペインのファイルタブ)。 -->
    <!-- CodeEditor のルート div は高さを持たない (ダイアログ用途では固定 height の
         .cm-editor を包むだけ)。ここでは height="100%" の連鎖を通すために class の
         fallthrough で h-full を与える — 無いと本体が中身の高さまで伸び、flex 親に
         切られて**下がスクロールできない** (実機で発覚)。 -->
    <div v-if="game.editor.current" class="flex-1 min-h-0 p-2">
      <CodeEditor
        v-model="game.editor.text"
        language="yaml"
        height="100%"
        class="h-full"
        :lint-provider="lintProvider"
        :completion-source="completionSource"
        :font-size="EDITOR_FONT_SIZES[game.editorFontStep]"
      />
    </div>
    <div v-else class="flex-1 flex items-center justify-center text-parchment/40 px-6 text-center text-sm">
      {{ t("editor.pickHint") }}
    </div>
  </div>
</template>
