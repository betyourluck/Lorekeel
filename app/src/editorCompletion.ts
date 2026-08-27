/**
 * spec 28 Phase C: エディタ補完のヒューリスティクス。
 *
 * 語彙は backend (`editor_vocabulary`) が組む — キー名は型から導出・id はディスクの
 * 保存済みパッケージから。ここが持つのは**文脈の推定だけ**:
 *
 * 1. 行内タグ最優先 — 実 content は flow style (`- { op: set_flag, key: done }`) が
 *    多く、インデント遡りは行内では効かない。行に `op:`/`kind:` が見えたら
 *    バリアント別キー表で絞る (lint のバリアント別検査と同じ解像度)。
 * 2. 無ければインデント遡りの親キー (`key_contexts`)。親が作者の付けた名前
 *    (locations の場所名など) なら祖父キーで `map_child_keys` を引く。
 * 3. それでも判別できなければ全キー (spec の v1 妥協 — 出さないより広く出す)。
 *
 * 値の補完はフィールド名 → id カテゴリの写像。`to:` や `key:` のような多義は
 * 行内タグで解き、解けなければ候補カテゴリを併記する (絞れないときに黙らない)。
 */
import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";

export interface EditorVocabulary {
  root_keys: Record<string, string[]>;
  key_contexts: Record<string, string[]>;
  map_child_keys: Record<string, string[]>;
  gate_variant_keys: Record<string, string[]>;
  op_variant_keys: Record<string, string[]>;
  tag_values: Record<string, string[]>;
  ids: Record<string, string[]>;
}

interface LineTag {
  op?: string;
  kind?: string;
}

/** 実効インデント: 先頭の空白 + リスト dash (`- `) を除いた位置。
 *  `- id: t1` の中の欄 (`when:`) は dash 込みの深さで並ぶので、dash を +2 で数えると
 *  兄弟が同じ深さ・容器 (`triggers:`) が浅い、という YAML の見た目どおりに歩ける。 */
function effIndent(text: string): number {
  const m = /^(\s*)(- )?/.exec(text);
  return (m?.[1]?.length ?? 0) + (m?.[2] ? 2 : 0);
}

function keyOf(text: string): string | null {
  const m = /^\s*(?:- )?([A-Za-z_][A-Za-z0-9_]*):/.exec(text);
  return m ? m[1] : null;
}

/** 値フィールド → id カテゴリ。多義は行内タグで解く。 */
function idCategories(field: string, tag: LineTag, docKind: string): string[] {
  switch (field) {
    case "at":
      return ["locations"];
    case "start":
      // campaign.start は開始モジュール / scenario.start は開始場所。
      return docKind === "campaign" ? ["modules"] : ["locations"];
    case "to":
      if (tag.op === "give_item") return ["entities"];
      if (tag.op === "move") return ["locations"];
      // exits[].to = 場所 / campaign edges[].to = モジュール。
      return docKind === "campaign" ? ["modules"] : ["locations"];
    case "key":
      if (tag.op === "set_flag" || tag.kind === "flag_is") return ["flags"];
      if (
        tag.op === "adjust_stat" ||
        tag.op === "scale_stat" ||
        tag.op === "record_turn" ||
        tag.op === "roll_stat" ||
        tag.op === "check" ||
        tag.op === "check_under" ||
        tag.kind?.startsWith("stat_") ||
        tag.kind === "turns_since"
      )
        return ["stats"];
      return ["flags", "stats"]; // 絞れないときは両方 (黙るより広く)
    case "item":
      return ["items"];
    case "flag":
      return ["flags"];
    case "entity":
    case "opponent":
    case "among":
      return ["entities"];
    case "from":
      if (tag.op === "give_item") return ["entities"];
      // campaign edges[].from = モジュール / spend_rules.from = stat。
      return docKind === "campaign" ? ["modules"] : ["stats", "entities"];
    case "challenge":
      return ["challenges"];
    case "contest":
      return ["contests"];
    case "skill":
      return ["skills"];
    case "stat":
      return ["stats"];
    case "on_goal":
      return ["goals"];
    case "cast":
    case "present":
    case "party":
      return ["entities"];
    default:
      return [];
  }
}

/** キー候補: 行内タグ > 親キー > 祖父 (map 容器) > 全キー。 */
function keyCandidates(
  context: CompletionContext,
  lineNumber: number,
  tag: LineTag,
  vocab: EditorVocabulary,
  docKind: string,
): string[] {
  if (tag.op) return vocab.op_variant_keys[tag.op] ?? vocab.key_contexts["effects"] ?? [];
  if (tag.kind) return vocab.gate_variant_keys[tag.kind] ?? vocab.key_contexts["when"] ?? [];

  const doc = context.state.doc;
  const myIndent = effIndent(doc.line(lineNumber).text);
  if (myIndent === 0) return vocab.root_keys[docKind] ?? [];

  // 親 = 上方向で最初の「浅い」行。
  let pLine = -1;
  let pIndent = -1;
  for (let n = lineNumber - 1; n >= 1; n--) {
    const t = doc.line(n).text;
    if (!t.trim()) continue;
    const ind = effIndent(t);
    if (ind < myIndent) {
      pLine = n;
      pIndent = ind;
      break;
    }
  }
  if (pLine < 0) return vocab.root_keys[docKind] ?? [];
  const parent = keyOf(doc.line(pLine).text);
  if (parent && vocab.key_contexts[parent]) return vocab.key_contexts[parent];

  // 親が作者の付けた名前 (locations の場所名など) → 祖父キーで map 容器を引く。
  for (let n = pLine - 1; n >= 1; n--) {
    const t = doc.line(n).text;
    if (!t.trim()) continue;
    const ind = effIndent(t);
    if (ind < pIndent) {
      const g = keyOf(t);
      if (g && vocab.map_child_keys[g]) return vocab.map_child_keys[g];
      break;
    }
  }

  // 判別できない → 全キー (spec v1 の妥協。出さないより広く出す)。
  const all = new Set<string>();
  for (const list of Object.values(vocab.key_contexts)) for (const k of list) all.add(k);
  for (const list of Object.values(vocab.map_child_keys)) for (const k of list) all.add(k);
  for (const k of vocab.root_keys[docKind] ?? []) all.add(k);
  return [...all].sort();
}

/** 補完ソースを作る。vocab / kind は getter で受ける (ファイル切替・保存後の語彙更新に追従)。 */
export function makeCompletionSource(
  getVocab: () => EditorVocabulary | null,
  getDocKind: () => string,
) {
  return (context: CompletionContext): CompletionResult | null => {
    const vocab = getVocab();
    if (!vocab) return null;
    const docKind = getDocKind();
    const line = context.state.doc.lineAt(context.pos);
    const before = line.text.slice(0, context.pos - line.from);
    const tag: LineTag = {
      op: /\bop:\s*([A-Za-z_]+)/.exec(line.text)?.[1],
      kind: /\bkind:\s*([A-Za-z_]+)/.exec(line.text)?.[1],
    };

    // --- 値の補完: `field: 入力途中` ---
    const vm = /([A-Za-z_][A-Za-z0-9_]*)\s*:\s*([^\s:,{}[\]]*)$/.exec(before);
    if (vm) {
      const field = vm[1];
      const typed = vm[2];
      let options: string[];
      if (field === "op" || field === "kind") {
        options = vocab.tag_values[field] ?? [];
      } else {
        options = idCategories(field, tag, docKind).flatMap((c) => vocab.ids[c] ?? []);
      }
      if (!options.length) return null;
      // 値欄は typed が空でも開く (「: 」を打った直後に候補が見えるのが本命 —
      // id の typo = 死んだ参照の上流予防。キー欄と違い explicit を要求しない)。
      return {
        from: context.pos - typed.length,
        options: [...new Set(options)].map((label): Completion => ({ label, type: "constant" })),
        validFor: /^[^\s:,{}[\]]*$/,
      };
    }

    // --- キーの補完: 行頭 / `- ` / `{ ` / `, ` の後の単語 ---
    const km = /(?:^|[-{,]\s*|^\s+|\s)([A-Za-z_][A-Za-z0-9_]*)?$/.exec(before);
    if (!km) return null;
    const typed = km[1] ?? "";
    if (!typed && !context.explicit) return null;
    const keys = keyCandidates(context, line.number, tag, vocab, docKind);
    if (!keys.length) return null;
    return {
      from: context.pos - typed.length,
      options: keys.map(
        (label): Completion => ({ label, type: "property", apply: `${label}: ` }),
      ),
      validFor: /^[A-Za-z_][A-Za-z0-9_]*$/,
    };
  };
}
