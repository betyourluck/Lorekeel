//! spec 38 Phase C: AI でパッケージを組む — app の持ち場。
//!
//! 計画・組み立て・適用の中身は harness ([`harness::package_build`]、CLI `play build` と同じ実体)。
//! ここが持つのは app にしか無い 2 つだけ:
//!
//! - **一時フォルダの置き場** — `app_data/build_staging/run_{nanos}`。実行ごとに作り、適用か破棄で
//!   消す。強制終了で残ったものは次の実行の前に片付ける ([`clean_leftovers`])。
//! - **実行から適用までのジョブ** ([`BuildJob`]) — 組み立ては command 1 回で終わるが、適用は作者が
//!   変更セットを見てから別の command で行う。その間、一時フォルダと結果をここで持つ。
//!   **適用先は組み立てた時の編集ルート**で、適用の時点の編集ルートと食い違えば書かない
//!   (別のパッケージへ変更セットを書く経路を作らない)。

use std::path::{Path, PathBuf};

use harness::edit_assist::{Diag, Stopped};
use harness::edit_tools;
use harness::editor_lint::EditorIssue;
use harness::package_build::{
    self as pb, ApplyReport, BuildOutcome, BuiltFile, FailedFile, LoopUsage, PlanAction, PlanItem,
};
use llm_client::LlmLedger;
use serde::Serialize;

/// 一時フォルダを置く場所 (`app_data` の直下)。
pub fn staging_base(app_data: &Path) -> PathBuf {
    app_data.join("build_staging")
}

/// 実行 1 回分の一時フォルダ (まだ作らない — 作るのは [`pb::Staging::prepare`])。
pub fn new_staging_dir(base: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    base.join(format!("run_{nanos}"))
}

/// 前回までの一時フォルダ (強制終了で残ったもの) を片付ける。消すのは `base` の**中の**
/// `run_*` だけで、`base` と編集ルートが重なるときは何もしない (パッケージを消さない)。
/// 片付けの失敗は組み立てを止めない (残っても次の機会に消える)。
pub fn clean_leftovers(base: &Path, root: &Path) {
    let (Ok(base_c), Ok(root_c)) = (base.canonicalize(), root.canonicalize()) else {
        return; // base がまだ無い = 片付けるものが無い
    };
    if root_c.starts_with(&base_c) || base_c.starts_with(&root_c) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(&base_c) else { return };
    for e in rd.flatten() {
        let is_run = e.file_name().to_str().is_some_and(|n| n.starts_with("run_"));
        if is_run && e.path().is_dir() {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// 組み立てから適用までのジョブ。
pub struct BuildJob {
    /// 組み立てた時の編集ルート (= 適用先)。
    root: PathBuf,
    staging: pb::Staging,
    files: Vec<BuiltFile>,
}

impl BuildJob {
    pub fn new(root: PathBuf, staging: pb::Staging, files: Vec<BuiltFile>) -> Self {
        Self { root, staging, files }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 今の編集ルートが組み立てた時と同じか。違えば適用しない理由を返す。
    pub fn check_root(&self, current: Option<&Path>) -> Result<(), String> {
        match current {
            None => Err("編集モードではありません".into()),
            Some(c) if c == self.root => Ok(()),
            Some(_) => Err("組み立てた時と別のパッケージを編集しています。変更セットは適用できません".into()),
        }
    }

    /// 選んだファイルを書き、一時フォルダを消す (ジョブはここで終わる)。
    pub fn apply(self, selected: &[String], fork: bool, meta_files: &[&str]) -> ApplyReport {
        let rep = pb::apply_build(&self.root, &self.files, selected, fork, meta_files);
        self.staging.discard();
        rep
    }

    /// 適用せずに捨てる。
    pub fn discard(self) {
        self.staging.discard();
    }
}

/// 変更セットの 1 行 (画面用)。本文は LF。`diff` は開始時の本文 (新規は空) との差分。
#[derive(Debug, Serialize)]
pub struct BuiltFileView {
    pub path: String,
    pub action: PlanAction,
    pub changed: bool,
    pub template_only: bool,
    pub repaired: bool,
    pub iterations: u32,
    pub summary: String,
    pub diagnostics: Vec<Diag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
    pub diff: String,
    /// 組み上がった本文 (適用前にエディタで見るため)。
    pub after: String,
}

/// 組み立ての返り (画面用)。
#[derive(Debug, Serialize)]
pub struct BuildView {
    pub files: Vec<BuiltFileView>,
    pub failed: Vec<FailedFile>,
    pub not_run: Vec<String>,
    pub package_issues: Vec<EditorIssue>,
    pub preexisting_issues: usize,
    pub iterations: u32,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_read: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
    /// 使ったモデル (`EDITOR_LLM_*` か GM の設定か)。
    pub model: String,
}

pub fn view_of(o: &BuildOutcome, model: &str) -> BuildView {
    BuildView {
        files: o
            .files
            .iter()
            .map(|f| BuiltFileView {
                path: f.path.clone(),
                action: f.action,
                changed: f.changed,
                template_only: f.template_only,
                repaired: f.repaired,
                iterations: f.iterations,
                summary: f.summary.clone(),
                diagnostics: f.diagnostics.clone(),
                stopped: f.stopped,
                diff: if f.changed {
                    edit_tools::unified_diff(&f.path, f.before.as_deref().unwrap_or(""), &f.after)
                } else {
                    String::new()
                },
                after: f.after.clone(),
            })
            .collect(),
        failed: o.failed.clone(),
        not_run: o.not_run.clone(),
        package_issues: o.package_issues.clone(),
        preexisting_issues: o.preexisting_issues,
        iterations: o.iterations,
        prompt_tokens: o.prompt_tokens,
        completion_tokens: o.completion_tokens,
        cache_read: o.cache_read,
        stopped: o.stopped,
        model: model.to_string(),
    }
}

// =============================================================================
// 組み立ての記録 (`app_data/logs/build.jsonl`、2026-11-24)
// =============================================================================

/// 組み立ての記録 1 行。後から「計画は strict で送ったか」「どのファイルのどの回に何トークン
/// 使ったか」を確かめるためのもの — 利用量ログ (`usage.jsonl`) は 1 リクエスト 1 行で、役割
/// `editor` としか分からない (実機 1 回目で入力の約 7 割が 1 本の長いループに集まったのに、
/// どのファイルの回だったか分からなかった)。
///
/// **ファイルの本文 (before / after) と差分は載せない** — 載るのは作者の依頼・計画の指示・
/// モデルの報告・パス・周回とトークンだけ。置き場は `usage.jsonl` と同じ `app_data/logs/`
/// 固定 (会話ログの保存先設定には従わせない = 機械が読む物)。
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuildLog {
    /// `build_plan` 1 回。
    Plan {
        root: String,
        model_id: String,
        request: String,
        items: Vec<PlanItem>,
        /// 最初の計画が受け付けられなかった理由 (立て直したときだけ)。
        rejected: Vec<String>,
        /// 計画を strict で送ったか (= Anthropic で名指しの強制を断られて降格した状態、
        /// [`llm_client::LlmClient::strict_active`])。強制の 400 は利用量ログに残らない。
        strict: bool,
        /// 計画係の client の累計 (立て直しの呼び出しも含む)。
        usage: LlmLedger,
        elapsed_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// `build_run` 1 回。
    Run {
        root: String,
        model_id: String,
        request: String,
        plan: Vec<PlanItem>,
        files: Vec<FileLog>,
        failed: Vec<FailedFile>,
        not_run: Vec<String>,
        /// この組み立てが生んで、修復しても残った層 2 の指摘の件数。
        new_issues: usize,
        preexisting_issues: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        stopped: Option<Stopped>,
        /// ループの返りの合計 (= `files[].loops` の合計)。
        iterations: u32,
        prompt_tokens: u64,
        completion_tokens: u64,
        cache_read: u64,
        /// client の累計 (`LlmClient` の記録点 = 全リクエスト)。ループの合計より多ければ、
        /// 失敗して結果に残らなかった回がある。
        usage: LlmLedger,
        elapsed_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// `build_apply` 1 回。
    Apply {
        root: String,
        written: Vec<String>,
        conflicts: Vec<String>,
        errors: Vec<FailedFile>,
        forked: bool,
    },
}

/// 記録の 1 ファイル (本文は持たない)。
#[derive(Debug, Serialize)]
pub struct FileLog {
    pub path: String,
    pub action: PlanAction,
    pub changed: bool,
    pub template_only: bool,
    pub repaired: bool,
    /// 層 1 の診断の件数。
    pub diagnostics: usize,
    pub summary: String,
    /// 回したループの周回とトークン (最初の回・やり直し・修復)。
    pub loops: Vec<LoopUsage>,
}

/// 記録の共通部 (どの組み立てか)。
pub struct LogHead {
    pub root: String,
    pub model_id: String,
    pub request: String,
}

/// `build_run` の記録を組む (純関数)。`outcome` が無いのは計画の検め直しで落ちたとき。
pub fn run_log(
    head: LogHead,
    plan: &[PlanItem],
    outcome: Option<&BuildOutcome>,
    usage: LlmLedger,
    elapsed_ms: u64,
    error: Option<String>,
) -> BuildLog {
    let empty = BuildOutcome::default();
    let o = outcome.unwrap_or(&empty);
    BuildLog::Run {
        root: head.root,
        model_id: head.model_id,
        request: head.request,
        plan: plan.to_vec(),
        files: o
            .files
            .iter()
            .map(|f| FileLog {
                path: f.path.clone(),
                action: f.action,
                changed: f.changed,
                template_only: f.template_only,
                repaired: f.repaired,
                diagnostics: f.diagnostics.len(),
                summary: f.summary.clone(),
                loops: f.loops.clone(),
            })
            .collect(),
        failed: o.failed.clone(),
        not_run: o.not_run.clone(),
        new_issues: o.package_issues.len(),
        preexisting_issues: o.preexisting_issues,
        stopped: o.stopped,
        iterations: o.iterations,
        prompt_tokens: o.prompt_tokens,
        completion_tokens: o.completion_tokens,
        cache_read: o.cache_read,
        usage,
        elapsed_ms,
        error,
    }
}

/// jsonl の 1 行 (`t_ms` と `kind` が同じ段に並ぶ — `usage.jsonl` と同じ形)。
#[derive(Serialize)]
struct LoggedBuild<'a> {
    t_ms: u64,
    #[serde(flatten)]
    entry: &'a BuildLog,
}

/// 記録を 1 行追記する。失敗は呼び出し側が stderr に出して握り潰す (記録の失敗で組み立てを止めない)。
pub fn append_build_log(path: &Path, entry: &BuildLog) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let t_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut line = serde_json::to_string(&LoggedBuild { t_ms, entry }).map_err(std::io::Error::other)?;
    line.push('\n');
    std::fs::OpenOptions::new().create(true).append(true).open(path)?.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness::edit_assist::ToolChat;
    use harness::package_build::Staging;
    use llm_client::{ChatMessage, ChatTurn, Finish, LlmError, ToolCall, ToolSpec, Usage};
    use serde_json::json;
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lorekeel_buildjob_{name}_{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn package(dir: &Path) {
        std::fs::create_dir_all(dir.join("scenarios")).unwrap();
        std::fs::create_dir_all(dir.join("characters")).unwrap();
        std::fs::write(dir.join("package.yaml"), "title: T\nentry: scenarios/main.yaml\n").unwrap();
        std::fs::write(
            dir.join("scenarios/main.yaml"),
            "title: 部屋\nstart: room\nlocations:\n  room:\n    description: 小さな部屋\ngoal: { kind: flag_is, key: done }\nallowed_flags: [done]\n",
        )
        .unwrap();
    }

    /// 台本どおりに返す偽のモデル。
    struct Scripted(Mutex<Vec<ChatTurn>>);
    impl Scripted {
        fn next(&self) -> Result<ChatTurn, LlmError> {
            let mut q = self.0.lock().unwrap();
            Ok(if q.is_empty() { text("(台本切れ)") } else { q.remove(0) })
        }
    }
    impl ToolChat for Scripted {
        async fn chat(&self, _m: Vec<ChatMessage>, _t: Vec<ToolSpec>) -> Result<ChatTurn, LlmError> {
            self.next()
        }
        async fn wrap_up(&self, _m: Vec<ChatMessage>, _t: Vec<ToolSpec>) -> Result<ChatTurn, LlmError> {
            self.next()
        }
    }
    fn text(t: &str) -> ChatTurn {
        ChatTurn { text: Some(t.into()), tool_calls: vec![], finish: Finish::Stop, usage: usage() }
    }
    fn sd(pattern: &str, replacement: &str, apply: bool) -> ChatTurn {
        let mut args = json!({ "pattern": pattern, "replacement": replacement });
        if apply {
            args["apply"] = json!(true);
        }
        ChatTurn {
            text: None,
            tool_calls: vec![ToolCall { id: "c0".into(), name: "sd".into(), args, thought_signature: None }],
            finish: Finish::ToolUse,
            usage: usage(),
        }
    }
    fn usage() -> Usage {
        Usage { prompt: 100, completion: 7, cache_read: 40, cost_usd: None }
    }

    /// 【spec 38 C】実行 → 変更セット → 適用までのジョブの一生。
    /// - 一時フォルダは `build_staging` の下に作られ、適用でも破棄でも消える
    /// - 画面用の返りに差分と組み上がった本文が載る
    /// - **編集ルートが組み立てた時と違えば適用しない**。拒否してもジョブは残る (正しいパッケージに
    ///   戻れば適用できる)
    /// - 適用は組み立てた時のパッケージへ書く
    #[test]
    fn a_build_job_lives_from_run_to_apply_and_refuses_another_package() {
        let app_data = scratch("appdata");
        let pkg = scratch("pkg");
        package(&pkg);
        let other = scratch("other");
        package(&other);
        let pkg = pkg.canonicalize().unwrap();

        let base = staging_base(&app_data);
        let dir = new_staging_dir(&base);
        assert!(dir.starts_with(&base));
        let staging = Staging::prepare(&pkg, &dir).unwrap();
        let plan = vec![PlanItem {
            path: "scenarios/main.yaml".into(),
            action: PlanAction::Edit,
            instruction: "部屋の説明を書き足す".into(),
        }];
        let chat = Scripted(Mutex::new(vec![
            sd("小さな部屋", "小さな部屋。窓がひとつある", false),
            sd("小さな部屋", "小さな部屋。窓がひとつある", true),
            text("窓を足しました"),
        ]));
        let cancel = AtomicBool::new(false);
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let outcome = rt
            .block_on(pb::run_build(&chat, "窓を足して", &plan, &staging, &cancel, &mut |_| {}))
            .expect("計画は通る");

        let view = view_of(&outcome, "fake-model");
        assert_eq!(view.model, "fake-model");
        let f = &view.files[0];
        assert!(f.changed);
        assert!(f.diff.starts_with("--- a/scenarios/main.yaml"), "差分が載る: {}", f.diff);
        assert!(f.diff.contains("+    description: 小さな部屋。窓がひとつある"), "差分が載る: {}", f.diff);
        assert!(f.after.contains("窓がひとつある"), "組み上がった本文が載る");
        assert!(dir.is_dir(), "適用までは一時フォルダが残る");

        let job = BuildJob::new(pkg.clone(), staging, outcome.files);
        // 別のパッケージ・編集モード外では適用しない。
        assert!(job.check_root(Some(&other.canonicalize().unwrap())).is_err());
        assert!(job.check_root(None).is_err());
        assert!(job.check_root(Some(&pkg)).is_ok());

        let rep = job.apply(&["scenarios/main.yaml".into()], false, &[]);
        assert_eq!(rep.written, ["scenarios/main.yaml"]);
        let disk = std::fs::read_to_string(pkg.join("scenarios/main.yaml")).unwrap();
        assert!(disk.contains("窓がひとつある"), "組み立てた時のパッケージへ書く");
        let untouched = std::fs::read_to_string(other.join("scenarios/main.yaml")).unwrap();
        assert!(!untouched.contains("窓"), "別のパッケージには触れない");
        assert!(!dir.exists(), "適用で一時フォルダが消える");

        // 破棄でも消える。
        let dir2 = new_staging_dir(&base);
        let st2 = Staging::prepare(&pkg, &dir2).unwrap();
        BuildJob::new(pkg.clone(), st2, vec![]).discard();
        assert!(!dir2.exists(), "破棄で一時フォルダが消える");

        for d in [&app_data, &pkg, &other] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// 【組み立ての記録 (2026-11-24)】実機 1 回目で入力の約 7 割が 1 本の長いループに集まったのに、
    /// 利用量ログ (`usage.jsonl`) は役割 `editor` としか言わず、どのファイルの回だったか分からなかった。
    /// - 記録の 1 ファイルにループごとの周回とトークンが載り、合計が組み立ての合計と一致する
    /// - **本文 (before / after) と差分は載らない**
    /// - 追記は 1 行 1 事象で、`t_ms` と `kind` が同じ段に並ぶ
    #[test]
    fn the_build_log_records_tokens_per_file_without_bodies() {
        let pkg = scratch("log_pkg");
        package(&pkg);
        let staging = Staging::prepare(&pkg, &scratch("log_st").join("run")).unwrap();
        let plan = vec![PlanItem {
            path: "scenarios/main.yaml".into(),
            action: PlanAction::Edit,
            instruction: "部屋の説明を書き足す".into(),
        }];
        let chat = Scripted(Mutex::new(vec![
            sd("小さな部屋", "小さな部屋。窓がひとつある", false),
            sd("小さな部屋", "小さな部屋。窓がひとつある", true),
            text("書き足しました"),
        ]));
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let outcome = rt
            .block_on(pb::run_build(&chat, "窓を足して", &plan, &staging, &AtomicBool::new(false), &mut |_| {}))
            .expect("計画は通る");
        assert!(outcome.files[0].after.contains("窓がひとつある"), "前提: 本文は変わった");

        let head = LogHead { root: "pkg".into(), model_id: "fake-model".into(), request: "窓を足して".into() };
        let entry = run_log(head, &plan, Some(&outcome), LlmLedger::default(), 1234, None);
        let v = serde_json::to_value(&entry).unwrap();
        assert_eq!(v["kind"], "run");
        let f = &v["files"][0];
        assert_eq!(f["path"], "scenarios/main.yaml");
        assert_eq!(f["loops"][0]["kind"], "main");
        // 台本の 1 往復は入力 100 / キャッシュ読み 40 (台本切れの往復も同じ値)。
        let n = f["loops"][0]["iterations"].as_u64().unwrap();
        assert!(n >= 3, "台本の 3 往復以上: {f}");
        assert_eq!(f["loops"][0]["prompt_tokens"], n * 100, "往復の数 × 100: {f}");
        assert_eq!(f["loops"][0]["cache_read"], n * 40);
        let loops_total: u64 = f["loops"].as_array().unwrap().iter().map(|l| l["prompt_tokens"].as_u64().unwrap()).sum();
        assert_eq!(v["prompt_tokens"], loops_total, "ループの合計と組み立ての合計が一致する");
        let s = serde_json::to_string(&entry).unwrap();
        assert!(!s.contains("窓がひとつある"), "本文と差分は載らない: {s}");
        assert!(f.get("after").is_none() && f.get("before").is_none() && f.get("diff").is_none());

        let log = scratch("log_dir").join("logs").join("build.jsonl");
        append_build_log(&log, &entry).unwrap();
        append_build_log(&log, &BuildLog::Apply {
            root: "pkg".into(),
            written: vec!["scenarios/main.yaml".into()],
            conflicts: vec![],
            errors: vec![],
            forked: false,
        })
        .unwrap();
        let text = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2, "1 行 1 事象");
        assert!(lines[0]["t_ms"].as_u64().unwrap() > 0);
        assert_eq!((lines[0]["kind"].as_str(), lines[1]["kind"].as_str()), (Some("run"), Some("apply")));
        staging.discard();
        let _ = std::fs::remove_dir_all(&pkg);
    }

    /// 【spec 38 C】強制終了で残った一時フォルダは片付けるが、`run_*` 以外には触れず、
    /// 置き場と編集ルートが重なるときは何もしない (パッケージを消さない)。
    #[test]
    fn leftovers_are_cleaned_but_never_a_package() {
        let app_data = scratch("left");
        let base = staging_base(&app_data);
        std::fs::create_dir_all(base.join("run_1/scenarios")).unwrap();
        std::fs::create_dir_all(base.join("keep_me")).unwrap();
        let pkg = scratch("leftpkg");
        package(&pkg);

        clean_leftovers(&base, &pkg);
        assert!(!base.join("run_1").exists(), "残った一時フォルダは消える");
        assert!(base.join("keep_me").exists(), "run_* 以外には触れない");

        // 置き場の中にパッケージがある (ありえないが) なら何もしない。
        let inner = base.join("run_2");
        package(&inner);
        clean_leftovers(&base, &inner);
        assert!(inner.join("package.yaml").exists(), "編集ルートを含む置き場は片付けない");
        // 置き場が見つからない (まだ作られていない) ときも何もしない。
        clean_leftovers(&scratch("none").join("build_staging"), &pkg);

        let _ = std::fs::remove_dir_all(&app_data);
        let _ = std::fs::remove_dir_all(&pkg);
    }
}
