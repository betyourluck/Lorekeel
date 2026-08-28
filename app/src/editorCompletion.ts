/**
 * spec 28 Phase C: エディタ補完のヒューリスティクス。
 *
 * 語彙は backend (`editor_vocabulary`) が組む — キー名は型から導出・**説明は doc comment
 * から機械抽出**・id はディスクの保存済みパッケージから。ここが持つのは**文脈の推定だけ**:
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
 *
 * **編集中のバッファも id 源にする (2026-08-28)**: backend の id は「保存済みの現物」なので、
 * いま打ったばかりの場所名・フラグ名が出ない。**書きかけの盤面こそ補完が要る**ので、素朴な
 * 行走査でバッファからも拾って合流する (v2 で Lezer 構文木に置き換わる想定の暫定実装)。
 */
import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import type { EditorState } from "@codemirror/state";

/** 候補 1 つ。`doc` は backend が型の doc comment から抽出したもの (無いことがある)。 */
export interface VocabItem {
  name: string;
  doc?: string;
}

export interface EditorVocabulary {
  root_keys: Record<string, VocabItem[]>;
  key_contexts: Record<string, VocabItem[]>;
  map_child_keys: Record<string, VocabItem[]>;
  gate_variant_keys: Record<string, VocabItem[]>;
  op_variant_keys: Record<string, VocabItem[]>;
  tag_values: Record<string, VocabItem[]>;
  ids: Record<string, VocabItem[]>;
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

/** 一覧の脇に出す短い説明。1 文目 (「。」まで) か 42 字で切る。 */
function shortDoc(doc: string): string {
  const period = doc.indexOf("。");
  const head = period > 0 && period <= 42 ? doc.slice(0, period) : doc;
  return head.length > 42 ? `${head.slice(0, 42)}…` : head;
}

/** VocabItem → CodeMirror の候補。説明があれば脇 (detail) と詳細 (info) に出す。 */
function toCompletion(item: VocabItem, type: string, apply?: string): Completion {
  const c: Completion = { label: item.name, type };
  if (apply) c.apply = apply;
  if (item.doc) {
    c.detail = shortDoc(item.doc);
    // 畳んだ分が失われるときだけ全文をポップアップに回す。
    if (c.detail !== item.doc) c.info = item.doc;
  }
  return c;
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
    // アセット ID (spec 01)。**engine は宣言を持たない**ので死んだ参照 lint の射程外 —
    // 実名を出す補完がタイポの唯一の予防になる (backend はディスクの実ファイルを集めている)。
    case "image":
    case "icon":
      return ["images"];
    case "bgm":
    case "sound":
      return ["audios"];
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
): VocabItem[] {
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
  const all = new Map<string, VocabItem>();
  for (const list of Object.values(vocab.key_contexts)) for (const k of list) all.set(k.name, k);
  for (const list of Object.values(vocab.map_child_keys)) for (const k of list) all.set(k.name, k);
  for (const k of vocab.root_keys[docKind] ?? []) all.set(k.name, k);
  return [...all.values()].sort((a, b) => a.name.localeCompare(b.name));
}

// --- 編集中のバッファから拾う id ------------------------------------------------------------
/** 名前つき map 容器 (キーが id)。 */
const BUFFER_MAP_IDS: Record<string, string> = {
  locations: "locations",
  characters: "entities",
  challenges: "challenges",
  contests: "contests",
  initial_stats: "stats",
  stats: "stats",
  attributes: "stats", // 属性キーは stat と同じ「宣言済みキー」の欄に出る (絞れないより広く)
};
/** 列 (`- item`) が id。 */
const BUFFER_LIST_IDS: Record<string, string> = {
  allowed_flags: "flags",
  global_flags: "flags",
  persistent_flags: "flags",
  hidden_flags: "flags",
  internal_flags: "flags",
  initial_skills: "skills",
  initial_inventory: "items",
  skills: "skills",
  inventory: "items",
};

/**
 * いま開いているバッファから id を拾う (保存前でも補完が効くように)。
 *
 * **素朴な行走査**で、容器の直下だけを見る (`locations:` の子キー / `allowed_flags:` の列 /
 * `goals:` の `- id:`)。深い入れ子や flow style は拾わない — 拾えないものは backend の
 * 保存済み語彙が埋める。v2 で Lezer 構文木に置き換われば、この近似ごと消える。
 */
function bufferIds(state: EditorState): Record<string, Set<string>> {
  const out: Record<string, Set<string>> = {};
  const add = (cat: string, id: string) => {
    if (!id) return;
    (out[cat] ??= new Set()).add(id);
  };
  let container: string | null = null;
  let containerIndent = 0;
  let childIndent = -1;

  const lines = state.doc.toString().split("\n");
  for (const raw of lines) {
    if (!raw.trim() || raw.trim().startsWith("#")) continue;
    const ind = effIndent(raw);
    const key = keyOf(raw);

    if (container !== null && ind <= containerIndent) container = null;

    if (container === null) {
      if (key && (BUFFER_MAP_IDS[key] || BUFFER_LIST_IDS[key] || key === "goals")) {
        container = key;
        containerIndent = ind;
        childIndent = -1;
      }
      continue;
    }

    // 容器の直下だけ (最初に見つけた深さを子とみなす)。
    if (childIndent < 0) childIndent = ind;
    if (ind !== childIndent) continue;

    if (container === "goals") {
      const m = /^\s*-\s*id:\s*([^\s#]+)/.exec(raw);
      if (m) add("goals", m[1]);
      continue;
    }
    const listCat = BUFFER_LIST_IDS[container];
    if (listCat) {
      const m = /^\s*-\s*([^\s#][^#]*?)\s*$/.exec(raw);
      if (m) add(listCat, m[1].replace(/^["']|["']$/g, ""));
      continue;
    }
    const mapCat = BUFFER_MAP_IDS[container];
    if (mapCat && key) add(mapCat, key);
  }
  return out;
}

/** 保存済み語彙 + バッファの id を合流 (同名は保存済みの説明を優先)。 */
function idOptions(cats: string[], vocab: EditorVocabulary, buffer: Record<string, Set<string>>): VocabItem[] {
  const merged = new Map<string, VocabItem>();
  for (const cat of cats) {
    for (const item of vocab.ids[cat] ?? []) merged.set(item.name, item);
    for (const name of buffer[cat] ?? []) if (!merged.has(name)) merged.set(name, { name });
  }
  return [...merged.values()];
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
      let options: VocabItem[];
      if (field === "op" || field === "kind") {
        options = vocab.tag_values[field] ?? [];
      } else {
        options = idOptions(idCategories(field, tag, docKind), vocab, bufferIds(context.state));
      }
      if (!options.length) return null;
      // 値欄は typed が空でも開く (「: 」を打った直後に候補が見えるのが本命 —
      // id の typo = 死んだ参照の上流予防。キー欄と違い explicit を要求しない)。
      return {
        from: context.pos - typed.length,
        options: options.map((i) => toCompletion(i, "constant")),
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
      options: keys.map((i) => toCompletion(i, "property", `${i.name}: `)),
      validFor: /^[A-Za-z_][A-Za-z0-9_]*$/,
    };
  };
}
