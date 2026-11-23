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
use harness::package_build::{self as pb, ApplyReport, BuildOutcome, BuiltFile, FailedFile, PlanAction};
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

#[cfg(test)]
mod tests {
    use super::*;
    use harness::edit_assist::ToolChat;
    use harness::package_build::{PlanItem, Staging};
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
        ChatTurn { text: Some(t.into()), tool_calls: vec![], finish: Finish::Stop, usage: Usage::default() }
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
            usage: Usage::default(),
        }
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
