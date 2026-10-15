// 語りの段 (spec 35) の提示層での整え。
//
// **モブの台詞は地の文に含める** (2026-10-15 ユーザー要望)。登場人物 (scenario の characters) に
// いない人物 = `speaker` が null で `name` だけを持つ段 (店員・通行人など) は、吹き出しで描かず
// 読み上げでも台詞として扱わない。`名前「…」` の形の地の文へ畳む — backend が平らな語りを作る
// 形 (`plain`) と同じ書き方なので、段の無い語りと見た目が揃う。
//
// 畳むのは提示層だけ — 正本・GM への還流・セーブは段のまま (backend は触らない)。

/** 段の共通形 (TurnView.segments / 会話ログの段 / 読み上げの段)。 */
export interface SegmentLike {
  speaker: string | null;
  name: string;
  text: string;
}

/** モブの台詞か (名前はあるが登場人物ではない)。 */
export function isMobLine(seg: SegmentLike): boolean {
  return seg.speaker === null && seg.name.trim() !== "";
}

/**
 * モブの台詞を地の文へ畳む (純関数)。登場人物の台詞と地の文はそのまま。
 * 隣り合う段はつなげない — 読み上げのスキップが段単位なので、つなげると 1 回のスキップで
 * 飛ぶ範囲が広がる。
 */
export function foldMobLines<T extends SegmentLike>(segments: T[]): T[] {
  return segments.map((seg) =>
    isMobLine(seg) ? { ...seg, name: "", text: `${seg.name.trim()}「${seg.text}」` } : seg,
  );
}
