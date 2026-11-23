"""spec 38 Phase B: run.ps1 の出力フォルダ (play build --report の JSON) を集計する。

使い方: python -I measure/spec38_phase_b/report.py <出力フォルダ> [<出力フォルダ> ...]

測るもの (spec 38 Phase B): 計画の却下 (最初の計画が受け付けられたか) / ファイルごとの周回 /
層 1 の診断 (未知キーなど) / 層 2 の残り (死んだ参照など) / トークン。本文は表示しない
(差分は stdout.txt にある)。
"""
import json
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8")


def show(path: Path) -> None:
    r = json.loads(path.read_text(encoding="utf-8"))
    led = r.get("ledger", {})
    print(f"== {path.parent.name}/{path.stem}  model={r.get('model')}  {r.get('elapsed_secs', 0):.0f}s")
    if r.get("plan_error"):
        print(f"  計画が立たなかった: {r['plan_error'][:300]}")
    plan = r.get("plan") or {}
    items = plan.get("items", [])
    rej = plan.get("rejected", [])
    print(f"  計画 {len(items)} 件 / 最初の計画は {'却下 (' + str(len(rej)) + ' 件の理由)' if rej else '一発で通った'}")
    for x in rej:
        print(f"    却下: {x}")
    o = r.get("outcome")
    if o:
        for f in o.get("files", []):
            diags = f.get("diagnostics", [])
            e = sum(1 for d in diags if d.get("severity") == "error")
            w = len(diags) - e
            flags = []
            if f.get("repaired"):
                flags.append("修復")
            if f.get("stopped"):
                flags.append(f"打ち切り={f['stopped']}")
            if f.get("template_only"):
                flags.append("雛形のまま")
            if not f.get("changed"):
                flags.append("変化なし")
            print(f"  - {f['path']:<28} {f['action']:<6} {f['iterations']:>3} 周  層1 E{e} W{w}  {' '.join(flags)}")
            for d in diags:
                print(f"      [{d['severity']}] {d['message'][:160]}")
        for f in o.get("failed", []):
            print(f"  ✗ {f['path']}: {f['message'][:200]}")
        if o.get("not_run"):
            print(f"  未実行: {', '.join(o['not_run'])}")
        for i in o.get("package_issues", []):
            print(f"  層2 [{i['severity']}] {i.get('file') or 'パッケージ全体'}: {i['message'][:200]}")
        print(f"  開始時から在った指摘: {o.get('preexisting_issues', 0)} 件")
    t = r.get("tally")
    if t:
        print(
            f"  集計: ファイル {t['files']} (変更 {t['changed']} / 雛形のまま {t.get('template_only', 0)}) / "
            f"{t['iterations']} 周 / 修復 {t['repaired']} / "
            f"層1 E{t['l1_errors']} W{t['l1_warnings']} / 層2 E{t['l2_errors']} W{t['l2_warnings']}"
        )
    pt, cr = led.get("prompt_tokens", 0), led.get("cache_read_tokens", 0)
    ratio = f"{cr / pt:.0%}" if pt else "-"
    print(
        f"  利用量: リクエスト {led.get('requests', 0)} / 入力 {pt:,} (キャッシュ {cr:,} = {ratio}) / "
        f"出力 {led.get('completion_tokens', 0):,}"
    )
    print()


def main() -> None:
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    for d in sys.argv[1:]:
        p = Path(d)
        files = sorted(p.glob("*.json")) if p.is_dir() else [p]
        for f in files:
            show(f)


if __name__ == "__main__":
    main()
