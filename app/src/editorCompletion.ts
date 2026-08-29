/**
 * spec 28 v2: エディタ補完の**文脈解決**。
 *
 * v1 との違いは一点 — 文脈をヒューリスティクスで推さず、**構文木のパスを配線で辿って**
 * 確定する。配線 (`vocab.wiring`) は gm_core の未知キー lint が使う表そのもので、
 * ここに写しは無い (「補完に出るのに lint に叱られる」乖離が構造的に起きない)。
 *
 * 何が直ったか:
 * - **多行で書いた gate / op のバリアント絞り込み** — v1 は `kind:` が同じ行に無いと
 *   全バリアントの和集合へ落ちていた。木なら兄弟の Pair として見える。
 * - **同じキーが親で割れる** — `to:` は `Exit` なら場所・`give_item` ならエンティティ、
 *   `start:` は campaign ならモジュール。v1 は平らな表 + docKind の場当たりで補っていた。
 * - **作者の付けた名前の段** (`locations:` の場所名) を段として扱う。v1 は祖父キーを
 *   引く近似で、名前の位置にも型のキーを出していた。
 */
import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import { contextAt, harvestIds, type IdHarvest } from "./editorPath";

/** 候補 1 つ。`doc` は backend が型の doc comment から抽出したもの (無いことがある)。 */
export interface VocabItem {
  name: string;
  doc?: string;
}

/** 配線 1 本 (gm_core の表。`kind` = direct/seq/map_values/item_map/stat_map)。 */
export interface WiringEntry {
  parent: string;
  key: string;
  kind: string;
  child: string;
  /** その mapping が `kind:` を持つときの子文脈 (`Location.items` の旧形式 = Gate)。 */
  child_tagged?: string;
}

export interface EditorVocabulary {
  roots: Record<string, string>;
  contexts: Record<string, VocabItem[]>;
  wiring: WiringEntry[];
  gate_variant_keys: Record<string, VocabItem[]>;
  op_variant_keys: Record<string, VocabItem[]>;
  tag_values: Record<string, VocabItem[]>;
  ids: Record<string, VocabItem[]>;
}

type Tag = { kind?: string; op?: string };

/** 一覧の脇に出す短い説明。1 文目 (「。」まで) か 42 字で切る。 */
function shortDoc(doc: string): string {
  const period = doc.indexOf("。");
  const head = period > 0 && period <= 42 ? doc.slice(0, period) : doc;
  return head.length > 42 ? `${head.slice(0, 42)}…` : head;
}

/** VocabItem → CodeMirror の候補。説明があれば脇 (detail) と詳細 (info) に出す。 */
function toCompletion(item: VocabItem, type: string, apply?: string, boost?: number): Completion {
  const c: Completion = { label: item.name, type };
  if (boost !== undefined) c.boost = boost;
  if (apply) c.apply = apply;
  if (item.doc) {
    c.detail = shortDoc(item.doc);
    // 畳んだ分が失われるときだけ全文をポップアップに回す。
    if (c.detail !== item.doc) c.info = item.doc;
  }
  return c;
}

/** 名前つき容器 (`map_values` / `item_map` / `stat_map`) は「作者の名前」の段を挟む。 */
const NAMED = new Set(["map_values", "item_map", "stat_map"]);

export interface Resolved {
  ctx: string;
  /** カーソルが作者の自由語彙 (map のキー) の位置に在る = 型のキーは出さない。 */
  atName: boolean;
}

/** パスを配線で辿って文脈を確定する。未知キー (作者の名前) は文脈を変えない。 */
export function resolveContext(vocab: EditorVocabulary, root: string, path: string[]): Resolved {
  let ctx = root;
  let pendingName = false;
  for (const seg of path) {
    if (pendingName) {
      pendingName = false; // このセグメントが名前を消費する
      continue;
    }
    if (seg === "[]") continue;
    const e = vocab.wiring.find((w) => w.parent === ctx && w.key === seg);
    if (!e) continue; // 未知キー = 型の外 (葉 or 作者の語彙)。文脈は据え置き。
    ctx = e.child;
    if (NAMED.has(e.kind)) pendingName = true;
  }
  return { ctx, atName: pendingName };
}

/** 文脈 + 兄弟タグ → キー候補。Gate/Op はタグでバリアントまで絞る。 */
/** その文脈でまだ決まっていないタグ欄 (これを書くまで他の欄が決まらない)。 */
function needsTagIn(ctx: string, tag: Tag): "kind" | "op" | null {
  if ((ctx === "Gate" || ctx === "LocationItem") && !tag.kind) return "kind";
  if (ctx === "Op" && !tag.op) return "op";
  return null;
}

function keysFor(vocab: EditorVocabulary, ctx: string, tag: Tag): VocabItem[] {
  if (ctx === "Gate" && tag.kind) {
    return vocab.gate_variant_keys[tag.kind] ?? vocab.contexts["Gate"] ?? [];
  }
  if (ctx === "Op" && tag.op) {
    return vocab.op_variant_keys[tag.op] ?? vocab.contexts["Op"] ?? [];
  }
  // Location.items の旧形式 (Gate 直書き) — 配線が child_tagged で渡してくる分岐。
  if (ctx === "LocationItem" && tag.kind) {
    return vocab.gate_variant_keys[tag.kind] ?? vocab.contexts["Gate"] ?? [];
  }
  return vocab.contexts[ctx] ?? [];
}

/** `op:` の値 → その op の `key:` が指すもの (フラグか数値か)。 */
function keyCategoryByOp(op?: string): string[] {
  switch (op) {
    case "set_flag":
      return ["flags"];
    case "adjust_stat":
    case "scale_stat":
    case "record_turn":
    case "roll_stat":
    case "check_under":
    case "set_attribute":
      return ["stats"];
    default:
      return ["flags", "stats"]; // 絞れないときは両方 (黙るより広く)
  }
}

/** `kind:` の値 → その gate の `key:` が指すもの。 */
function keyCategoryByKind(kind?: string): string[] {
  switch (kind) {
    case "flag_is":
      return ["flags"];
    case "stat_at_least":
    case "stat_at_most":
    case "turns_since":
    case "attribute_is":
      return ["stats"];
    default:
      return ["flags", "stats"];
  }
}

/**
 * (文脈, 欄名) → id カテゴリ。**文脈で引くのが v2 の要点** — v1 は欄名だけで引いて
 * いたので `to` / `from` / `start` の多義を docKind の場当たりで補っていた。
 *
 * この表は「その欄が何を指すか」という意味の話で、型からは導けない (Rust 側でも
 * `LocationId` は `String` の別名でしかなく serde には残らない) ので手で持つ。
 */
function idCategories(ctx: string, field: string, tag: Tag): string[] {
  switch (ctx) {
    case "Op":
      switch (field) {
        case "to":
          return tag.op === "give_item" ? ["entities"] : ["locations"];
        case "from":
        case "entity":
        case "voter":
        case "target":
          return ["entities"];
        case "item":
          return ["items"];
        case "skill":
          return ["skills"];
        case "challenge":
          return ["challenges"];
        case "contest":
          return ["contests"];
        case "stat":
          return ["stats"];
        case "key":
          return keyCategoryByOp(tag.op);
        default:
          return [];
      }
    case "Gate":
      switch (field) {
        case "at":
          return ["locations"];
        case "entity":
          return ["entities"];
        case "item":
          return ["items"];
        case "skill":
          return ["skills"];
        case "key":
          return keyCategoryByKind(tag.kind);
        default:
          return [];
      }
    case "Exit":
      return field === "to" ? ["locations"] : [];
    case "Scenario":
      switch (field) {
        case "start":
          return ["locations"];
        case "cast":
        case "party":
          return ["entities"];
        case "allowed_flags":
        case "global_flags":
        case "persistent_flags":
        case "hidden_flags":
        case "internal_flags":
          return ["flags"];
        case "hidden_stats":
        case "internal_stats":
        case "secret_attributes":
        case "hidden_attributes":
          return ["stats"];
        case "initial_skills":
          return ["skills"];
        case "initial_inventory":
          return ["items"];
        default:
          return [];
      }
    case "Location":
      switch (field) {
        case "present":
          return ["entities"];
        case "image":
          return ["images"];
        case "bgm":
          return ["audios"];
        default:
          return [];
      }
    case "Trigger":
      if (field === "image") return ["images"];
      if (field === "sound") return ["audios"];
      return [];
    case "Outcome":
    case "Tier":
      if (field === "sound") return ["audios"];
      if (field === "flag") return ["flags"];
      return [];
    case "Challenge":
      if (field === "entity") return ["entities"];
      if (field === "stat") return ["stats"];
      return [];
    case "Contest":
      if (field === "opponent") return ["entities"];
      return [];
    case "RollSpec":
      return field === "stat" ? ["stats"] : [];
    case "CharacterDef":
      switch (field) {
        case "icon":
          return ["images"];
        case "skills":
          return ["skills"];
        case "inventory":
          return ["items"];
        default:
          return [];
      }
    case "Protagonist":
      return field === "icon" ? ["images"] : [];
    case "RoleAssignment":
      return field === "among" ? ["entities"] : [];
    case "SpendRules":
    case "PushCost":
      return field === "from" ? ["stats"] : [];
    case "Campaign":
      return field === "start" ? ["modules"] : [];
    case "CampaignEdge":
      if (field === "from" || field === "to") return ["modules"];
      if (field === "on_goal") return ["goals"];
      return [];
    case "PlayerDef":
      if (field === "icon") return ["images"];
      if (field === "skills") return ["skills"];
      if (field === "items") return ["items"];
      return [];
    case "Globals":
      return field === "flags" ? ["flags"] : [];
    default:
      return [];
  }
}

/** バッファから id を拾うときの容器 (カテゴリ名は backend の `ids` と揃える)。 */
const HARVEST: IdHarvest = {
  mapKeys: {
    locations: "locations",
    characters: "entities",
    challenges: "challenges",
    contests: "contests",
    initial_stats: "stats",
    stats: "stats",
    attributes: "stats",
    initial_attributes: "stats",
  },
  listItems: {
    allowed_flags: "flags",
    global_flags: "flags",
    persistent_flags: "flags",
    hidden_flags: "flags",
    internal_flags: "flags",
    initial_skills: "skills",
    skills: "skills",
    initial_inventory: "items",
    inventory: "items",
    items: "items",
    cast: "entities",
    present: "entities",
    party: "entities",
  },
  itemIds: { goals: "goals" },
};

/** 保存済み語彙 + バッファの id を合流 (同名は保存済みの説明を優先)。 */
function idOptions(
  cats: string[],
  vocab: EditorVocabulary,
  buffer: Record<string, Set<string>>,
): VocabItem[] {
  const merged = new Map<string, VocabItem>();
  for (const cat of cats) {
    for (const item of vocab.ids[cat] ?? []) merged.set(item.name, item);
    for (const name of buffer[cat] ?? []) if (!merged.has(name)) merged.set(name, { name });
  }
  return [...merged.values()];
}

/** カーソル位置で出すべき候補を決める (純粋 — CodeMirror に依らないのでテストできる)。 */
export function candidatesAt(
  vocab: EditorVocabulary,
  docKind: string,
  text: string,
  pos: number,
): { options: VocabItem[]; isKey: boolean; needsTag: "kind" | "op" | null } {
  const root = vocab.roots[docKind];
  if (!root) return { options: [], isKey: false, needsTag: null };
  const { path, where, blockValue, tag } = contextAt(text, pos);

  if (where === "key") {
    const { ctx, atName } = resolveContext(vocab, root, path);
    // 作者の自由語彙の段 (場所名・キャラ名) では型のキーを出さない。
    return {
      options: atName ? [] : keysFor(vocab, ctx, tag),
      isKey: true,
      needsTag: atName ? null : needsTagIn(ctx, tag),
    };
  }

  // 値 / 列の要素。どちらも「実はキーの位置」でありうるので、配線で判別する。
  const isItem = where === "item";
  const fieldPath = isItem ? path.slice(0, -1) : path;
  const field = fieldPath[fieldPath.length - 1] ?? "";
  const outer = resolveContext(vocab, root, fieldPath.slice(0, -1));
  const wired = vocab.wiring.find((w) => w.parent === outer.ctx && w.key === field);

  // 列の要素: 容器が mapping の列 (`exits:`) ならキーの位置。scalar の列 (`cast:`) は値。
  if (isItem && wired?.kind === "seq") {
    return {
      options: keysFor(vocab, wired.child, tag),
      isKey: true,
      needsTag: needsTagIn(wired.child, tag),
    };
  }
  // 別の行から始まる空の値は、構文上は値でも**入れ子 mapping の最初のキー**でありうる。
  // 分かれ目は配線 — 作者の名前の段の内側 (`locations: room: |`) か、
  // その欄が構造を持つ (`when: |`) なら キー。素の文字列欄 (`description: |`) は値のまま。
  if (where === "value" && blockValue) {
    if (outer.atName) {
      return { options: keysFor(vocab, outer.ctx, tag), isKey: true, needsTag: needsTagIn(outer.ctx, tag) };
    }
    if (wired?.kind === "direct") {
      return {
        options: keysFor(vocab, wired.child, tag),
        isKey: true,
        needsTag: needsTagIn(wired.child, tag),
      };
    }
    if (wired && NAMED.has(wired.kind)) return { options: [], isKey: true, needsTag: null }; // 名前の段
  }
  // `op:` / `kind:` の値はバリアント名そのもの。
  if (field === "op" || field === "kind") {
    return { options: vocab.tag_values[field] ?? [], isKey: false, needsTag: null };
  }
  const cats = idCategories(outer.ctx, field, tag);
  return {
    options: idOptions(cats, vocab, harvestIds(text, HARVEST)),
    isKey: false,
    needsTag: null,
  };
}

/** 補完ソースを作る。vocab / kind は getter で受ける (ファイル切替・保存後の語彙更新に追従)。 */
export function makeCompletionSource(
  getVocab: () => EditorVocabulary | null,
  getDocKind: () => string,
) {
  return (context: CompletionContext): CompletionResult | null => {
    const vocab = getVocab();
    if (!vocab) return null;
    const line = context.state.doc.lineAt(context.pos);
    const before = line.text.slice(0, context.pos - line.from);
    // いま打ちかけの語 (候補の置換範囲を決めるだけ — 文脈判定は木が行う)。
    const typed = /([A-Za-z_][A-Za-z0-9_]*)?$/.exec(before)?.[1] ?? "";

    const { options, isKey, needsTag } = candidatesAt(
      vocab,
      getDocKind(),
      context.state.doc.toString(),
      context.pos,
    );
    if (!options.length) return null;
    // キー欄は明示要求か打ち始めてから (行頭で常に開くとうるさい)。
    // 値欄は「: 」の直後に開く — id の typo = 死んだ参照の**上流予防**が本命。
    if (isKey && !typed && !context.explicit) return null;
    return {
      from: context.pos - typed.length,
      options: options.map((i) =>
        isKey
          // タグ欄 (`kind:`/`op:`) がまだ無い文脈では**それを先頭へ** — これを書くまで
          // 他の欄は決まらないのに、名前順だと真ん中に埋もれる (ユーザー実測 2026-08-29)。
          ? toCompletion(i, "property", `${i.name}: `, i.name === needsTag ? 99 : undefined)
          : toCompletion(i, "constant"),
      ),
      validFor: isKey ? /^[A-Za-z_][A-Za-z0-9_]*$/ : /^[^\s:,{}[\]]*$/,
    };
  };
}
