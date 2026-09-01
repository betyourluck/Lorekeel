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
import { rangeOfPath } from "../editorPath";
import {
  closeSearchPanel,
  highlightSelectionMatches,
  openSearchPanel,
  search,
  searchKeymap,
  searchPanelOpen,
} from "@codemirror/search";
import { Compartment, EditorState, findClusterBreak } from "@codemirror/state";
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
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { locale, t } from "../i18n";
import { searchPhrases } from "../editorPhrases";
import { countChars, overwriteSpan } from "../editorTyping";

import { theme } from "../theme";

/** `text` = 行番号も構文も持たない素の複数行 (プロンプト)。`json` = 構文色 + パースの lint。
 *  `yaml` = 構文色のみ (spec 28 — 診断は Phase B で backend の既存 lint を linter() に繋ぐ)。 */
type EditorLanguage = "text" | "json" | "yaml";

/** 外部診断 1 件。位置は二形式のどちらか (両方無しなら先頭に出す):
 *  `line` = parse エラーの行 (1 始まり・backend の serde_yaml Location) /
 *  `path` = 未知キーの YAML パス。**構文木でノードの範囲まで**解く (spec 28 v2 —
 *  v1 は backend が行を前方検索で近似しており、flow style では位置を諦めていた)。 */
export interface EditorLintIssue {
  line: number | null;
  path?: string | null;
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
    /** フッタ (行・列・全体行数・文字数・挿入/上書き) を出す。既定 false = 従来の見た目。
     *  **Insert キーの上書き切替もこれに連動する** — モードが見えない場所で打鍵の意味だけ
     *  変わるのは事故なので、表示と機能を対にする。 */
    status?: boolean;
  }>(),
  {
    language: "text",
    placeholder: "",
    readonly: false,
    height: "16rem",
    lintProvider: undefined,
    completionSource: undefined,
    fontSize: 13,
    status: false,
  },
);

const emit = defineEmits<{ (e: "update:modelValue", value: string): void }>();

const host = ref<HTMLElement | null>(null);
/** フッタの表示値。行・列は 1 始まり、文字数は**コードポイント数** (絵文字を 2 と数えない)。 */
const cursorLine = ref(1);
const cursorCol = ref(1);
const totalLines = ref(1);
const totalChars = ref(0);
/** 上書きモード (Insert で切替)。CodeMirror は挿入しか持たないので inputHandler で作る。 */
const overwrite = ref(false);
const modeLabel = computed(() =>
  overwrite.value ? t("editor.statusOverwrite") : t("editor.statusInsert"),
);
const languageCompartment = new Compartment();
const readOnlyCompartment = new Compartment();
const placeholderCompartment = new Compartment();
const themeCompartment = new Compartment();
// 検索・置換パネル (CodeMirror 組み込み) の文言。**キーはライブラリ側の英語文字列**なので
// 表は editorPhrases.ts に置き、ここは差し替え口だけ持つ (テーマと同じく Compartment =
// 言語を切り替えたら開いているエディタにも即反映する。再読込を要求しない)。
const phrasesCompartment = new Compartment();
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

function editorTheme(dark: boolean, fontSize: number) {
  return EditorView.theme(
    {
      "&": {
        // 高さは包みの div が持つ (フッタと縦に並べるため)。ここは容器いっぱい。
        height: "100%",
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

/** 外部診断 → CodeMirror Diagnostic。YAML パスは木でキーそのものへ、行は行全体へ、
 *  どちらも解けなければ先頭 (位置を偽らない)。 */
function externalLinter(provider: (text: string) => Promise<EditorLintIssue[]>) {
  return linter(
    async (view) => {
      const text = view.state.doc.toString();
      const issues = await provider(text);
      const doc = view.state.doc;
      return issues.map((i): Diagnostic => {
        const severity = i.severity === "error" ? "error" : "warning";
        if (i.path) {
          const r = rangeOfPath(text, i.path);
          if (r) return { from: r.from, to: r.to, severity, message: i.message };
        }
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

/** 上書きモードの入力。**行末では何もしない** (次の行を食わない = 一般的なエディタの流儀)。
 *  消す幅は書記素クラスタ単位 (`findClusterBreak`) — サロゲートペアや結合文字を半分にしない。 */
function overwriteHandler() {
  return EditorView.inputHandler.of((view, from, to, text) => {
    if (!overwrite.value || from !== to || text.length === 0) return false;
    const line = view.state.doc.lineAt(from);
    const offset = from - line.from;
    const span = overwriteSpan(line.length, offset, findClusterBreak(line.text, offset));
    if (span === 0) return false;
    view.dispatch({
      changes: { from, to: from + span, insert: text },
      selection: { anchor: from + text.length },
      userEvent: "input.type",
    });
    return true;
  });
}

/** フッタの値をいまの state から取り直す (カーソル移動でも本文変更でも呼ばれる)。 */
function refreshStatus(state: EditorState, docChanged: boolean) {
  const pos = state.selection.main.head;
  const line = state.doc.lineAt(pos);
  cursorLine.value = line.number;
  // 列も**コードポイント数**で数える (文字数と同じ物差し。`pos - line.from` は
  // UTF-16 の符号単位なので、行頭に絵文字が在ると列だけ 1 多く出る)。
  cursorCol.value = countChars(line.text.slice(0, pos - line.from)) + 1;
  totalLines.value = state.doc.lines;
  // 文字数だけは走査が要るので本文が変わったときだけ数える。
  if (docChanged) totalChars.value = countChars(state.doc.toString());
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
        ...(props.status ? [overwriteHandler()] : []),
        keymap.of([
          // 上書き切替は**フッタを出しているときだけ**受ける (モードが見えない場所で
          // 打鍵の意味だけ変わるのを避ける)。
          ...(props.status
            ? [{ key: "Insert", run: () => ((overwrite.value = !overwrite.value), true) }]
            : []),
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
        themeCompartment.of(editorTheme(theme.value === "dark", props.fontSize)),
        phrasesCompartment.of(EditorState.phrases.of(searchPhrases(locale.value))),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            const value = update.state.doc.toString();
            if (value !== props.modelValue) emit("update:modelValue", value);
          }
          if (props.status && (update.docChanged || update.selectionSet)) {
            refreshStatus(update.state, update.docChanged);
          }
        }),
      ],
    }),
  });
  if (props.status) refreshStatus(editor.state, true);
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
watch(locale, (l) => {
  if (!editor) return;
  editor.dispatch({
    effects: phrasesCompartment.reconfigure(EditorState.phrases.of(searchPhrases(l))),
  });
  // **開いている検索パネルは自力では貼り替わらない** — CodeMirror の SearchPanel はラベルを
  // コンストラクタで 1 度だけ組み、`update()` は検索語しか同期しない。ファセットを差し替えた
  // だけだと「言語を変えたのにパネルだけ英語のまま」という半分動く形になるので、開いていれば
  // 閉じて開き直す (検索語・オプションは state 側にあるので失われない)。
  if (searchPanelOpen(editor.state)) {
    closeSearchPanel(editor);
    openSearchPanel(editor);
  }
});
watch([theme, () => props.fontSize], ([th, fs]) =>
  editor?.dispatch({
    effects: themeCompartment.reconfigure(editorTheme(th === "dark", fs as number)),
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
  <!-- 包みが高さを持ち、本体とフッタを縦に並べる (CodeMirror 側は height:100%)。 -->
  <div
    class="flex flex-col overflow-hidden rounded"
    :class="{ 'is-overwrite': overwrite }"
    :style="{ height }"
  >
    <div ref="host" class="min-h-0 flex-1 overflow-hidden" />
    <!-- フッタ: 行・列 / 全体行数・文字数 / 挿入・上書き。YAML はインデントが意味を持つので
         列が効く。等幅にして、桁が動いても行がガタつかないようにする。 -->
    <div
      v-if="status"
      class="flex shrink-0 items-center gap-4 border-t border-ash/40 bg-ash/30 px-2 py-0.5 font-mono text-[11px] leading-5 text-parchment/50"
    >
      <span>{{ t("editor.statusPos", { line: cursorLine, col: cursorCol }) }}</span>
      <span>{{ t("editor.statusDoc", { lines: totalLines, chars: totalChars }) }}</span>
      <!-- 読み取り専用では打鍵できないのでモードを出さない (在る意味の無い表示を作らない)。 -->
      <span
        v-if="!readonly"
        class="ml-auto"
        :class="overwrite ? 'text-ember' : ''"
        :title="t('editor.statusModeTitle')"
        >{{ modeLabel }}</span
      >
    </div>
  </div>
</template>

<style scoped>
/* 上書きモードはカーソルを太くする — フッタの語だけだと、打鍵の意味が変わったことに
   本文を見ている目が気づけない。CodeMirror の内部要素なので :deep() で届かせる。 */
.is-overwrite :deep(.cm-cursor) {
  border-left-width: 0.55em;
  opacity: 0.45;
}
</style>
