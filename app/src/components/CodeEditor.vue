<script setup lang="ts">
/**
 * CodeMirror 6 の薄い包み (spec 27 Phase C)。**汎用コンポーネント**として作る —
 * いま使うのは ComfyUI のワークフロー JSON 欄とプロンプト欄だが、将来のパッケージ YAML
 * エディタも同じ器に乗せる想定 (`@codemirror/lang-yaml` はその時に足す。依存は後から積める)。
 *
 * 配色は **CSS 変数 (main.css) を引く**ので、テーマ切替に自動追従する。ただし `dark` フラグだけは
 * 追従しない — CodeMirror はこの真偽値で `&dark` / `&light` の**別系統の既定**(検索一致の強調・
 * 補完候補の選択色・プレースホルダ・特殊文字) を選ぶ。`var()` で書けない値がライブラリの中に
 * あるので、フラグを差し替える経路 (Compartment) を持つ。
 */
import {
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
  type CompletionSource,
} from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { json, jsonParseLinter } from "@codemirror/lang-json";
import { yaml } from "@codemirror/lang-yaml";
import { defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { linter, lintGutter, type Diagnostic } from "@codemirror/lint";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { Compartment, EditorState } from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  placeholder,
} from "@codemirror/view";
import { onBeforeUnmount, onMounted, ref, watch } from "vue";

import { theme } from "../theme";

/** `text` = 行番号も構文も持たない素の複数行 (プロンプト)。`json` = 構文色 + パースの lint。
 *  `yaml` = 構文色のみ (spec 28 — 診断は Phase B で backend の既存 lint を linter() に繋ぐ)。 */
type EditorLanguage = "text" | "json" | "yaml";

/** 外部診断 1 件 (spec 28 Phase B)。line は 1 始まり、null = 位置なし (先頭に出す)。 */
export interface EditorLintIssue {
  line: number | null;
  severity: string;
  message: string;
}

const props = withDefaults(
  defineProps<{
    modelValue: string;
    language?: EditorLanguage;
    placeholder?: string;
    readonly?: boolean;
    /** エディタの高さ (CSS 値)。 */
    height?: string;
    /** 外部診断の供給源 (spec 28 Phase B)。与えると linter (デバウンス 500ms) + ガターが付く。
     *  検査そのものは backend の既存 lint — ここは位置を範囲へ写すだけ。 */
    lintProvider?: (text: string) => Promise<EditorLintIssue[]>;
    /** 補完の供給源 (spec 28 Phase C)。与えると既定の補完を差し替える (override)。 */
    completionSource?: CompletionSource;
    /** 本文の文字サイズ (px)。既定 13 = 従来のダイアログ用途の値 (無指定で挙動不変)。 */
    fontSize?: number;
  }>(),
  {
    language: "text",
    placeholder: "",
    readonly: false,
    height: "16rem",
    lintProvider: undefined,
    completionSource: undefined,
    fontSize: 13,
  },
);

const emit = defineEmits<{ (e: "update:modelValue", value: string): void }>();

const host = ref<HTMLElement | null>(null);
const languageCompartment = new Compartment();
const readOnlyCompartment = new Compartment();
const placeholderCompartment = new Compartment();
const themeCompartment = new Compartment();
let editor: EditorView | null = null;

/** `text` では構文も lint も付けない (プロンプトに文法は無い — 赤線は誤警告にしかならない)。 */
function languageExtension(language: EditorLanguage) {
  if (language === "json") return [json(), linter(jsonParseLinter())];
  if (language === "yaml") return [yaml()];
  return [];
}

function readOnlyExtension(readonly: boolean) {
  return [EditorState.readOnly.of(readonly), EditorView.editable.of(!readonly)];
}

function editorTheme(dark: boolean, height: string, fontSize: number) {
  return EditorView.theme(
    {
      "&": {
        height,
        backgroundColor: "rgb(var(--ash) / 0.4)",
        color: "rgb(var(--parchment))",
        fontSize: `${fontSize}px`,
        borderRadius: "0.25rem",
      },
      "&.cm-focused": { outline: "1px solid rgb(var(--ember) / 0.6)" },
      ".cm-scroller": {
        fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
        lineHeight: "1.6",
        overflow: "auto",
      },
      ".cm-content": { padding: "8px 0", caretColor: "rgb(var(--ember))" },
      ".cm-line": { padding: "0 10px" },
      ".cm-gutters": {
        backgroundColor: "rgb(var(--ash) / 0.6)",
        color: "rgb(var(--parchment) / 0.4)",
        border: "none",
      },
      ".cm-activeLine": { backgroundColor: "rgb(var(--ember) / 0.08)" },
      ".cm-activeLineGutter": { backgroundColor: "rgb(var(--ember) / 0.12)" },
      ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
        backgroundColor: "rgb(var(--ember) / 0.35)",
      },
      ".cm-cursor, .cm-dropCursor": { borderLeftColor: "rgb(var(--ember))" },
      ".cm-placeholder": { color: "rgb(var(--parchment) / 0.35)" },
      ".cm-tooltip": {
        border: "1px solid rgb(var(--ash))",
        backgroundColor: "rgb(var(--ink))",
        color: "rgb(var(--parchment))",
      },
      ".cm-panels": { backgroundColor: "rgb(var(--ink))", color: "rgb(var(--parchment))" },
      ".cm-textfield": { backgroundColor: "rgb(var(--ash) / 0.5)", color: "rgb(var(--parchment))" },
    },
    { dark },
  );
}

/** 外部診断 → CodeMirror Diagnostic。行が document の範囲内ならその行全体、外/なしは先頭。 */
function externalLinter(provider: (text: string) => Promise<EditorLintIssue[]>) {
  return linter(
    async (view) => {
      const issues = await provider(view.state.doc.toString());
      const doc = view.state.doc;
      return issues.map((i): Diagnostic => {
        const severity = i.severity === "error" ? "error" : "warning";
        if (i.line && i.line >= 1 && i.line <= doc.lines) {
          const ln = doc.line(i.line);
          return { from: ln.from, to: ln.to, severity, message: i.message };
        }
        return { from: 0, to: 0, severity, message: i.message };
      });
    },
    { delay: 500 },
  );
}

onMounted(() => {
  if (!host.value) return;
  const withLineNumbers = props.language !== "text" ? [lineNumbers(), highlightActiveLineGutter()] : [];
  const withLint = props.lintProvider ? [lintGutter(), externalLinter(props.lintProvider)] : [];
  editor = new EditorView({
    parent: host.value,
    state: EditorState.create({
      doc: props.modelValue,
      extensions: [
        ...withLineNumbers,
        ...withLint,
        highlightSpecialChars(),
        history(),
        drawSelection(),
        EditorState.allowMultipleSelections.of(true),
        EditorView.lineWrapping,
        highlightActiveLine(),
        search({ top: true }),
        highlightSelectionMatches(),
        autocompletion(props.completionSource ? { override: [props.completionSource] } : {}),
        closeBrackets(),
        keymap.of([
          // **リロードを飲む。** WebView の Ctrl+R は画面を作り直すので、編集中の本文が確認を
          // 一つも通さずに消える。エディタにフォーカスがある間だけ塞ぐ。
          { key: "Mod-r", run: () => true },
          indentWithTab,
          ...closeBracketsKeymap,
          ...defaultKeymap,
          ...historyKeymap,
          ...searchKeymap,
          ...completionKeymap,
        ]),
        syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
        languageCompartment.of(languageExtension(props.language)),
        readOnlyCompartment.of(readOnlyExtension(props.readonly)),
        placeholderCompartment.of(placeholder(props.placeholder)),
        themeCompartment.of(editorTheme(theme.value === "dark", props.height, props.fontSize)),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            const value = update.state.doc.toString();
            if (value !== props.modelValue) emit("update:modelValue", value);
          }
        }),
      ],
    }),
  });
});

watch(
  () => props.modelValue,
  (value) => {
    if (!editor || value === editor.state.doc.toString()) return;
    editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: value } });
  },
);
watch(
  () => props.language,
  (l) => editor?.dispatch({ effects: languageCompartment.reconfigure(languageExtension(l)) }),
);
watch(
  () => props.readonly,
  (r) => editor?.dispatch({ effects: readOnlyCompartment.reconfigure(readOnlyExtension(r)) }),
);
watch(
  () => props.placeholder,
  (p) => editor?.dispatch({ effects: placeholderCompartment.reconfigure(placeholder(p)) }),
);
watch([theme, () => props.height, () => props.fontSize], ([t, h, fs]) =>
  editor?.dispatch({
    effects: themeCompartment.reconfigure(editorTheme(t === "dark", h as string, fs as number)),
  }),
);

onBeforeUnmount(() => {
  editor?.destroy();
  editor = null;
});

/** 親から本文へフォーカスを当てる (ダイアログを開いた直後など)。 */
defineExpose({ focus: () => editor?.focus() });
</script>

<template>
  <div ref="host" class="overflow-hidden rounded" />
</template>
