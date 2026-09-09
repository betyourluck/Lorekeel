/**
 * マップの見せ方 (spec 15 rev2、2026-09-09 ユーザー決定で有向グラフ → リストへ)。
 *
 * 「現在地と、そこから行ける場所」だけを見る形に変えた — 元の要望 (「どこへ行ける?」を GM に
 * 尋ねる往復を減らす) に対して、**答えは常に現在地の隣接だけ**で、全体図はその問いに答えて
 * いなかった (ユーザーの言「マップにしてと言ったのは僕だったが、やはりリストのほうが見やすい」)。
 *
 * ここは DTO からリストを導く**純粋関数**。DOM に依らないので提示層でも機械が検算できる
 * (`tour.ts` / `editorPath.ts` と同じ枠)。backend の `map_view` は無改修で、霧の範囲も不変。
 */
import type { MapNode, MapView } from "./types/api";

/** 現在地から出る 1 本の出口。 */
export interface MapExit {
  /** 行き先のノード (`visited=false` なら未踏 = 名前は出るが中身は伏せてある)。 */
  node: MapNode;
  /** gate 未達 = いまは通れない (🔒)。 */
  locked: boolean;
}

/** リスト表示の素材。 */
export interface MapList {
  /** 現在地 (盤面が始まっていなければ null)。 */
  current: MapNode | null;
  /** 現在地から出る出口。**作者が書いた `exits` の順**を保つ。 */
  exits: MapExit[];
}

/**
 * `MapView` から「現在地 + そこから行ける場所」を導く。
 *
 * - 出口は**現在地から出る辺だけ**。他の訪問済みノードの辺は見ない (奥は畳む)。
 * - **同じ行き先へ複数の出口があれば 1 行に畳み、どれか 1 本でも通れるなら通れる扱い**
 *   (別々の gate が付いた二つの道は、片方が開いていれば行ける)。
 * - ノードが解決できない行き先は落とす (幻の行を作らない)。
 */
export function mapList(view: MapView): MapList {
  const current = view.nodes.find((n) => n.current) ?? null;
  if (!current) return { current: null, exits: [] };
  const byId = new Map(view.nodes.map((n) => [n.id, n]));
  const exits: MapExit[] = [];
  const seen = new Map<string, MapExit>();
  for (const e of view.edges) {
    if (e.from !== current.id) continue;
    const hit = seen.get(e.to);
    if (hit) {
      // 二本目以降は畳む。通れる道が一本でもあれば通れる。
      hit.locked = hit.locked && e.locked;
      continue;
    }
    const node = byId.get(e.to);
    if (!node) continue;
    const exit: MapExit = { node, locked: e.locked };
    seen.set(e.to, exit);
    exits.push(exit);
  }
  return { current, exits };
}
