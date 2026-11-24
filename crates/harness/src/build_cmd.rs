//! spec 38 Phase B: `play build` — AI でパッケージを組む (CLI)。
//!
//! ```text
//! play build <パッケージのフォルダ> <依頼…> [--plan-only] [--write] [--report <path.json>]
//! ```
//!
//! GUI と同じ harness の部品 ([`harness::package_build`]) をそのまま回す: 計画 → 一時フォルダで
//! 1 ファイルずつ → パッケージ全体の検査と修復 → (`--write` のときだけ) ディスクへ適用。
//! 既定は**ディスクに書かない** (`play edit` と同じ)。`--plan-only` は計画だけ出して止まる。
//! `--report` は測定用の JSON (計画・却下理由・ファイルごとの結果・利用量) を書く。
//! モデルは `EDITOR_LLM_*` > `LLM_*` (spec 38 決定 14)。
//!
//! 終了コード: 0 = 通った / 1 = 計画が立たない・書けなかった・止まった・エラーが残った / 2 = 使い方。

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use harness::edit_tools;
use harness::package_build::{self as pb, ApplyReport, BuildOutcome, Planned};
use llm_client::{LlmClient, LlmConfig, LlmLedger};
use serde::Serialize;

pub(crate) const USAGE: &str =
    "使い方: play build <パッケージのフォルダ> <依頼…> [--plan-only] [--write] [--report <path.json>]";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BuildArgs {
    pub root: PathBuf,
    pub request: String,
    pub plan_only: bool,
    pub write: bool,
    pub report: Option<PathBuf>,
}

/// 引数を読む (純関数)。フラグはどこに置いてもよい。**知らない `--…` は拒否する** — 打ち間違えた
/// フラグ (`--writ`) が依頼文の一部としてモデルへ送られ、しかも書き込まれないまま終わるのを防ぐ。
pub(crate) fn parse_args(args: &[String]) -> Result<BuildArgs, String> {
    let (mut plan_only, mut write, mut report) = (false, false, None);
    let mut pos: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--plan-only" => plan_only = true,
            "--write" => write = true,
            "--report" => {
                let v = args.get(i + 1).ok_or("--report の後に書き出す JSON のパスを指定してください")?;
                report = Some(PathBuf::from(v));
                i += 1;
            }
            a if a.starts_with("--") => return Err(format!("知らないオプションです: {a}")),
            a => pos.push(a),
        }
        i += 1;
    }
    if plan_only && write {
        return Err("--plan-only と --write は同時に指定できません (計画だけでは書くものがありません)".into());
    }
    if pos.len() < 2 {
        return Err("パッケージのフォルダと依頼を指定してください".into());
    }
    let request = pos[1..].join(" ");
    if request.trim().is_empty() {
        return Err("依頼が空です".into());
    }
    Ok(BuildArgs { root: PathBuf::from(pos[0]), request, plan_only, write, report })
}

/// 組み立ての集計 (測定と終了コードの素)。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Tally {
    pub files: usize,
    pub changed: usize,
    pub iterations: u32,
    /// 層 1 (ファイル単体の診断) の件数。
    pub l1_errors: usize,
    pub l1_warnings: usize,
    /// 層 2 (この組み立てが生み、修復しても残った指摘) の件数。
    pub l2_errors: usize,
    pub l2_warnings: usize,
    pub repaired: usize,
    /// 打ち切り (上限・反復など) で終わったファイル。
    pub stopped_files: usize,
    /// 新規なのに雛形のまま終わったファイル (やり直しても埋まらなかった)。
    pub template_only: usize,
    pub failed: usize,
    pub not_run: usize,
}

pub(crate) fn tally(o: &BuildOutcome) -> Tally {
    let mut t = Tally {
        files: o.files.len(),
        iterations: o.iterations,
        failed: o.failed.len(),
        not_run: o.not_run.len(),
        ..Default::default()
    };
    for f in &o.files {
        t.changed += f.changed as usize;
        t.repaired += f.repaired as usize;
        t.stopped_files += f.stopped.is_some() as usize;
        t.template_only += f.template_only as usize;
        for d in &f.diagnostics {
            match d.severity.as_str() {
                "error" => t.l1_errors += 1,
                _ => t.l1_warnings += 1,
            }
        }
    }
    for i in &o.package_issues {
        match i.severity.as_str() {
            "error" => t.l2_errors += 1,
            _ => t.l2_warnings += 1,
        }
    }
    t
}

/// 終了コード (純関数)。`play edit` と同じ線: エラーが残った・止まった・書けなかったら 1。
/// 警告だけなら 0 (`play lint` と同じ — 警告は報告するが失敗にはしない)。
pub(crate) fn exit_code(o: &BuildOutcome) -> i32 {
    let t = tally(o);
    let bad = o.stopped.is_some()
        || t.stopped_files > 0
        || t.template_only > 0
        || t.failed > 0
        || t.not_run > 0
        || t.l1_errors > 0
        || t.l2_errors > 0;
    i32::from(bad)
}

/// 計画の表示 (純関数)。
pub(crate) fn format_plan(p: &Planned) -> String {
    let mut s = format!("計画 ({} 件):\n", p.items.len());
    if !p.rejected.is_empty() {
        s.push_str("  (最初の計画は受け付けられず、立て直しました。理由:)\n");
        for r in &p.rejected {
            s.push_str(&format!("    - {r}\n"));
        }
    }
    for (i, it) in p.items.iter().enumerate() {
        let label = match it.action {
            pb::PlanAction::Create => "新規",
            pb::PlanAction::Edit => "変更",
        };
        s.push_str(&format!("  {}. [{label}] {} — {}\n", i + 1, it.path, it.instruction.replace('\n', " ")));
    }
    s
}

#[derive(Serialize)]
struct Report<'a> {
    base_url: &'a str,
    model: &'a str,
    package: String,
    request: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan: Option<&'a Planned>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<&'a BuildOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tally: Option<Tally>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied: Option<&'a ApplyReport>,
    /// 計画の呼び出しも含む全リクエストの利用量。
    ledger: LlmLedger,
    elapsed_secs: f64,
}

fn write_report(path: Option<&Path>, r: &Report<'_>) {
    let Some(path) = path else { return };
    let res = serde_json::to_string_pretty(r)
        .map_err(|e| e.to_string())
        .and_then(|s| std::fs::write(path, s).map_err(|e| e.to_string()));
    match res {
        Ok(()) => eprintln!("[組み立て] 測定の記録を書きました: {}", path.display()),
        Err(e) => eprintln!("[警告] 測定の記録を書けませんでした ({}): {e}", path.display()),
    }
}

/// 一時フォルダ (パッケージの外 = OS の一時フォルダ。実行ごとに別の名前)。
fn staging_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("lorekeel_play_build_{}_{nanos}", std::process::id()))
}

pub(crate) async fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let a = match parse_args(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    if !a.root.join("package.yaml").is_file() {
        eprintln!("`{}` に package.yaml がありません (パッケージのフォルダを指定してください)\n{USAGE}", a.root.display());
        std::process::exit(2);
    }
    let base = LlmConfig::from_env()?;
    let config = LlmConfig::editor_from_env(&base)?.unwrap_or(base);
    let (base_url, model) = (config.base_url.clone(), config.model.clone());
    eprintln!("[組み立て] {base_url} / model={model} / パッケージ={}", a.root.display());
    let client = LlmClient::new(config)?.with_role("editor");
    let staging = pb::Staging::prepare(&a.root, &staging_dir())?;
    let result = body(&a, &client, &staging, &base_url, &model).await;
    staging.discard(); // 適用したかどうかに関わらず一時フォルダは消す (中身は変更セットに写してある)
    let code = result?;
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

async fn body(
    a: &BuildArgs,
    client: &LlmClient,
    staging: &pb::Staging,
    base_url: &str,
    model: &str,
) -> Result<i32, Box<dyn Error>> {
    let t0 = Instant::now();
    let mut report = Report {
        base_url,
        model,
        package: a.root.display().to_string(),
        request: &a.request,
        plan: None,
        plan_error: None,
        outcome: None,
        tally: None,
        applied: None,
        ledger: LlmLedger::default(),
        elapsed_secs: 0.0,
    };
    let existing = staging.existing();
    let ids = edit_tools::ids_table(&staging.vocabulary());
    eprintln!("  > 計画を立てています…");
    let planned = match pb::make_plan(client, &a.request, &existing, &ids).await {
        Ok(p) => p,
        Err(e) => {
            println!("{e}");
            report.plan_error = Some(e.to_string());
            report.ledger = client.usage_ledger();
            report.elapsed_secs = t0.elapsed().as_secs_f64();
            write_report(a.report.as_deref(), &report);
            return Ok(1);
        }
    };
    print!("{}", format_plan(&planned));
    report.plan = Some(&planned);
    if a.plan_only {
        report.ledger = client.usage_ledger();
        report.elapsed_secs = t0.elapsed().as_secs_f64();
        write_report(a.report.as_deref(), &report);
        println!("(--plan-only: 計画だけ出して止まりました)");
        return Ok(0);
    }

    let cancel = AtomicBool::new(false);
    let outcome = match pb::run_build(client, &a.request, &planned.items, staging, &cancel, &mut |l| {
        eprintln!("  > {l}")
    })
    .await
    {
        Ok(o) => o,
        Err(errs) => {
            // make_plan が検めた計画なので通常は起きない (run_build は作者が書き換えた計画も受けるため検め直す)。
            println!("計画を受け付けられませんでした:\n{}", errs.join("\n"));
            return Ok(1);
        }
    };
    for f in &outcome.files {
        let label = match f.action {
            pb::PlanAction::Create => "新規",
            pb::PlanAction::Edit => "変更",
        };
        let mut head = format!("\n── {} ({label}) {} 周", f.path, f.iterations);
        if f.repaired {
            head.push_str(" / 修復あり");
        }
        if let Some(s) = f.stopped {
            head.push_str(&format!(" / 打ち切り: {s:?}"));
        }
        if f.template_only {
            head.push_str(" / 雛形のまま (埋められませんでした)");
        }
        println!("{head}");
        for l in &f.loops {
            println!(
                "   {:?}: {} 周 / 入力 {} (キャッシュ読み {}) / 出力 {}{}",
                l.kind,
                l.iterations,
                l.prompt_tokens,
                l.cache_read,
                l.completion_tokens,
                l.stopped.map(|s| format!(" / 打ち切り: {s:?}")).unwrap_or_default(),
            );
        }
        if f.changed {
            println!("{}", edit_tools::unified_diff(&f.path, f.before.as_deref().unwrap_or(""), &f.after));
        } else {
            println!("(本文は変わっていません)");
        }
        println!("報告: {}", f.summary.trim());
        for d in &f.diagnostics {
            let at = match (d.line, &d.path) {
                (Some(l), _) => format!(" (行 {l})"),
                (None, Some(p)) => format!(" ({p})"),
                _ => String::new(),
            };
            println!("  [{}]{at} {}", d.severity, d.message);
        }
    }
    for f in &outcome.failed {
        println!("\n書けなかったファイル: {} — {}", f.path, f.message);
    }
    if !outcome.not_run.is_empty() {
        println!("\n止まったので書いていないファイル: {}", outcome.not_run.join(", "));
    }
    if let Some(s) = outcome.stopped {
        println!("組み立ては途中で止まりました: {s:?}");
    }
    if !outcome.package_issues.is_empty() {
        println!("\nパッケージ全体の検査 (この組み立てで生まれ、修復しても残ったもの):");
        for i in &outcome.package_issues {
            println!("  [{}] {} — {}", i.severity, i.file.as_deref().unwrap_or("パッケージ全体"), i.message);
        }
    }
    if outcome.preexisting_issues > 0 {
        println!("(開始時から在った指摘 {} 件には触れていません)", outcome.preexisting_issues);
    }
    let t = tally(&outcome);
    println!(
        "\n集計: ファイル {} (変更 {} / 雛形のまま {}) / {} 周 / 修復 {} / 層1 エラー {} 警告 {} / 層2 エラー {} 警告 {}",
        t.files,
        t.changed,
        t.template_only,
        t.iterations,
        t.repaired,
        t.l1_errors,
        t.l1_warnings,
        t.l2_errors,
        t.l2_warnings
    );
    let ledger = client.usage_ledger();
    eprintln!(
        "[組み立て] {:.1}s / リクエスト {} / 入力 {} tok (キャッシュ {}) / 出力 {} tok",
        t0.elapsed().as_secs_f32(),
        ledger.requests,
        ledger.prompt_tokens,
        ledger.cache_read_tokens,
        ledger.completion_tokens
    );

    let applied = if a.write {
        let selected: Vec<String> = outcome.files.iter().filter(|f| f.changed).map(|f| f.path.clone()).collect();
        // `play edit --write` と同じく fork はしない (CLI は作者がフォルダを直接扱う道具)。
        let rep = pb::apply_build(&a.root, &outcome.files, &selected, false, &[]);
        for w in &rep.written {
            println!("書き込みました: {w}");
        }
        for c in &rep.conflicts {
            println!("書いていません (実行中にディスクの本文が変わった): {c}");
        }
        for e in &rep.errors {
            println!("書けませんでした: {} — {}", e.path, e.message);
        }
        Some(rep)
    } else {
        if t.changed > 0 {
            println!("(ディスクには書いていません。書くなら --write)");
        }
        None
    };
    report.outcome = Some(&outcome);
    report.tally = Some(t);
    report.applied = applied.as_ref();
    report.ledger = ledger;
    report.elapsed_secs = t0.elapsed().as_secs_f64();
    write_report(a.report.as_deref(), &report);
    let conflicts = applied.as_ref().is_some_and(|r| !r.conflicts.is_empty() || !r.errors.is_empty());
    Ok(if conflicts { 1 } else { exit_code(&outcome) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness::edit_assist::{Diag, Stopped};
    use harness::editor_lint::EditorIssue;
    use harness::package_build::{BuiltFile, FailedFile, PlanAction, PlanItem};

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn args_put_flags_anywhere_and_reject_unknown_ones() {
        let a = parse_args(&s(&["pkg", "--write", "キャラを", "2 人", "足して", "--report", "r.json"])).unwrap();
        assert_eq!(
            a,
            BuildArgs {
                root: "pkg".into(),
                request: "キャラを 2 人 足して".into(),
                plan_only: false,
                write: true,
                report: Some("r.json".into()),
            }
        );
        // 打ち間違えたフラグは依頼文に混ぜない (モデルへ送られ、しかも書かれない)。
        assert!(parse_args(&s(&["pkg", "足して", "--writ"])).unwrap_err().contains("--writ"));
        assert!(parse_args(&s(&["pkg", "x", "--plan-only", "--write"])).is_err());
        assert!(parse_args(&s(&["pkg", "x", "--report"])).is_err());
        assert!(parse_args(&s(&["pkg"])).is_err(), "依頼が無い");
        assert!(parse_args(&s(&["pkg", "  "])).is_err(), "依頼が空白だけ");
        assert!(parse_args(&s(&["pkg", "x", "--plan-only"])).unwrap().plan_only);
    }

    fn diag(sev: &str) -> Diag {
        Diag { severity: sev.into(), message: "m".into(), line: None, path: None }
    }

    fn file(path: &str, diags: Vec<Diag>, stopped: Option<Stopped>) -> BuiltFile {
        BuiltFile {
            path: path.into(),
            action: PlanAction::Create,
            before: None,
            after: "x".into(),
            changed: true,
            summary: String::new(),
            diagnostics: diags,
            iterations: 3,
            stopped,
            loops: vec![],
            repaired: false,
            template_only: false,
        }
    }

    #[test]
    fn exit_code_fails_only_on_errors_stops_and_unwritten_files() {
        let ok = BuildOutcome {
            files: vec![file("characters/a.yaml", vec![diag("warning")], None)],
            package_issues: vec![EditorIssue { file: None, severity: "warning".into(), message: "w".into() }],
            iterations: 3,
            ..Default::default()
        };
        let t = tally(&ok);
        assert_eq!((t.files, t.changed, t.l1_warnings, t.l2_warnings, t.iterations), (1, 1, 1, 1, 3));
        assert_eq!(exit_code(&ok), 0, "警告だけなら 0 (play lint と同じ)");

        let l1 = BuildOutcome { files: vec![file("characters/a.yaml", vec![diag("error")], None)], ..Default::default() };
        assert_eq!(exit_code(&l1), 1);
        let l2 = BuildOutcome {
            package_issues: vec![EditorIssue { file: None, severity: "error".into(), message: "e".into() }],
            ..ok.clone()
        };
        assert_eq!(exit_code(&l2), 1);
        let limit = BuildOutcome { files: vec![file("a", vec![], Some(Stopped::Limit))], ..Default::default() };
        assert_eq!(exit_code(&limit), 1, "打ち切りで終わったファイルがあれば 1");
        let mut tpl = file("characters/b.yaml", vec![], None);
        tpl.template_only = true;
        let tpl = BuildOutcome { files: vec![tpl], ..Default::default() };
        assert_eq!(tally(&tpl).template_only, 1);
        assert_eq!(exit_code(&tpl), 1, "新規が雛形のまま (changed でも書けていない) なら 1");
        let failed = BuildOutcome {
            failed: vec![FailedFile { path: "a".into(), message: "m".into() }],
            ..Default::default()
        };
        assert_eq!(exit_code(&failed), 1);
        let not_run = BuildOutcome { not_run: vec!["b".into()], stopped: Some(Stopped::Cancel), ..Default::default() };
        assert_eq!(exit_code(&not_run), 1);
    }

    #[test]
    fn plan_listing_says_when_the_first_plan_was_rejected() {
        let items = vec![PlanItem { path: "characters/a.yaml".into(), action: PlanAction::Create, instruction: "アリス\nを作る".into() }];
        let first = format_plan(&Planned { items: items.clone(), rejected: vec![] });
        assert!(first.contains("1. [新規] characters/a.yaml — アリス を作る") && !first.contains("立て直し"), "{first}");
        let retried = format_plan(&Planned { items, rejected: vec!["1 行目: 理由".into()] });
        assert!(retried.contains("立て直しました") && retried.contains("1 行目: 理由"), "{retried}");
    }
}
