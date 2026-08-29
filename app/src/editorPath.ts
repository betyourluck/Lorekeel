/**
 * spec 28 v2: **テキスト上の位置 ↔ YAML パス** の写像。
 *
 * v1 はこの写像を持たず、三箇所で別々に近似していた:
 * ①補完の文脈推定 (インデント遡り + 行内タグの正規表現) ②診断の行推定 (backend の
 * 親キー列の前方検索) ③バッファ id の収集 (素朴な行走査)。どれも flow style・多行の
 * 入れ子・引用キーで破れる。三つとも同じ欠落から出ていたので、木で一度に解く。
 *
 * 木は `@lezer/yaml` — `@codemirror/lang-yaml` が既に積んでいる構文色用のパーサで、
 * 追加の依存は無い。
 *
 * **入力途中でも解ける**のが要点。カーソル位置に短い番兵を差し込んで parse し直すと、
 * 空行・`- ` の直後・`{ op: x, ` の途中でも「いまどこに居るか」が確定する
 * (Lezer は誤り耐性があるが、**文字が無い位置には節点が生えない**ので番兵で生やす)。
 */
import { parser } from "@lezer/yaml";
import type { SyntaxNode, Tree } from "@lezer/common";

/** 番兵。YAML の識別子として無害で、実 content にまず現れない綴り。 */
const PROBE = "zqxprobe";

/** カーソルが何を打っているか。 */
export type CursorWhere =
  /** キー名 (`entity:` の左) を打っている。`path` は**その mapping まで**。 */
  | "key"
  /** 値 (`entity: a` の右) を打っている。`path` の末尾がその欄名。 */
  | "value"
  /** 列の要素そのもの (`- ` の直後)。`path` の末尾は `[]`、その手前が容器のキー。
   *  容器が mapping の列 (`exits`) なのか scalar の列 (`allowed_flags`) なのかは
   *  **構文では決まらない**ので、呼び出し側が配線で判別する。 */
  | "item";

export interface CursorContext {
  path: string[];
  where: CursorWhere;
  /** 値が**キーと別の行**から始まっている (`room:` の下の行) = 構文上は値だが、
   *  中身が空なら「入れ子 mapping の最初のキー」でもありうる曖昧な位置。
   *  どちらかは構文では決まらないので、呼び出し側が配線で判別する。 */
  blockValue: boolean;
  /** カーソルを囲む mapping の兄弟 `kind:` / `op:` の値 (バリアント絞り込み用)。
   *  列の境界を跨いでは拾わない (`of:` の新要素が親 gate の kind を拾わないように)。 */
  tag: { kind?: string; op?: string };
}

/** 引用キー (`"a b"`) の引用符を落とす。 */
function unquote(s: string): string {
  return s.replace(/^(['"])([\s\S]*)\1$/, "$2");
}

function keyText(doc: string, pair: SyntaxNode): string | null {
  const k = pair.getChild("Key");
  return k ? unquote(doc.slice(k.from, k.to)) : null;
}

/** ある節点を囲む直近の mapping から `kind:` / `op:` を拾う。
 *  途中で列 (Item / *Sequence) を跨いだら諦める (別の要素のタグを拾わないため)。 */
function siblingTag(doc: string, node: SyntaxNode): { kind?: string; op?: string } {
  let map: SyntaxNode | null = null;
  for (let c: SyntaxNode | null = node; c; c = c.parent) {
    if (c.name === "Item" || c.name === "BlockSequence" || c.name === "FlowSequence") break;
    if (c.name === "BlockMapping" || c.name === "FlowMapping") {
      map = c;
      break;
    }
  }
  const tag: { kind?: string; op?: string } = {};
  if (!map) return tag;
  for (let ch = map.firstChild; ch; ch = ch.nextSibling) {
    if (ch.name !== "Pair") continue;
    const name = keyText(doc, ch);
    if (name !== "kind" && name !== "op") continue;
    // Pair = Key ":" 値。値は Key の 2 つ後ろ。
    const value = ch.getChild("Key")?.nextSibling?.nextSibling;
    if (value) tag[name] = unquote(doc.slice(value.from, value.to)).trim();
  }
  return tag;
}

/** 節点からルートまでのパス (`Pair` のキー / 列は `[]`)。 */
function pathOf(doc: string, node: SyntaxNode): string[] {
  const segs: string[] = [];
  for (let c: SyntaxNode | null = node; c; c = c.parent) {
    if (c.name === "Pair") segs.push(keyText(doc, c) ?? "?");
    else if (c.name === "Item") segs.push("[]");
  }
  return segs.reverse();
}

/**
 * カーソル位置の文脈を解く。番兵を差し込んで parse するので、
 * **書きかけ・空行・列の頭でも**解ける (v1 が近似で埋めていた場所)。
 */
export function contextAt(text: string, pos: number): CursorContext {
  const doc = text.slice(0, pos) + PROBE + text.slice(pos);
  const at = pos + PROBE.length;
  const node = parser.parse(doc).resolveInner(at, -1);

  // 番兵が Key の中に入ったか (= キーを打っている)。Pair より内側で判定する。
  let inKey = false;
  for (let c: SyntaxNode | null = node; c; c = c.parent) {
    if (c.name === "Key") {
      inKey = true;
      break;
    }
    if (c.name === "Pair") break;
  }
  const tag = siblingTag(doc, node);
  const path = pathOf(doc, node);

  if (inKey) {
    // 末尾は番兵自身 (打ちかけのキー) — 落とすと「その mapping まで」になる。
    return { path: path.slice(0, -1), where: "key", blockValue: false, tag };
  }
  // Pair の値でないなら列の要素そのもの (`- ` の直後 / scalar の列)。
  const isPairValue = path.length > 0 && path[path.length - 1] !== "[]";
  let blockValue = false;
  if (isPairValue) {
    let c: SyntaxNode | null = node;
    while (c && c.name !== "Pair") c = c.parent;
    const k = c?.getChild("Key");
    blockValue = !!k && doc.slice(k.to, at).includes("\n");
  }
  return { path, where: isPairValue ? "value" : "item", blockValue, tag };
}

/** 1 セグメント = キー名 + 列添字。`effects[1]` → `{ key: "effects", indices: [1] }`。 */
function splitSeg(seg: string): { key: string; indices: number[] } {
  const indices: number[] = [];
  const key = seg.replace(/\[(\d+)\]/g, (_, n) => {
    indices.push(Number(n));
    return "";
  });
  return { key, indices };
}

/** mapping 節点の直下から名前の一致する Pair を探す。 */
function findPair(doc: string, map: SyntaxNode, name: string): SyntaxNode | null {
  for (let ch = map.firstChild; ch; ch = ch.nextSibling) {
    if (ch.name === "Pair" && keyText(doc, ch) === name) return ch;
  }
  return null;
}

/** Pair の値の節点 (`Key ":" 値` の 3 つ目)。 */
function valueOf(pair: SyntaxNode): SyntaxNode | null {
  return pair.getChild("Key")?.nextSibling?.nextSibling ?? null;
}

function asMapping(node: SyntaxNode | null): SyntaxNode | null {
  if (!node) return null;
  if (node.name === "BlockMapping" || node.name === "FlowMapping") return node;
  return null;
}

function nthItem(seq: SyntaxNode | null, i: number): SyntaxNode | null {
  if (!seq || (seq.name !== "BlockSequence" && seq.name !== "FlowSequence")) return null;
  let n = 0;
  for (let ch = seq.firstChild; ch; ch = ch.nextSibling) {
    // FlowSequence の要素は Item を挟まないことがある (`[a, b]`)。
    const isItem = ch.name === "Item";
    const isFlowElement = seq.name === "FlowSequence" && ch.name !== "," && ch.name !== "[" && ch.name !== "]";
    if (!isItem && !isFlowElement) continue;
    if (n === i) return ch.name === "Item" ? (ch.firstChild ?? ch) : ch;
    n++;
  }
  return null;
}

function rootMapping(tree: Tree): SyntaxNode | null {
  const top = tree.topNode.getChild("Document") ?? tree.topNode;
  return asMapping(top.getChild("BlockMapping") ?? top.getChild("FlowMapping") ?? top.firstChild);
}

/**
 * 診断の YAML パス (`triggers[0].effects[1].entity`) を**そのキーの範囲**へ解く。
 *
 * 解けなければ `null` — 位置を偽らない規律は v1 から不変で、変わったのは
 * 解ける範囲 (flow style・引用キー・同名キーの入れ子が解けるようになった) と
 * 粒度 (行全体 → キーそのもの)。
 */
export function rangeOfPath(text: string, path: string): { from: number; to: number } | null {
  const segs = path.split(".").filter((s) => s.length > 0);
  if (!segs.length) return null;
  const tree = parser.parse(text);
  let map = rootMapping(tree);
  let hit: SyntaxNode | null = null;

  for (let i = 0; i < segs.length; i++) {
    if (!map) return null;
    const { key, indices } = splitSeg(segs[i]);
    const pair = findPair(text, map, key);
    if (!pair) return null;
    hit = pair.getChild("Key");
    let value = valueOf(pair);
    for (const idx of indices) value = nthItem(value, idx);
    if (i < segs.length - 1) map = asMapping(value);
  }
  return hit ? { from: hit.from, to: hit.to } : null;
}

// --- 宣言済み id の収穫 (バッファから) ---------------------------------------------------

/** どの容器キーの下に、どの形で id が並ぶか。カテゴリ名は backend の `ids` と揃える。 */
export interface IdHarvest {
  /** 容器キー → カテゴリ。値が mapping で、その**キー**が id (`locations:` 等)。 */
  mapKeys: Record<string, string>;
  /** 容器キー → カテゴリ。値が列で、その**要素**が id (`allowed_flags:` 等)。 */
  listItems: Record<string, string>;
  /** 容器キー → カテゴリ。値が列で、要素 mapping の `id:` が id (`goals:` 等)。 */
  itemIds: Record<string, string>;
}

/**
 * 編集中のバッファから宣言済み id を集める。
 *
 * v1 は素朴な行走査で「容器の直下だけ」を見ており、深い入れ子・flow style を拾えなかった。
 * ここは木を歩くので位置に依らない — 保存前でも、どこに書いてあっても拾う。
 */
export function harvestIds(text: string, h: IdHarvest): Record<string, Set<string>> {
  const out: Record<string, Set<string>> = {};
  const add = (cat: string, id: string) => {
    const t = id.trim();
    if (!t) return;
    (out[cat] ??= new Set()).add(t);
  };
  const scalar = (n: SyntaxNode | null): string | null =>
    n && (n.name === "Literal" || n.name === "QuotedLiteral") ? unquote(text.slice(n.from, n.to)) : null;

  const visit = (node: SyntaxNode) => {
    for (let ch = node.firstChild; ch; ch = ch.nextSibling) {
      if (ch.name === "Pair") {
        const key = keyText(text, ch);
        const value = valueOf(ch);
        if (key && value) {
          const asMap = asMapping(value);
          if (h.mapKeys[key] && asMap) {
            for (let p = asMap.firstChild; p; p = p.nextSibling) {
              if (p.name !== "Pair") continue;
              const name = keyText(text, p);
              if (name) add(h.mapKeys[key], name);
            }
          }
          if (h.listItems[key] && (value.name === "BlockSequence" || value.name === "FlowSequence")) {
            for (let it = value.firstChild; it; it = it.nextSibling) {
              const inner = it.name === "Item" ? it.firstChild : it;
              const v = scalar(inner);
              if (v) add(h.listItems[key], v);
            }
          }
          if (h.itemIds[key] && (value.name === "BlockSequence" || value.name === "FlowSequence")) {
            for (let it = value.firstChild; it; it = it.nextSibling) {
              const m = asMapping(it.name === "Item" ? it.firstChild : it);
              if (!m) continue;
              const idPair = findPair(text, m, "id");
              const v = idPair ? scalar(valueOf(idPair)) : null;
              if (v) add(h.itemIds[key], v);
            }
          }
        }
      }
      visit(ch);
    }
  };
  visit(parser.parse(text).topNode);
  return out;
}
