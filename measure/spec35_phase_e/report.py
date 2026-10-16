"""spec 35 Phase E (E-2/E-3): run.ps1 の出力フォルダを集計し、手で分類するための読み物を書く。

使い方:
    python -I measure/spec35_phase_e/report.py <出力フォルダ> [<出力フォルダ> ...]

- 標準出力: フォルダ (= モデル) ごと・パッケージごとの表 (指標の定義は specs/35 の Phase E の節)
- 各フォルダに <package>.read.txt: jsonl と CLI の標準出力をターンごとに並べたもの (③⑤⑥と副は手で読む)

本文を外へ送る処理は無い (読むのはローカルのファイルだけ)。
"""
import json
import sys
from collections import Counter
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8")

SPEAKER_CODES = {"speaker_controlled", "speaker_not_present", "ambiguous_speaker", "unknown_speaker"}

# 却下理由 → その原因になった op (却下の後の再提出で「却下されていない op まで消えたか」を見るため)。
REASON_OPS = {
    "no_exit": {"move"},
    "move_gate_unmet": {"move"},
    "vote_not_declared": {"cast_vote"},
    "vote_not_allowed": {"cast_vote"},
    "flag_not_allowed": {"set_flag"},
    "flag_gate_unmet": {"set_flag"},
    "unknown_stat": {"adjust_stat", "scale_stat", "check", "check_under"},
    "item_not_here": {"add_item"},
    "item_not_held": {"remove_item", "give_item"},
    "item_fixed": {"add_item"},
    "item_already_taken": {"add_item"},
    "challenge_locked": {"attempt_challenge"},
    "unknown_challenge": {"attempt_challenge"},
}

# 登録モデルの単価 (USD / 100 万トークン、入力・出力)。キャッシュ読みは jsonl に無いので**上限**の見積もり。
PRICES = {
    "muse-spark-1.3-contributor": (0.1, 0.2),
    "gpt-6-luna": (0.1, 0.5),
    "claude-opus-5-5": (4.0, 20.0),
    "gemini-4-argon": (1.5, 9.0),
}


def load(path):
    return [json.loads(l) for l in path.read_text(encoding="utf-8").splitlines() if l.strip()]


def is_spec35_retry(rt):
    if rt["kind"] == "rejected":
        return any(c["code"] in SPEAKER_CODES for c in rt["reasons"])
    d = rt.get("detail", "")
    return any(w in d for w in ("narration", "speaker", "segment"))


def dropped_after_rejection(rec):
    """却下された提案と次の提案を比べ、却下の原因でない op と台詞の段が消えた件を返す。"""
    out = []
    props = rec["proposals"]
    for i, rt in enumerate(rec["retries"]):
        if rt["kind"] != "rejected" or i + 1 >= len(props):
            continue
        a, b = props[i], props[i + 1]
        if "ops" not in a or "ops" not in b:
            continue
        codes = {c["code"] for c in rt["reasons"]}
        implicated = set().union(*(REASON_OPS.get(c, set()) for c in codes)) if codes else set()
        gone = (set(a["ops"]) - set(b["ops"])) - implicated
        if gone:
            out.append(f"op {sorted(gone)} (却下理由 {sorted(codes)})")
        if a.get("speakers") and not b.get("speakers") and not (codes & SPEAKER_CODES):
            out.append(f"台詞の段 {len(a['speakers'])} → 0 (却下理由 {sorted(codes)})")
    return out


def summarize(recs):
    ok = [r for r in recs if r["accepted"]]
    nat = [r for r in ok if not r["tag"]]
    forms = Counter(p["form"] for r in recs for p in r["proposals"])
    spk_turns = [r for r in recs if any(
        rt["kind"] == "rejected" and any(c["code"] in SPEAKER_CODES for c in rt["reasons"]) for rt in r["retries"])]
    spk_codes = Counter(c["code"] for r in recs for rt in r["retries"] if rt["kind"] == "rejected"
                        for c in rt["reasons"] if c["code"] in SPEAKER_CODES)
    return {
        "turns": len(recs),
        "accepted": len(ok),
        "failed": len(recs) - len(ok),
        "forms": dict(forms),
        "spk_turns": len(spk_turns),
        "spk_turns_natural": sum(1 for r in spk_turns if not r["tag"]),
        "spk_codes": dict(spk_codes),
        "natural_accepted": len(nat),
        "spec35_retries": sum(1 for r in recs for rt in r["retries"] if is_spec35_retry(rt)),
        "all_retries": sum(len(r["retries"]) for r in recs),
        "leaks": sum(len(r.get("format_leaks", [])) for r in recs),
        "dlg": sum(r["segs"]["dlg"] for r in ok),
        "extra": sum(r["segs"]["extra"] for r in ok),
        "narr": sum(r["segs"]["narr"] for r in ok),
        "dlg_turns": sum(1 for r in ok if r["segs"]["dlg"] or r["segs"]["extra"]),
        "dropped": sum(len(dropped_after_rejection(r)) for r in recs),
        "requests": sum(r["requests"] for r in recs),
        "prompt": sum(r["prompt_tokens"] for r in recs),
        "completion": sum(r["completion_tokens"] for r in recs),
        "model": next((r["model"] for r in recs), "?"),
    }


def cost(s):
    p = PRICES.get(s["model"])
    if not p:
        return "-"
    return f"≤${(s['prompt'] * p[0] + s['completion'] * p[1]) / 1e6:.2f}"


def line(label, s):
    f = s["forms"]
    seg = f.get("segments", 0)
    total = sum(f.values()) - f.get("error", 0)
    return (f"{label:<28} 受理 {s['accepted']:>2}/{s['turns']:<2} | ① 配列 {seg}/{total}"
            f" raw {f.get('raw', 0)} 壊れ {f.get('malformed', 0)}"
            f" | ② 話者却下 {s['spk_turns']} (自然 {s['spk_turns_natural']}/{s['natural_accepted']}) {s['spk_codes'] or ''}"
            f" | ④ {s['spec35_retries']}/{s['accepted']} (全やり直し {s['all_retries']})"
            f" | ⑦ {s['leaks']} | 台詞 {s['dlg']} モブ {s['extra']} (台詞のあるターン {s['dlg_turns']})"
            f" | 却下後に消えた {s['dropped']}"
            f" | 要求 {s['requests']} 入力 {s['prompt']:,} 出力 {s['completion']:,} {cost(s)}")


def write_reading(folder, name, recs):
    out = folder / f"{name}.stdout.txt"
    script = folder / f"{name}.script.txt"
    if not script.exists():
        script = Path(__file__).parent / "scripts" / f"{name}.txt"
    actions = [l for l in script.read_text(encoding="utf-8").splitlines() if l.strip()]
    blocks = out.read_text(encoding="utf-8").split("\n> ")[1:] if out.exists() else []
    lines = [f"# {folder.name} / {name} — ③⑤⑥と副は手で分類する (specs/35 の Phase E の節)", ""]
    for i, r in enumerate(recs):
        props = [(p["form"], p.get("speakers"), p.get("ops")) for p in r["proposals"]]
        lines.append(f"=== T{r['index']} [{r['tag'] or '-'}] 受理={r['accepted']} 試行={r['attempts']} "
                     f"移動={r['moved']} 段={r['segs']}")
        lines.append(f"  行動: {actions[i] if i < len(actions) else '?'}")
        lines.append(f"  提案: {props}")
        for rt in r["retries"]:
            lines.append("  やり直し: " + json.dumps(rt, ensure_ascii=False)[:400])
        for d in dropped_after_rejection(r):
            lines.append("  ⚠ 却下後に消えた: " + d)
        for lk in r.get("format_leaks", []):
            lines.append("  ⑦ 書式の漏れ: " + lk)
        if r["error"]:
            lines.append("  失敗: " + r["error"][:200])
        blk = blocks[i] if i < len(blocks) else ""
        for ln in blk.splitlines():
            ln = ln.rstrip()
            if ln and not ln.startswith("  [所在"):
                lines.append("  | " + ln)
        lines.append("  分類: ③[ ] ⑤[ ] ⑥[ ] 副[ ] メモ:")
        lines.append("")
    (folder / f"{name}.read.txt").write_text("\n".join(lines), encoding="utf-8")


def main(folders):
    for f in folders:
        folder = Path(f)
        runs = sorted(folder.glob("*.jsonl"))
        if not runs:
            print(f"{folder}: jsonl がありません")
            continue
        print(f"## {folder.name}")
        every = []
        for j in runs:
            recs = load(j)
            every += recs
            print(line(j.stem, summarize(recs)))
            write_reading(folder, j.stem, recs)
        if len(runs) > 1:
            print(line("合計", summarize(every)))
        print(f"  (読み物: {folder}\\<package>.read.txt)")
        print()


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    main(sys.argv[1:])
