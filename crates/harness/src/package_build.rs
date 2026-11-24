//! spec 38: AI でパッケージを組む — **計画 → 1 ファイルずつ書く → まとめて検める → 作者が適用**。
//!
//! 複数ファイルの扱いは機械 (このモジュール) が持ち、モデルには spec 29 と同じ「1 ファイル・
//! 種別ごとの仕様・`sd` で少しずつ」の仕事を順に渡す。**モデルが 1 回に書く量は増やさない**
//! (spec 29 が退けた一発生成 = 出力上限で途中が切れ、ファイルをまたぐ参照が片側だけ書かれる、を
//! 持ち込まない)。
//!
//! - 計画 ([`make_plan`]): 構造化出力 1 回。**エンジンが検める** ([`validate_plan`]) — 却下は理由を
//!   積んで再計画 1 回 (#42: 何をすれば通るかを語る)。順序は機械が決める (参照する側を後に)。
//! - ステージング ([`Staging`]): パッケージの YAML を一時フォルダへ写し、各ファイルのループは
//!   **そこを編集ルートにして**回す。前のファイルの結果を次のファイルが `read` で読み、
//!   `build_vocabulary` が id 表に拾い、`inspect_package` がファイルをまたいで検める —
//!   どれも**既存コードのまま** (新しい検査規則ゼロ)。
//! - 変更セット ([`apply_build`]): ディスクに書くのは作者。開始時の本文と食い違うファイルは
//!   書かない (衝突 = 黙って上書きしない)。
//!
//! 計画にあるのは `create` と `edit` だけ — 削除・改名・メディアは作らない (spec 38 決定 13)。

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use llm_client::{ChatMessage, LlmClient, LlmError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::edit_assist::{self, Diag, EditOutcome, EditRequest, Stopped, ToolChat};
use crate::edit_tools::{self, EditSession};
use crate::editor;
use crate::editor_lint::{self, EditorIssue};
use crate::editor_vocab::{self, EditorVocabulary};
use crate::package_spec;

/// 計画に挙げられるファイル数の上限 (spec 38 決定 12)。
pub const MAX_PLAN_FILES: usize = 12;

/// 計画用の単一ツール名 (構造化出力で強制する)。
pub const PLAN_TOOL: &str = "emit_plan";

// =============================================================================
// 計画
// =============================================================================

/// 計画 1 行の操作。削除・改名は無い (決定 13)。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanAction {
    /// まだ無いファイルを雛形から作って埋める。
    Create,
    /// 既に在るファイルを書き換える。
    Edit,
}

impl PlanAction {
    fn label(self) -> &'static str {
        match self {
            PlanAction::Create => "新規",
            PlanAction::Edit => "変更",
        }
    }
}

/// 計画 1 行 (ファイル 1 つ)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanItem {
    /// パッケージ相対パス (`characters/alice.yaml` など)。
    pub path: String,
    pub action: PlanAction,
    /// このファイルで何を書く・直すか (計画係が書き、作者が直せる)。
    pub instruction: String,
}

/// 計画係の出力 (`emit_plan` の引数)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RawPlan {
    #[serde(default)]
    pub files: Vec<PlanItem>,
}

/// 編集ルート相対パス → (カテゴリ, stem)。編集できる場所でなければ None。
/// カテゴリは `editor::new_file_template` と同じ語彙 (`package` は作成不可)。
fn category_of(rel: &str) -> Option<(&'static str, String)> {
    editor::validate_rel_path(rel).ok()?;
    match rel {
        "package.yaml" => return Some(("package", String::new())),
        "campaign.yaml" => return Some(("campaign", String::new())),
        _ => {}
    }
    let (dir, name) = rel.split_once('/')?;
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name).to_string();
    let cat = match dir {
        "scenarios" => "scenario",
        "characters" => "character",
        "memoria" => "memoria",
        _ => return None,
    };
    Some((cat, stem))
}

/// 書く順 (spec 38 決定 4): 参照される側を先に、参照する側を後に。
/// package.yaml (主人公・全体のフラグ) → characters → memoria → scenarios (cast / present /
/// recall で前の 3 つを指す) → campaign.yaml (modules でシナリオを指す)。
fn order_rank(rel: &str) -> u8 {
    match category_of(rel).map(|(c, _)| c) {
        Some("package") => 0,
        Some("character") => 1,
        Some("memoria") => 2,
        Some("scenario") => 3,
        Some("campaign") => 4,
        _ => 5,
    }
}

/// 計画を検める (spec 38 決定 2)。問題は**全部まとめて**返す (1 件ずつ直させると往復が増える)。
/// 通ったら書く順に並べ替えて返す (同じ種別の中では計画の順を保つ)。
/// 文言は「何をすれば通るか」を言う (#42)。
pub fn validate_plan(items: Vec<PlanItem>, existing: &[String]) -> Result<Vec<PlanItem>, Vec<String>> {
    let mut errs = Vec::new();
    if items.is_empty() {
        return Err(vec!["計画が空です。作る・直すファイルを 1 つ以上挙げてください。".into()]);
    }
    if items.len() > MAX_PLAN_FILES {
        errs.push(format!(
            "ファイルが多すぎます ({} 件、上限 {MAX_PLAN_FILES} 件)。依頼に要るファイルだけに絞ってください。",
            items.len()
        ));
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut ok = Vec::new();
    for (i, it) in items.into_iter().enumerate() {
        let path = it.path.trim().to_string();
        let label = format!("{} 行目 `{path}`", i + 1);
        if it.instruction.trim().is_empty() {
            errs.push(format!("{label}: 指示が空です。このファイルで何を書く・直すのかを書いてください。"));
            continue;
        }
        if !seen.insert(path.clone()) {
            errs.push(format!("{label}: 同じファイルが 2 回挙がっています。1 行にまとめてください。"));
            continue;
        }
        let Some((cat, stem)) = category_of(&path) else {
            errs.push(format!(
                "{label}: 編集できる場所ではありません。書けるのは package.yaml / campaign.yaml と、scenarios/・characters/・memoria/ の直下の .yaml だけです (画像・音声は扱えません)。"
            ));
            continue;
        };
        let exists = existing.iter().any(|f| f == &path);
        match it.action {
            PlanAction::Edit if !exists => {
                errs.push(format!("{label}: このファイルはまだありません。新しく作るなら action を create にしてください。"));
                continue;
            }
            PlanAction::Edit => {}
            PlanAction::Create if exists => {
                errs.push(format!("{label}: このファイルは既にあります。書き換えるなら action を edit にしてください。"));
                continue;
            }
            PlanAction::Create => match editor::new_file_template(cat, &stem) {
                Err(e) => {
                    errs.push(format!("{label}: {e}"));
                    continue;
                }
                Ok((rel, _)) if rel != path => {
                    errs.push(format!("{label}: 新しいファイルは `{rel}` の形で挙げてください。"));
                    continue;
                }
                Ok(_) => {}
            },
        }
        ok.push(PlanItem { path, action: it.action, instruction: it.instruction.trim().to_string() });
    }
    // 新しいシナリオは package.yaml の entry か campaign.yaml の modules から指さないと読まれない
    // (2026-11-23、spec 38 Phase B の実測: 「温室という場所を足して」を別のシナリオのファイルとして
    // 作り、どこからも読まれないまま上限で途中終わりになった)。読ませる行が計画に無ければ却下する。
    let wires = ok.iter().any(|it| it.path == "package.yaml" || it.path == "campaign.yaml");
    for it in ok.iter().filter(|it| it.action == PlanAction::Create && it.path.starts_with("scenarios/")) {
        if !wires {
            errs.push(format!(
                "`{}`: 新しいシナリオを読ませる行がありません。シナリオは package.yaml の entry か campaign.yaml の modules から指さないと読まれません。場所を足すだけなら、新しいファイルは作らず、その場所へ行ける既存のシナリオの locations に足してください (action: edit)。章を増やすなら campaign.yaml も計画に入れてください。",
                it.path
            ));
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    ok.sort_by_key(|it| order_rank(&it.path));
    Ok(ok)
}

/// `emit_plan` の JSON Schema (手書き — schemars の対象は正本の型であって道具ではない)。
pub fn plan_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "files": {
                "type": "array",
                "description": "作る・直すファイルの一覧 (1 行 = 1 ファイル)",
                "items": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "パッケージ相対パス (例 characters/alice.yaml)" },
                        "action": { "type": "string", "enum": ["create", "edit"], "description": "create = 新しく作る / edit = 既にあるファイルを直す" },
                        "instruction": { "type": "string", "description": "このファイルで何を書く・直すか。他のファイルと揃える id は綴りまで書く" }
                    },
                    "required": ["path", "action", "instruction"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["files"],
        "additionalProperties": false
    })
}

/// 計画係の system (純関数)。大原則と構造の断片 + ファイル種別の要旨 + 計画の決まり。
pub fn plan_system_prompt() -> String {
    let mut s = String::new();
    s.push_str(
        "あなたは Lorekeel (LLM をナレーターにした TRPG エンジン) のパッケージを組む計画係です。\n\
         作者の依頼を、**ファイルごとの作業の一覧**に分けて `emit_plan` で出してください。各ファイルは\n\
         あとで別々に (1 ファイルずつ、そのファイル種別の仕様だけを見て) 書かれます。ここでは本文を書きません。\n\n",
    );
    s.push_str(&format!(
        "# 計画の決まり\n\
         - 書けるのは package.yaml / campaign.yaml / scenarios/<名前>.yaml / characters/<名前>.yaml / memoria/<名前>.yaml だけ。画像・音声は扱えない。\n\
         - 既にあるファイルは action: edit、新しく作るファイルは action: create。消す・名前を変えることはできない。\n\
         - 新しいファイルの名前は英数字と _ - だけ (拡張子は .yaml)。characters/ のファイル名はそのままキャラクターの id になる (player / party は使えない)。memoria/ のファイル名は伏線の id になる。\n\
         - ファイルは多くても {MAX_PLAN_FILES} 個まで。依頼に要らないファイルは挙げない (直さないファイルを edit で挙げない)。\n\
         - 各行の instruction には、そのファイルで何を書く・直すかを具体的に書く。**他のファイルと揃える名前 (場所・キャラクター・フラグ・アイテム・ゴールの id)** は全部の行で同じ綴りにし、instruction に明記する — 各ファイルは別々に書かれるので、ここで決めた名前だけが共有される。\n\
         - 書く順は機械が決める (package.yaml → characters → memoria → scenarios → campaign.yaml)。順を気にせず、依頼に要るファイルを挙げればよい。\n\
         - 新しいシナリオは、package.yaml の entry か campaign.yaml の modules から指さないと読まれない。キャラクターはシナリオの cast と、居る場所の present に挙げないと登場しない — キャラクターを登場させるシナリオの行の instruction には、**cast と present の両方**に挙げることを書く。\n\
         - package.yaml にキャラクターや場所の一覧は無い。キャラクターや場所を足すだけなら package.yaml は直さない。\n\n"
    ));
    s.push_str(
        "# ファイルの種類\n\
         - package.yaml — パッケージの表紙: title / entry (最初に読むシナリオか campaign.yaml) / 主人公 (player) / 全体で使うフラグ (globals) / 世界観 (world)\n\
         - characters/<id>.yaml — 登場人物 1 人: name / profile / stats / skills / inventory など\n\
         - memoria/<id>.yaml — 伏線 1 つ: tags / text (トリガーの recall で思い出される)\n\
         - scenarios/<名前>.yaml — 盤面 1 つ: 場所と出口・アイテム・フラグ・ゴール・トリガー・判定。**場所を足すだけなら、その場所へ行ける既存のシナリオの locations に足す** (新しいシナリオのファイルは別の盤面・章で、entry か campaign.yaml から指す)\n\
         - campaign.yaml — 複数のシナリオのつなぎ方: modules / edges\n\n",
    );
    s.push_str("# 仕様の大原則\n\n");
    for id in &package_spec::index().common {
        if let Some(text) = package_spec::slice(id) {
            s.push_str(&package_spec::expand(text));
            s.push('\n');
        }
    }
    s
}

/// 計画係の messages (純関数)。user = 今あるファイル + 今ある id + 依頼。
pub fn plan_messages(request: &str, existing: &[String], ids_table: &str) -> Vec<ChatMessage> {
    let mut u = String::from("# 今あるファイル\n");
    for f in existing {
        u.push_str(&format!("- `{f}`\n"));
    }
    u.push_str("\n# 今ある id (パッケージ内の宣言から収集)\n");
    u.push_str(ids_table);
    u.push_str("\n# 作者の依頼\n");
    u.push_str(request.trim());
    vec![ChatMessage::system(plan_system_prompt()), ChatMessage::user(u)]
}

/// 計画係 (依存性逆転 — 実体は [`LlmClient::generate_structured`]、PoC は fake)。
pub trait Planner {
    fn plan(&self, messages: Vec<ChatMessage>) -> impl Future<Output = Result<RawPlan, LlmError>> + Send;
}

impl Planner for LlmClient {
    /// strict 版で呼ぶ ([`plan_schema`] は strict の制約を満たす手書きの schema)。Anthropic で名指しの
    /// 強制を断られたモデル (Opus 5.5 等) が auto へ降格した周に、入力が schema どおりに縛られる
    /// (2026-11-24 実機: 縛らないと `files` の配列にツール呼び出しの書式を文字列で書いた)。
    async fn plan(&self, messages: Vec<ChatMessage>) -> Result<RawPlan, LlmError> {
        self.generate_structured_strict::<RawPlan>(
            messages,
            PLAN_TOOL,
            "パッケージを組む計画 (ファイルごとの作業の一覧) を出す",
            plan_schema(),
        )
        .await
    }
}

/// 計画が立てられなかった理由。
#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("計画係の呼び出しに失敗しました: {0}")]
    Llm(#[from] LlmError),
    #[error("計画を受け付けられませんでした (立て直しても通りませんでした):\n{}", .0.join("\n"))]
    Invalid(Vec<String>),
}

/// 立てた計画。`rejected` は**最初の計画が受け付けられなかった理由** (一発で通れば空)。
/// 立て直しを数える計器 (spec 38 Phase B の「計画の却下率」) と、画面で「立て直しました」を
/// 言うための材料。
#[derive(Debug, Clone, Serialize)]
pub struct Planned {
    pub items: Vec<PlanItem>,
    pub rejected: Vec<String>,
}

/// 計画を立てる (決定 2)。検査に落ちたら理由と前の計画を積んで**立て直し 1 回**。
pub async fn make_plan<P: Planner>(
    planner: &P,
    request: &str,
    existing: &[String],
    ids_table: &str,
) -> Result<Planned, PlanError> {
    let mut messages = plan_messages(request, existing, ids_table);
    let (errs, retry) = match planner.plan(messages.clone()).await {
        Ok(raw) => match validate_plan(raw.files.clone(), existing) {
            Ok(items) => return Ok(Planned { items, rejected: Vec::new() }),
            Err(errs) => {
                let prev = serde_json::to_string(&raw.files).unwrap_or_default();
                let retry = format!(
                    "前の計画は次の理由で受け付けられません:\n{}\n\n前の計画: {prev}\n\n直した計画を、**全部の行を含めて** `emit_plan` で出し直してください。",
                    errs.iter().map(|e| format!("- {e}")).collect::<Vec<_>>().join("\n")
                );
                (errs, retry)
            }
        },
        // 出力が計画として読めなかった (欄の型が違う・JSON が壊れている・ツールを呼ばなかった)。
        // 検査に落ちたのと同じく理由を言って 1 回だけ立て直す。前の出力そのものは積まない —
        // 壊れた形 (実機ではツール呼び出しの書式が文字列に混ざった) をもう一度見せると、同じ形を
        // 誘う。理由も型の不一致か構文かだけを言う (serde の文面は壊れた値を丸ごと引用する)。
        Err(e @ (LlmError::Parse { .. } | LlmError::NoStructuredOutput)) => {
            let reason = unreadable_reason(&e);
            let retry = format!(
                "前の出力は計画として読めませんでした ({reason})。`emit_plan` を呼び、引数 `files` には \
                 {{\"path\", \"action\", \"instruction\"}} の object を並べた**配列**を入れてください。\
                 ツール呼び出しの書式 (<invoke> など) を文字列の中に書かないでください。全部の行を含めて出し直してください。"
            );
            (vec![format!("出力が計画として読めませんでした ({reason})")], retry)
        }
        Err(e) => return Err(e.into()),
    };
    messages.push(ChatMessage::user(retry));
    let raw = planner.plan(messages).await?;
    let items = validate_plan(raw.files, existing).map_err(PlanError::Invalid)?;
    Ok(Planned { items, rejected: errs })
}

/// 計画が読めなかった理由を短く (立て直しの指示と計器のため)。serde の文面は壊れた値を丸ごと
/// 引用するので使わず、型の不一致か構文か・どこかだけを言う。
fn unreadable_reason(e: &LlmError) -> String {
    match e {
        LlmError::Parse { source, .. } => match source.classify() {
            serde_json::error::Category::Data => format!(
                "欄の型が schema と合いません — files が配列になっていない等 (行 {} 列 {})",
                source.line(),
                source.column()
            ),
            serde_json::error::Category::Eof => "JSON が途中で切れています".to_string(),
            _ => format!("JSON として読めません (行 {} 列 {})", source.line(), source.column()),
        },
        LlmError::NoStructuredOutput => "emit_plan が呼ばれず、本文にも JSON がありません".to_string(),
        other => other.to_string(),
    }
}

// =============================================================================
// ステージング
// =============================================================================

/// 一時フォルダの写し (決定 6)。YAML (package.yaml / campaign.yaml / scenarios / characters /
/// memoria) だけを写し、**中身は LF にそろえる** (道具の `(?m)$` が CRLF の `\r` の前で
/// 止まらないため。`play edit` と同じ扱い — 書き戻しで元の改行に戻す)。
/// アセット名は写さず元のフォルダから取る ([`Staging::vocabulary`])。
pub struct Staging {
    dir: PathBuf,
    src: PathBuf,
    /// 開始時のディスク本文 (LF)。変更セットの「前」と衝突検出の基準。
    original: BTreeMap<String, String>,
    /// 開始時に既にあった層 2 の指摘 (作者の持ち込み分 — 直させも報告もしない)。
    baseline: Vec<EditorIssue>,
}

impl Staging {
    /// `src` (パッケージ) を `dir` (専用の一時フォルダ) へ写す。`dir` が既にあれば消して作り直す
    /// ので、**パッケージと重なる場所は拒否**する (消してはいけないものを消さない)。
    pub fn prepare(src: &Path, dir: &Path) -> Result<Self, String> {
        let src_c = src.canonicalize().map_err(|e| format!("パッケージフォルダを開けません: {e}"))?;
        if dir.exists() {
            let dir_c = dir.canonicalize().map_err(|e| format!("一時フォルダを開けません: {e}"))?;
            if dir_c.starts_with(&src_c) || src_c.starts_with(&dir_c) {
                return Err("一時フォルダはパッケージの外に置いてください".into());
            }
            std::fs::remove_dir_all(dir).map_err(|e| format!("一時フォルダを片付けられません: {e}"))?;
        } else if let Some(parent) = dir.parent() {
            if let Ok(parent_c) = parent.canonicalize() {
                if parent_c.starts_with(&src_c) {
                    return Err("一時フォルダはパッケージの外に置いてください".into());
                }
            }
        }
        std::fs::create_dir_all(dir).map_err(|e| format!("一時フォルダを作れません: {e}"))?;
        let mut original = BTreeMap::new();
        for e in editor::list_files(src) {
            let raw = std::fs::read_to_string(src.join(&e.rel_path))
                .map_err(|err| format!("`{}` を読めません: {err}", e.rel_path))?;
            let text = raw.replace("\r\n", "\n");
            let dst = dir.join(&e.rel_path);
            if let Some(p) = dst.parent() {
                std::fs::create_dir_all(p).map_err(|err| format!("一時フォルダを作れません: {err}"))?;
            }
            std::fs::write(&dst, &text).map_err(|err| format!("一時フォルダへ写せません: {err}"))?;
            original.insert(e.rel_path, text);
        }
        let baseline = editor_lint::inspect_issues(dir);
        Ok(Self { dir: dir.to_path_buf(), src: src.to_path_buf(), original, baseline })
    }

    /// 一時フォルダ (各ループの編集ルート)。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 開始時にあったファイル (計画の検査の `existing`)。
    pub fn existing(&self) -> Vec<String> {
        self.original.keys().cloned().collect()
    }

    /// 開始時の本文 (LF)。
    pub fn original(&self, rel: &str) -> Option<&str> {
        self.original.get(rel).map(String::as_str)
    }

    /// 開始時に既にあった層 2 の指摘の件数。
    pub fn baseline_issue_count(&self) -> usize {
        self.baseline.len()
    }

    pub fn read(&self, rel: &str) -> Result<String, String> {
        let p = editor::resolve_in_root(&self.dir, rel)?;
        std::fs::read_to_string(p).map_err(|e| format!("`{rel}` を読めません: {e}"))
    }

    pub fn write(&self, rel: &str, text: &str) -> Result<(), String> {
        editor::validate_rel_path(rel)?;
        let p = self.dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("フォルダを作れません: {e}"))?;
        }
        editor::atomic_write(&p, text)
    }

    fn remove(&self, rel: &str) {
        if editor::validate_rel_path(rel).is_ok() {
            let _ = std::fs::remove_file(self.dir.join(rel));
        }
    }

    /// 語彙 (id 表・既知キー表の素)。YAML は一時フォルダ、アセット名は元のパッケージから。
    pub fn vocabulary(&self) -> EditorVocabulary {
        editor_vocab::build_vocabulary_with_media(&self.dir, &self.src)
    }

    /// 開始時に無かった層 2 の指摘 (この組み立てが生んだもの)。
    pub fn new_issues(&self) -> Vec<EditorIssue> {
        let mut seen = BTreeSet::new();
        editor_lint::inspect_issues(&self.dir)
            .into_iter()
            .filter(|i| !self.baseline.contains(i))
            .filter(|i| seen.insert((i.file.clone(), i.severity.clone(), i.message.clone())))
            .collect()
    }

    /// 一時フォルダを消す (適用か破棄のあと)。
    pub fn discard(self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// =============================================================================
// 組み立て (1 ファイルずつ)
// =============================================================================

/// 1 ファイルに回したループの種類 (2026-11-24)。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LoopKind {
    /// 計画の順に書く最初の回。
    Main,
    /// 雛形のまま終わった新規ファイルのやり直し (層 2 の指摘も持っていれば一緒に直す)。
    Redo,
    /// 層 2 の指摘の修復。
    Repair,
}

/// ループ 1 本の周回とトークン (2026-11-24、実機 1 回目の後のユーザー要望)。後から「どのファイルの
/// どの回に何トークン使ったか」を確かめるための記録 — 値は spec 29 のループの返りそのもの。
/// 実機では組み立ての入力の約 7 割が 1 本の長いループに集まったのに、利用量ログ (`usage.jsonl`) には
/// 役割 `editor` としか残らず、どのファイルの回だったか分からなかった。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LoopUsage {
    pub kind: LoopKind,
    pub iterations: u32,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_read: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
}

impl LoopUsage {
    fn of(kind: LoopKind, o: &EditOutcome) -> Self {
        Self {
            kind,
            iterations: o.iterations,
            prompt_tokens: o.prompt_tokens,
            completion_tokens: o.completion_tokens,
            cache_read: o.cache_read,
            stopped: o.stopped,
        }
    }
}

/// 組み上がったファイル 1 つ (変更セットの 1 行)。本文はどちらも LF。
#[derive(Debug, Clone, Serialize)]
pub struct BuiltFile {
    pub path: String,
    pub action: PlanAction,
    /// 開始時の本文 (新規なら None)。
    pub before: Option<String>,
    pub after: String,
    pub changed: bool,
    /// モデルの報告 (修復の周があれば続けて書く)。
    pub summary: String,
    /// このファイル単体の診断 (層 1)。
    pub diagnostics: Vec<Diag>,
    /// 全ループの周回の合計 (内訳は `loops`)。
    pub iterations: u32,
    /// 最初の回 (`Main`) の打ち切り理由。やり直し・修復の回の打ち切りは `loops` に残る。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
    /// このファイルに回したループ (回した順)。周回とトークンの内訳。
    pub loops: Vec<LoopUsage>,
    /// 層 2 の指摘で修復の周を回したか。
    pub repaired: bool,
    /// 新規なのに**雛形のまま**終わった (モデルが埋められなかった)。`changed` は新規なら常に真
    /// なので、これを見ないと「全部書けた」に見える (spec 38 Phase B の実測)。
    pub template_only: bool,
}

/// 書けなかったファイル (LLM の呼び出しが失敗し、本文が変わっていなかった)。
#[derive(Debug, Clone, Serialize)]
pub struct FailedFile {
    pub path: String,
    pub message: String,
}

/// 組み立ての結果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct BuildOutcome {
    pub files: Vec<BuiltFile>,
    pub failed: Vec<FailedFile>,
    /// 止めたので書かなかったファイル。
    pub not_run: Vec<String>,
    /// この組み立てが生んだ層 2 の指摘のうち、修復しても残ったもの。
    pub package_issues: Vec<EditorIssue>,
    /// 開始時から既にあった層 2 の指摘の件数 (作者の持ち込み分。直させていない)。
    pub preexisting_issues: usize,
    pub iterations: u32,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_read: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
}

impl BuildOutcome {
    fn add_usage(&mut self, o: &EditOutcome) {
        self.iterations += o.iterations;
        self.prompt_tokens += o.prompt_tokens;
        self.completion_tokens += o.completion_tokens;
        self.cache_read += o.cache_read;
    }
}

/// 1 ファイル分の指示 (純関数)。計画全体を見せ、このファイルの役割を ▶ で示す (決定 7)。
/// 前のファイルの会話は積まない — 前のファイルの結果は一時フォルダの本文と id 表で届く。
pub fn file_instruction(request: &str, plan: &[PlanItem], idx: usize) -> String {
    let item = &plan[idx];
    let mut s = String::from("# パッケージ全体への作者の依頼\n");
    s.push_str(request.trim());
    s.push_str("\n\n# 計画 (▶ がこのファイル。✓ は書き終えたファイル、・はこの後に別の回で書かれるファイル)\n");
    for (i, it) in plan.iter().enumerate() {
        let mark = if i == idx { "▶" } else if i < idx { "✓" } else { "・" };
        s.push_str(&format!("{mark} `{}` ({}) — {}\n", it.path, it.action.label(), it.instruction.replace('\n', " ")));
    }
    s.push_str("\n# このファイルへの指示\n");
    s.push_str(&item.instruction);
    if item.action == PlanAction::Create {
        s.push_str(
            "\n\nこのファイルは計画に沿って**雛形から新しく作られました** (上の本文が雛形)。雛形の値を指示どおりに書き換え、要る節を足して埋めてください。大きな塊は 1 回の `sd` で全部入れず、節ごとに分けて足してください (1 回の差分には上限があります)。",
        );
    }
    s.push_str("\n\n書き換えられるのはこのファイルだけです。他のファイルに要るものは計画の別の行で書かれます。✓ のファイルは `read` で中身を確かめられます。");
    s
}

/// 層 2 の指摘を直させる指示 (純関数)。
pub fn repair_instruction(request: &str, issues: &[EditorIssue]) -> String {
    let list: Vec<String> = issues.iter().map(|i| format!("- [{}] {}", i.severity, i.message)).collect();
    format!(
        "# パッケージ全体の検査で見つかった問題\n\
         パッケージ全体 (ファイルをまたぐ参照を含む) を検査したところ、このファイルについて次の問題が出ました:\n{}\n\n\
         直してください。書き換えられるのはこのファイルだけです。原因が他のファイルにあるなら `read` / `grep` で確かめ、このファイル側でできる直し方を選んでください (例: 宣言の無い id を、在る id にそろえる)。\n\n\
         # もとの依頼 (参考)\n{}",
        list.join("\n"),
        request.trim()
    )
}

/// 1 ファイル分のループ (spec 29 の `run_edit_loop` をそのまま使う)。終わったら一時フォルダへ書く。
async fn edit_one<C: ToolChat>(
    chat: &C,
    staging: &Staging,
    rel: &str,
    instruction: String,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(String) + Send),
) -> Result<EditOutcome, String> {
    let entries = editor::list_files(staging.dir());
    let kind = edit_tools::kind_of(&entries, rel).ok_or_else(|| format!("`{rel}` が一時フォルダにありません"))?;
    let files: Vec<String> = entries.iter().map(|e| e.rel_path.clone()).collect();
    let vocab = staging.vocabulary();
    let initial = staging.read(rel)?;
    let req = EditRequest {
        kind: kind.clone(),
        target_rel: rel.to_string(),
        files: files.clone(),
        keys_table: edit_tools::keys_table(&vocab, &kind),
        ids_table: edit_tools::ids_table(&vocab),
        instruction,
        initial_text: initial.clone(),
    };
    let messages = edit_assist::build_messages(&req);
    let mut session = EditSession::new(staging.dir(), rel, &kind, files, &initial);
    let mut prefixed = |line: String| progress(format!("{rel}: {line}"));
    let out = edit_assist::run_edit_loop(
        chat,
        messages,
        edit_assist::tool_specs(),
        &mut session,
        &initial,
        cancel,
        &mut prefixed,
    )
    .await
    .map_err(|e| e.to_string())?;
    staging.write(rel, &out.text)?;
    Ok(out)
}

/// 組み立て (決定 1・4〜8・12)。計画は**ここでも検め直す** (作者が画面で書き換えた計画を受けるため)。
///
/// - ファイルごとに spec 29 のループを回し、終わるたびに一時フォルダへ書く。
/// - 打ち切り (上限・反復) はそのファイルの結果に残して次へ進む。取り消しと LLM の失敗は組み立て
///   ごと止め、**書き終えたファイルは結果に残す** (止めても無駄にしない)。
/// - 全部書き終えたら層 2 の検査。**この組み立てが生んだ**指摘 (エラーと警告) を持つ計画内の
///   ファイルに修復の周を 1 回ずつ。作者が持ち込んだ指摘には触れない。
pub async fn run_build<C: ToolChat>(
    chat: &C,
    request: &str,
    plan: &[PlanItem],
    staging: &Staging,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(String) + Send),
) -> Result<BuildOutcome, Vec<String>> {
    let plan = validate_plan(plan.to_vec(), &staging.existing())?;
    let mut out = BuildOutcome { preexisting_issues: staging.baseline_issue_count(), ..Default::default() };
    let rest = |from: usize| plan[from..].iter().map(|p| p.path.clone()).collect::<Vec<_>>();
    // 新規ファイルに置いた雛形 (終わって雛形のままなら「埋められなかった」)。
    let mut templates: BTreeMap<String, String> = BTreeMap::new();

    for (idx, item) in plan.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            out.stopped = Some(Stopped::Cancel);
            out.not_run = rest(idx);
            break;
        }
        progress(format!("▶ {} ({})", item.path, item.action.label()));
        if item.action == PlanAction::Create {
            // 雛形 (create_file と同じもの) を置いてから、モデルに sd で埋めさせる。
            let written = category_of(&item.path)
                .ok_or_else(|| format!("`{}` は作れない場所です", item.path))
                .and_then(|(cat, stem)| editor::new_file_template(cat, &stem))
                .and_then(|(rel, tpl)| {
                    staging.write(&rel, &tpl)?;
                    templates.insert(rel, tpl);
                    Ok(())
                });
            if let Err(e) = written {
                out.failed.push(FailedFile { path: item.path.clone(), message: e });
                continue;
            }
        }
        match edit_one(chat, staging, &item.path, file_instruction(request, &plan, idx), cancel, progress).await {
            Ok(o) => {
                out.add_usage(&o);
                let main = LoopUsage::of(LoopKind::Main, &o);
                let stop = o.stopped;
                let before = staging.original(&item.path).map(str::to_string);
                let template_only = templates.get(&item.path).is_some_and(|t| *t == o.text);
                out.files.push(BuiltFile {
                    path: item.path.clone(),
                    action: item.action,
                    changed: before.as_deref() != Some(o.text.as_str()),
                    before,
                    after: o.text,
                    summary: o.summary,
                    diagnostics: o.diagnostics,
                    iterations: o.iterations,
                    stopped: o.stopped,
                    loops: vec![main],
                    repaired: false,
                    template_only,
                });
                if matches!(stop, Some(Stopped::Cancel) | Some(Stopped::Error)) {
                    out.stopped = stop;
                    out.not_run = rest(idx + 1);
                    break;
                }
            }
            Err(message) => {
                if item.action == PlanAction::Create {
                    staging.remove(&item.path); // 埋められなかった雛形は変更セットに出さない
                }
                out.failed.push(FailedFile { path: item.path.clone(), message });
                out.stopped = Some(Stopped::Error);
                out.not_run = rest(idx + 1);
                break;
            }
        }
    }

    // 層 2 (決定 8)。止めたときは修復しない (結果の報告だけ)。
    if out.stopped.is_none() {
        let issues = staging.new_issues();
        for i in 0..out.files.len() {
            if cancel.load(Ordering::Relaxed) {
                out.stopped = Some(Stopped::Cancel);
                break;
            }
            let path = out.files[i].path.clone();
            let mine: Vec<EditorIssue> = issues.iter().filter(|x| x.file.as_deref() == Some(path.as_str())).cloned().collect();
            // 雛形のまま終わったファイルも 1 回だけやり直す (新しい文脈で、最初の指示から)。
            let untouched = out.files[i].template_only;
            let kind = if untouched { LoopKind::Redo } else { LoopKind::Repair };
            if mine.is_empty() && !untouched {
                continue;
            }
            let instruction = if untouched {
                progress(format!("やり直し: {path} (雛形のまま)"));
                let idx = plan.iter().position(|p| p.path == path).unwrap_or(0);
                let mut s = file_instruction(request, &plan, idx);
                s.push_str(
                    "\n\n**前の回ではこのファイルを埋められず、雛形のまま終わりました。** `sd` が一致しないときは、道具の返りの手がかり (足りない `(?m)`、本文にある実際の字面) に従って pattern を直してください。",
                );
                if !mine.is_empty() {
                    s.push_str("\n\n");
                    s.push_str(&repair_instruction(request, &mine));
                }
                s
            } else {
                progress(format!("修復: {path} ({} 件)", mine.len()));
                repair_instruction(request, &mine)
            };
            match edit_one(chat, staging, &path, instruction, cancel, progress).await {
                Ok(o) => {
                    out.add_usage(&o);
                    let f = &mut out.files[i];
                    f.loops.push(LoopUsage::of(kind, &o));
                    f.template_only = templates.get(&path).is_some_and(|t| *t == o.text);
                    f.changed = f.before.as_deref() != Some(o.text.as_str());
                    f.after = o.text;
                    f.diagnostics = o.diagnostics;
                    f.iterations += o.iterations;
                    f.repaired = true;
                    if !o.summary.trim().is_empty() {
                        f.summary = format!("{}\n(修復) {}", f.summary.trim_end(), o.summary.trim());
                    }
                    if o.stopped == Some(Stopped::Cancel) {
                        out.stopped = Some(Stopped::Cancel);
                        break;
                    }
                }
                Err(e) => {
                    let f = &mut out.files[i];
                    f.summary = format!("{}\n(修復の周が失敗しました: {e})", f.summary.trim_end());
                }
            }
        }
    }
    out.package_issues = staging.new_issues();
    Ok(out)
}

// =============================================================================
// 適用 (作者)
// =============================================================================

/// 適用の結果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApplyReport {
    pub written: Vec<String>,
    /// 実行中にディスクの本文が変わった (新規なら同名ができた) ので書かなかったファイル。
    pub conflicts: Vec<String>,
    pub errors: Vec<FailedFile>,
    pub forked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fork_warning: Option<String>,
}

/// 変更セットのうち `selected` をディスクへ書く (決定 9・10)。
///
/// - **衝突**: 開始時の本文 (`before`) と今のディスクが食い違えば書かない (新規は同名が既に
///   あれば書かない)。比較は改行を LF にそろえて行う。
/// - 改行は**元のファイルに合わせる** (CRLF だったファイルは CRLF で書く。新規は LF)。
/// - 1 つでも書けたら fork (書庫版の出所メタを消す。`save_with_fork` と同じ意味論)。
pub fn apply_build(
    root: &Path,
    files: &[BuiltFile],
    selected: &[String],
    fork: bool,
    meta_files: &[&str],
) -> ApplyReport {
    let mut rep = ApplyReport::default();
    for f in files.iter().filter(|f| f.changed && selected.contains(&f.path)) {
        if let Err(e) = editor::validate_rel_path(&f.path) {
            rep.errors.push(FailedFile { path: f.path.clone(), message: e });
            continue;
        }
        let path = root.join(&f.path);
        let current = std::fs::read_to_string(&path).ok();
        let conflict = match &f.before {
            None => path.exists(),
            Some(b) => current.as_deref().map(|c| c.replace("\r\n", "\n")).as_deref() != Some(b.as_str()),
        };
        if conflict {
            rep.conflicts.push(f.path.clone());
            continue;
        }
        let crlf = current.as_deref().is_some_and(|c| c.contains("\r\n"));
        let text = if crlf { f.after.replace('\n', "\r\n") } else { f.after.clone() };
        let res = path
            .parent()
            .map(|p| std::fs::create_dir_all(p).map_err(|e| format!("フォルダを作れません: {e}")))
            .unwrap_or(Ok(()))
            .and_then(|_| editor::atomic_write(&path, &text));
        match res {
            Ok(()) => rep.written.push(f.path.clone()),
            Err(message) => rep.errors.push(FailedFile { path: f.path.clone(), message }),
        }
    }
    if !rep.written.is_empty() {
        let (forked, warning) = editor::fork_meta(root, fork, meta_files);
        rep.forked = forked;
        rep.fork_warning = warning;
    }
    rep
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_client::{ChatTurn, Finish, ToolCall, ToolSpec, Usage};
    use std::sync::Mutex;

    fn item(path: &str, action: PlanAction, instruction: &str) -> PlanItem {
        PlanItem { path: path.into(), action, instruction: instruction.into() }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lorekeel_build_{name}_{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(dir.join("scenarios")).unwrap();
        std::fs::create_dir_all(dir.join("characters")).unwrap();
        std::fs::create_dir_all(dir.join("images")).unwrap();
        std::fs::write(dir.join("package.yaml"), "title: T\nentry: scenarios/main.yaml\n").unwrap();
        std::fs::write(
            dir.join("scenarios/main.yaml"),
            "title: T\r\nstart: hall\r\ngoal: { kind: flag_is, key: done, value: true }\r\nallowed_flags: [done]\r\nlocations:\r\n  hall:\r\n    description: 広間\r\n",
        )
        .unwrap();
        std::fs::write(dir.join("characters/moka.yaml"), "name: モカ\nprofile: \"\"\n").unwrap();
        std::fs::write(dir.join("images/hall.webp"), b"RIFF").unwrap();
        std::fs::write(dir.join(".env"), "LLM_API_KEY=secret\n").unwrap();
        dir
    }

    // --- fake ---
    /// 台本どおりに返し、各周の messages (system と最後の user) を記録する。
    struct Scripted {
        queue: Mutex<Vec<ChatTurn>>,
        seen: Mutex<Vec<Vec<ChatMessage>>>,
    }
    impl Scripted {
        fn new(turns: Vec<ChatTurn>) -> Self {
            Self { queue: Mutex::new(turns), seen: Mutex::new(Vec::new()) }
        }
        fn next(&self, m: Vec<ChatMessage>) -> Result<ChatTurn, LlmError> {
            self.seen.lock().unwrap().push(m);
            let mut q = self.queue.lock().unwrap();
            if q.is_empty() {
                return Ok(text("(台本切れ)"));
            }
            Ok(q.remove(0))
        }
        /// 各ファイルのループの最初の周の system (= そのファイルの素材)。
        fn first_systems(&self) -> Vec<String> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.len() == 2)
                .map(|m| m[0].content.clone())
                .collect()
        }
    }
    impl ToolChat for Scripted {
        async fn chat(&self, m: Vec<ChatMessage>, _t: Vec<ToolSpec>) -> Result<ChatTurn, LlmError> {
            self.next(m)
        }
        async fn wrap_up(&self, m: Vec<ChatMessage>, _t: Vec<ToolSpec>) -> Result<ChatTurn, LlmError> {
            self.next(m)
        }
    }
    fn text(t: &str) -> ChatTurn {
        ChatTurn { text: Some(t.into()), tool_calls: vec![], finish: Finish::Stop, usage: Usage { prompt: 10, completion: 2, cache_read: 4, cost_usd: None } }
    }
    fn calls(list: Vec<(&str, Value)>) -> ChatTurn {
        ChatTurn {
            text: None,
            tool_calls: list
                .into_iter()
                .enumerate()
                .map(|(i, (n, a))| ToolCall { id: format!("c{i}"), name: n.into(), args: a, thought_signature: None })
                .collect(),
            finish: Finish::ToolUse,
            usage: Usage { prompt: 100, completion: 5, cache_read: 0, cost_usd: None },
        }
    }
    /// sd の preview → apply → 報告、の 3 周。
    fn sd(pattern: &str, replacement: &str, report: &str) -> Vec<ChatTurn> {
        vec![
            calls(vec![("sd", json!({ "pattern": pattern, "replacement": replacement }))]),
            calls(vec![("sd", json!({ "pattern": pattern, "replacement": replacement, "apply": true }))]),
            text(report),
        ]
    }

    fn run(chat: &Scripted, request: &str, plan: &[PlanItem], st: &Staging, cancel: &AtomicBool) -> (BuildOutcome, Vec<String>) {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let mut lines = Vec::new();
        let out = rt
            .block_on(run_build(chat, request, plan, st, cancel, &mut |l| lines.push(l)))
            .expect("計画は通る");
        (out, lines)
    }

    #[test]
    fn plan_is_checked_with_reasons_and_sorted_by_dependency() {
        let existing = vec!["package.yaml".to_string(), "scenarios/main.yaml".into(), "characters/moka.yaml".into()];
        // 通る計画は依存の向き (package → characters → memoria → scenarios → campaign) に並ぶ。
        let ok = validate_plan(
            vec![
                item("campaign.yaml", PlanAction::Create, "2 章をつなぐ"),
                item("scenarios/main.yaml", PlanAction::Edit, "alice を cast に"),
                item("memoria/promise.yaml", PlanAction::Create, "約束の伏線"),
                item("characters/alice.yaml", PlanAction::Create, "司書のアリス"),
                item(" package.yaml ", PlanAction::Edit, "  title を変える  "),
            ],
            &existing,
        )
        .unwrap();
        let order: Vec<&str> = ok.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(order, ["package.yaml", "characters/alice.yaml", "memoria/promise.yaml", "scenarios/main.yaml", "campaign.yaml"]);
        assert_eq!(ok[0].instruction, "title を変える", "前後の空白は落とす");

        // 落ちる計画は**全部の理由**を、何をすれば通るかの形で返す。
        let errs = validate_plan(
            vec![
                item("scenarios/main.yaml", PlanAction::Create, "x"),
                item("characters/ghost.yaml", PlanAction::Edit, "x"),
                item("characters/player.yaml", PlanAction::Create, "x"),
                item("../evil.yaml", PlanAction::Create, "x"),
                item("images/a.png", PlanAction::Create, "x"),
                item("memoria/a b.yaml", PlanAction::Create, "x"),
                item("scenarios/two.yml", PlanAction::Create, "x"),
                item("package.yaml", PlanAction::Edit, " "),
                item("characters/moka.yaml", PlanAction::Edit, "x"),
                item("characters/moka.yaml", PlanAction::Edit, "y"),
            ],
            &existing,
        )
        .unwrap_err();
        let all = errs.join("\n");
        assert!(all.contains("1 行目") && all.contains("action を edit"), "{all}");
        assert!(all.contains("2 行目") && all.contains("action を create"), "{all}");
        assert!(all.contains("3 行目") && all.contains("予約された id"), "{all}");
        assert!(all.contains("4 行目") && all.contains("編集できる場所ではありません"), "{all}");
        assert!(all.contains("5 行目") && all.contains("編集できる場所ではありません"), "{all}");
        assert!(all.contains("6 行目") && all.contains("英数字"), "{all}");
        assert!(all.contains("7 行目") && all.contains("`scenarios/two.yaml` の形"), "{all}");
        assert!(all.contains("8 行目") && all.contains("指示が空"), "{all}");
        assert!(all.contains("10 行目") && all.contains("2 回挙がっています"), "{all}");
        assert!(!all.contains("9 行目"), "正しい行は名指ししない: {all}");

        let many: Vec<PlanItem> = (0..=MAX_PLAN_FILES).map(|i| item(&format!("memoria/m{i}.yaml"), PlanAction::Create, "x")).collect();
        assert!(validate_plan(many, &existing).unwrap_err()[0].contains("多すぎます"));
        assert!(validate_plan(vec![], &existing).unwrap_err()[0].contains("空"));
        // package.yaml は作れない (作成の語彙に無い)。
        assert!(validate_plan(vec![item("package.yaml", PlanAction::Create, "x")], &[]).is_err());

        // 新しいシナリオを読ませる行 (package.yaml / campaign.yaml) が無い計画は却下 (Phase B の実測:
        // 場所を足す依頼を別ファイルとして作り、どこからも読まれなかった)。
        let orphan = validate_plan(
            vec![
                item("scenarios/greenhouse.yaml", PlanAction::Create, "温室"),
                item("scenarios/main.yaml", PlanAction::Edit, "温室への出口"),
            ],
            &existing,
        )
        .unwrap_err();
        assert!(orphan.iter().any(|e| e.contains("scenarios/greenhouse.yaml") && e.contains("locations に足して")), "{orphan:?}");
        assert!(validate_plan(
            vec![
                item("scenarios/ch2.yaml", PlanAction::Create, "2 章"),
                item("campaign.yaml", PlanAction::Create, "1 章 → 2 章"),
            ],
            &existing,
        )
        .is_ok(), "読ませる行があれば通る");
    }

    /// 計画係の fake: 返す計画を順に出し、受け取った messages を記録する。
    struct FakePlanner(Mutex<Vec<RawPlan>>, Mutex<Vec<Vec<ChatMessage>>>);
    impl Planner for FakePlanner {
        async fn plan(&self, messages: Vec<ChatMessage>) -> Result<RawPlan, LlmError> {
            self.1.lock().unwrap().push(messages);
            Ok(self.0.lock().unwrap().remove(0))
        }
    }

    /// 結果 (成功か失敗) を台本どおりに返す計画係。
    struct ResultPlanner(Mutex<Vec<Result<RawPlan, LlmError>>>, Mutex<Vec<Vec<ChatMessage>>>);
    impl Planner for ResultPlanner {
        async fn plan(&self, messages: Vec<ChatMessage>) -> Result<RawPlan, LlmError> {
            self.1.lock().unwrap().push(messages);
            self.0.lock().unwrap().remove(0)
        }
    }

    /// 【計画が読めなかったら 1 回だけ立て直す (2026-11-24 ユーザー実機、Opus 5.5)】Opus 5.5 が
    /// `files` (object の配列) にツール呼び出しの書式を文字列で書き、`instruction` が上の階層へ漏れた。
    /// 従来は検査に落ちた計画だけを立て直し、**読めなかった計画はそのまま失敗**にしていた。
    /// 読めなかった理由 (型の不一致か構文か) を言って 1 回だけ立て直す。**壊れた出力そのものは積まない**
    /// (同じ形をもう一度見せると同じ形を誘う)。ツールを呼ばなかったときも同じ。2 回目も読めなければ諦め、
    /// 読めないこと以外の失敗 (HTTP・空の応答など) は立て直さない。
    #[test]
    fn an_unreadable_plan_is_retried_once_without_echoing_the_broken_output() {
        // 実機の出力 (2026-11-24 のスクリーンショットから。instruction は短く切った)。
        const RAW: &str = r#"{"files":"\n<invoke name=\"package.yaml\">\n<parameter name=\"action\">edit","instruction":"世界観 (world) を推理風中世ファンタジーとして厚くする。"}"#;
        let parse = || LlmError::Parse { source: serde_json::from_str::<RawPlan>(RAW).unwrap_err(), raw: RAW.into() };
        let good = || Ok(RawPlan { files: vec![item("package.yaml", PlanAction::Edit, "世界観を厚くする")] });
        let existing = vec!["package.yaml".to_string()];
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();

        let p = ResultPlanner(Mutex::new(vec![Err(parse()), good()]), Mutex::new(vec![]));
        let plan = rt.block_on(make_plan(&p, "陰謀を防ぐ推理にして", &existing, "")).unwrap();
        assert_eq!(plan.items.len(), 1);
        assert!(plan.rejected[0].contains("読めませんでした") && plan.rejected[0].contains("配列"), "{:?}", plan.rejected);
        let seen = p.1.lock().unwrap();
        assert_eq!(seen.len(), 2);
        let retry = &seen[1].last().unwrap().content;
        assert!(retry.contains("配列") && retry.contains("emit_plan"), "何をすれば通るかを言う: {retry}");
        assert!(!retry.contains("<invoke name="), "壊れた出力を積まない: {retry}");
        assert!(!retry.contains("推理風中世"), "壊れた出力の中身を積まない: {retry}");
        drop(seen);

        // ツールを呼ばなかったときも立て直す。
        let p = ResultPlanner(Mutex::new(vec![Err(LlmError::NoStructuredOutput), good()]), Mutex::new(vec![]));
        let plan = rt.block_on(make_plan(&p, "x", &existing, "")).unwrap();
        assert!(plan.rejected[0].contains("emit_plan が呼ばれず"), "{:?}", plan.rejected);

        // 2 回目も読めなければ諦める (3 回目は呼ばない)。
        let p = ResultPlanner(Mutex::new(vec![Err(parse()), Err(parse())]), Mutex::new(vec![]));
        let err = rt.block_on(make_plan(&p, "x", &existing, "")).unwrap_err();
        assert!(matches!(err, PlanError::Llm(LlmError::Parse { .. })), "{err}");
        assert_eq!(p.1.lock().unwrap().len(), 2);

        // 読めないこと以外の失敗は立て直さない (再送で直る類いは llm_client のリトライの領分)。
        let p = ResultPlanner(Mutex::new(vec![Err(LlmError::EmptyResponse)]), Mutex::new(vec![]));
        assert!(matches!(rt.block_on(make_plan(&p, "x", &existing, "")), Err(PlanError::Llm(LlmError::EmptyResponse))));
        assert_eq!(p.1.lock().unwrap().len(), 1);
    }

    /// 【配線】計画係の実体 (`impl Planner for LlmClient`) が strict 版で呼んでいること。Opus 5.5 と同じ
    /// 応答 (名指しの強制を 400) を返す台本サーバに当て、降格して再送した周の `emit_plan` に `strict: true`
    /// が載るのを本文で確かめる。純関数のテストでは「strict 版を作ったが計画係が呼んでいない」が見えない
    /// (2026-10-05 の #109 と同じ配線の穴)。
    #[test]
    fn the_planner_sends_emit_plan_as_strict_once_forcing_is_refused() {
        use std::io::{Read, Write};
        const REJECT: &str = r#"{"type":"error","error":{"type":"invalid_request_error","message":"tool_choice: type \"tool\" and \"any\" are not supported for this model."}}"#;
        const PLAN: &str = r#"{"content":[{"type":"tool_use","id":"toolu_1","name":"emit_plan","input":{"files":[{"path":"package.yaml","action":"edit","instruction":"世界観を厚くする"}]}}],"stop_reason":"tool_use","usage":{"input_tokens":10,"output_tokens":20}}"#;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let bodies = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let bodies_w = bodies.clone();
        std::thread::spawn(move || {
            for (status, body) in [(400, REJECT), (200, PLAN)] {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let head_end = loop {
                    let n = s.read(&mut chunk).unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..head_end]).to_ascii_lowercase();
                let len: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .map(|v| v.trim().parse().unwrap())
                    .unwrap_or(0);
                while buf.len() < head_end + len {
                    let n = s.read(&mut chunk).unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                }
                bodies_w.lock().unwrap().push(String::from_utf8_lossy(&buf[head_end..head_end + len]).into_owned());
                let resp = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                s.write_all(resp.as_bytes()).unwrap();
            }
        });
        let mut cfg = llm_client::LlmConfig::new(format!("http://{addr}"), "sk-test", "claude-opus-5-5");
        cfg.provider = llm_client::Provider::Anthropic;
        cfg.max_retries = 1;
        let client = LlmClient::new(cfg).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let plan = rt.block_on(make_plan(&client, "世界観を厚くして", &["package.yaml".to_string()], "")).unwrap();
        assert_eq!(plan.items[0].path, "package.yaml");
        let bodies = bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2);
        let resent: Value = serde_json::from_str(&bodies[1]).unwrap();
        assert_eq!(resent["tool_choice"]["type"], "auto");
        assert_eq!(resent["tools"][0]["name"], PLAN_TOOL);
        assert_eq!(resent["tools"][0]["strict"], true, "降格した周の emit_plan は strict で縛る");
    }

    #[test]
    fn a_rejected_plan_is_retried_once_with_the_reasons() {
        let existing = vec!["package.yaml".to_string()];
        let bad = RawPlan { files: vec![item("package.yaml", PlanAction::Create, "x")] };
        let good = RawPlan { files: vec![item("package.yaml", PlanAction::Edit, "題名を変える")] };
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();

        let p = FakePlanner(Mutex::new(vec![bad.clone(), good]), Mutex::new(vec![]));
        let plan = rt.block_on(make_plan(&p, "題名を変えて", &existing, "(まだ宣言が無い)\n")).unwrap();
        assert_eq!(plan.items[0].action, PlanAction::Edit);
        assert!(plan.rejected[0].contains("action を edit"), "最初の却下理由を返す (計器): {:?}", plan.rejected);
        let seen = p.1.lock().unwrap();
        assert_eq!(seen.len(), 2);
        let retry = &seen[1].last().unwrap().content;
        assert!(retry.contains("action を edit") && retry.contains("前の計画"), "理由と前の計画を積む: {retry}");
        // 計画係の素材: 大原則・決まり・今あるファイル・依頼。
        assert!(seen[0][0].content.contains("# 計画の決まり") && seen[0][0].content.contains("# 仕様の大原則"));
        assert!(seen[0][1].content.contains("`package.yaml`") && seen[0][1].content.contains("題名を変えて"));
        drop(seen);

        // 一発で通れば却下理由は空 (呼び出しは 1 回)。
        let good = RawPlan { files: vec![item("package.yaml", PlanAction::Edit, "題名を変える")] };
        let p = FakePlanner(Mutex::new(vec![good]), Mutex::new(vec![]));
        let plan = rt.block_on(make_plan(&p, "x", &existing, "")).unwrap();
        assert!(plan.rejected.is_empty());
        assert_eq!(p.1.lock().unwrap().len(), 1);

        // 立て直しても通らなければ理由つきで諦める (3 回目は呼ばない)。
        let p = FakePlanner(Mutex::new(vec![bad.clone(), bad]), Mutex::new(vec![]));
        let err = rt.block_on(make_plan(&p, "x", &existing, "")).unwrap_err();
        assert!(matches!(&err, PlanError::Invalid(e) if e[0].contains("action を edit")), "{err}");
        assert_eq!(p.1.lock().unwrap().len(), 2);
    }

    #[test]
    fn staging_copies_only_yaml_as_lf_and_takes_asset_names_from_the_package() {
        let src = scratch("stage");
        let dir = src.with_extension("stage");
        let st = Staging::prepare(&src, &dir).unwrap();
        assert!(dir.join("scenarios/main.yaml").is_file());
        assert!(!dir.join(".env").exists(), "YAML 以外は写さない");
        assert!(!dir.join("images").exists(), "メディアは写さない");
        assert!(!st.read("scenarios/main.yaml").unwrap().contains('\r'), "中身は LF");
        assert_eq!(st.original("scenarios/main.yaml").unwrap(), st.read("scenarios/main.yaml").unwrap());
        let v = st.vocabulary();
        assert!(v.ids["images"].iter().any(|i| i.name == "hall.webp"), "アセット名は元のパッケージから");
        // パッケージと重なる一時フォルダは拒否 (作り直しで消してはいけないものを消さない)。
        assert!(Staging::prepare(&src, &src).is_err());
        assert!(Staging::prepare(&src, &src.join("scenarios")).is_err());
        assert!(Staging::prepare(&src, &src.join("x")).is_err());
        st.discard();
        assert!(!dir.exists());
    }

    #[test]
    fn files_are_written_one_by_one_and_later_files_see_earlier_ids() {
        let src = scratch("build");
        let st = Staging::prepare(&src, &src.with_extension("st")).unwrap();
        // 計画は scenario → character の順に書かれていても、character が先に書かれる。
        let plan = vec![
            item("scenarios/main.yaml", PlanAction::Edit, "alice を cast と hall の present に"),
            item("characters/alice.yaml", PlanAction::Create, "司書のアリスを作る"),
        ];
        let mut turns = sd(r"(?m)^name: alice$", "name: アリス", "アリスを書いた");
        turns.extend(sd(r"(?m)^locations:$", "cast: [alice]\nlocations:", "cast に足した"));
        turns.extend(sd(r"(?m)^    description: 広間$", "    description: 広間\n    present: [alice]", "present に足した"));
        let chat = Scripted::new(turns);
        let (out, lines) = run(&chat, "司書のアリスを足して", &plan, &st, &AtomicBool::new(false));

        assert_eq!(out.files.len(), 2, "{lines:?}");
        assert_eq!(out.files[0].path, "characters/alice.yaml");
        assert_eq!(out.files[0].before, None);
        assert!(out.files[0].after.contains("name: アリス"), "雛形から sd で埋めた: {}", out.files[0].after);
        assert!(out.files[1].after.contains("cast: [alice]"));
        assert!(out.stopped.is_none() && out.failed.is_empty() && out.not_run.is_empty());
        // 2 ファイル目 (scenario) の素材には、1 ファイル目で**書いた**キャラの表示名が id 表に
        // 載っている = 一時フォルダが前の結果を次へ運んだ (1 ファイル目の時点では雛形の名前)。
        let systems = chat.first_systems();
        assert!(systems[1].contains("`alice` (アリス)"), "次のファイルの id 表に前のファイルの結果: {}", systems[1]);
        assert!(systems[0].contains("`alice` (alice)") && !systems[0].contains("アリス"), "1 ファイル目は雛形の時点");
        // ファイルごとに文脈を新しく組む (前のファイルの会話を積まない)。
        assert!(chat.seen.lock().unwrap().iter().filter(|m| m.len() == 2).count() == 2);
        // 計画全体と役割が指示に載る。
        let users: Vec<String> = chat.seen.lock().unwrap().iter().filter(|m| m.len() == 2).map(|m| m[1].content.clone()).collect();
        assert!(users[1].contains("✓ `characters/alice.yaml`") && users[1].contains("▶ `scenarios/main.yaml`"), "{}", users[1]);
        assert!(users[0].contains("雛形から新しく作られました"));
        // 層 2 はきれい (cast と present がそろった)。
        assert!(out.package_issues.is_empty(), "{:?}", out.package_issues);
        // **ディスクには何も書いていない** (書くのは作者)。
        assert!(!src.join("characters/alice.yaml").exists());
        assert!(!std::fs::read_to_string(src.join("scenarios/main.yaml")).unwrap().contains("alice"));
        assert!(lines.iter().any(|l| l.starts_with("▶ characters/alice.yaml")) && lines.iter().any(|l| l.starts_with("scenarios/main.yaml: sd")));
    }

    #[test]
    fn cross_file_issues_the_build_made_are_repaired_in_the_file_they_belong_to() {
        let src = scratch("repair");
        // 作者の持ち込み: キャラ定義の欄名の typo (= 開始時から在る層 2 の指摘)。
        std::fs::write(src.join("characters/old.yaml"), "name: 古株\nstat: { hp: 1 }\n").unwrap();
        let st = Staging::prepare(&src, &src.with_extension("st")).unwrap();
        assert!(st.baseline_issue_count() > 0, "持ち込みの指摘がある前提");
        let plan = vec![item("scenarios/main.yaml", PlanAction::Edit, "ghost を cast に")];
        let mut turns = sd(r"(?m)^locations:$", "cast: [ghost]\nlocations:", "ghost を足した"); // 居ないキャラ = 層 2 のエラー
        turns.extend(sd(r"cast: \[ghost\]", "cast: [moka]", "在るキャラにそろえた")); // 修復の周
        let chat = Scripted::new(turns);
        let (out, lines) = run(&chat, "仲間を足して", &plan, &st, &AtomicBool::new(false));

        let f = &out.files[0];
        assert!(f.repaired, "{lines:?}");
        assert!(f.after.contains("cast: [moka]"), "{}", f.after);
        assert!(f.summary.contains("(修復)"));
        assert!(out.package_issues.is_empty(), "この組み立てが生んだ指摘は直った: {:?} / baseline {:?}", out.package_issues, st.baseline);
        assert!(out.preexisting_issues > 0, "持ち込みの指摘は数えるだけ");
        // 修復の素材には層 2 の指摘そのものが載り、持ち込み分 (old.yaml) は載らない。
        let last_user = chat
            .seen
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|m| m.len() == 2)
            .map(|m| m[1].content.clone())
            .unwrap();
        assert!(last_user.contains("ghost") && !last_user.contains("old.yaml"), "{last_user}");
        // 持ち込みの指摘のファイル (計画外) は触っていない。
        assert!(out.files.iter().all(|f| f.path != "characters/old.yaml"));
        // ループの内訳: 最初の回と修復の回が順に残り、合計が組み立ての合計と一致する (2026-11-24)。
        let kinds: Vec<LoopKind> = f.loops.iter().map(|l| l.kind).collect();
        assert_eq!(kinds, vec![LoopKind::Main, LoopKind::Repair]);
        assert_eq!(f.loops.iter().map(|l| l.iterations).sum::<u32>(), f.iterations);
        assert_eq!(f.loops.iter().map(|l| l.prompt_tokens).sum::<u64>(), out.prompt_tokens);
        assert_eq!(f.loops.iter().map(|l| l.completion_tokens).sum::<u64>(), out.completion_tokens);
        assert_eq!(f.loops.iter().map(|l| l.cache_read).sum::<u64>(), out.cache_read);
        assert!(f.loops.iter().all(|l| l.prompt_tokens > 0), "どの回にもトークンが載る: {:?}", f.loops);
    }

    /// 【雛形のまま】spec 38 Phase B の実測 (2026-11-23、grok-4.3): 新しいキャラのファイルを 2 つとも
    /// 埋められず (`(?m)` の無い `^…$` が一致しないまま諦めた)、雛形のまま終わった。新規は
    /// `changed` が常に真なので、結果は「5 ファイル書けた・指摘 0」に見えていた。
    /// 雛形のままなら `template_only` で言い、新しい文脈で 1 回だけやり直す。
    #[test]
    fn a_new_file_left_as_its_template_is_retried_once_and_reported() {
        let src = scratch("tpl");
        let st = Staging::prepare(&src, &src.with_extension("st")).unwrap();
        let plan = vec![item("characters/alice.yaml", PlanAction::Create, "司書のアリス")];

        // 1 回目は何もせずに終わる → やり直しで埋まる。
        let mut turns = vec![text("一致しないので諦めます")];
        turns.extend(sd(r#"profile: """#, r#"profile: "司書""#, "埋めた"));
        let chat = Scripted::new(turns);
        let (out, lines) = run(&chat, "司書を足して", &plan, &st, &AtomicBool::new(false));
        let f = &out.files[0];
        assert!(lines.iter().any(|l| l.contains("やり直し: characters/alice.yaml")), "{lines:?}");
        assert!(!f.template_only && f.repaired, "やり直しで埋まった");
        let kinds: Vec<LoopKind> = f.loops.iter().map(|l| l.kind).collect();
        assert_eq!(kinds, vec![LoopKind::Main, LoopKind::Redo], "やり直しは修復と区別して残る");
        assert!(f.after.contains("司書"), "{}", f.after);
        let retry = chat.seen.lock().unwrap().iter().rev().find(|m| m.len() == 2).map(|m| m[1].content.clone()).unwrap();
        assert!(retry.contains("雛形のまま終わりました") && retry.contains("司書のアリス"), "最初の指示から: {retry}");
        st.discard();

        // やり直しても埋まらなければ、雛形のままだと結果で言う (2 回目のやり直しはしない)。
        let src = scratch("tpl2");
        let st = Staging::prepare(&src, &src.with_extension("st")).unwrap();
        let chat = Scripted::new(vec![text("無理"), text("やはり無理")]);
        let (out, _) = run(&chat, "司書を足して", &plan, &st, &AtomicBool::new(false));
        assert!(out.files[0].template_only && out.files[0].changed, "新規は changed でも雛形のまま");
        assert_eq!(chat.seen.lock().unwrap().len(), 2, "やり直しは 1 回だけ");
        st.discard();
    }

    #[test]
    fn cancelling_keeps_the_files_already_written() {
        let src = scratch("cancel");
        let st = Staging::prepare(&src, &src.with_extension("st")).unwrap();
        let plan = vec![
            item("characters/alice.yaml", PlanAction::Create, "アリス"),
            item("scenarios/main.yaml", PlanAction::Edit, "cast に"),
        ];
        let cancel = AtomicBool::new(false);
        // 1 ファイル目の報告の周で取り消しが押された想定: 台本の最後に取り消しを立てる fake は
        // 持たないので、1 ファイル目を書き終えた直後に立てる (周回境界で効く)。
        struct CancelAfter<'a>(Scripted, &'a AtomicBool, usize);
        impl ToolChat for CancelAfter<'_> {
            async fn chat(&self, m: Vec<ChatMessage>, t: Vec<ToolSpec>) -> Result<ChatTurn, LlmError> {
                let r = self.0.chat(m, t).await;
                if self.0.seen.lock().unwrap().len() >= self.2 {
                    self.1.store(true, Ordering::Relaxed);
                }
                r
            }
            async fn wrap_up(&self, m: Vec<ChatMessage>, t: Vec<ToolSpec>) -> Result<ChatTurn, LlmError> {
                self.0.wrap_up(m, t).await
            }
        }
        let chat = CancelAfter(Scripted::new(sd(r#"profile: """#, r#"profile: "司書""#, "書いた")), &cancel, 3);
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let out = rt
            .block_on(run_build(&chat, "x", &plan, &st, &cancel, &mut |_| {}))
            .unwrap();
        assert_eq!(out.stopped, Some(Stopped::Cancel));
        assert_eq!(out.files.len(), 1, "書き終えたファイルは残る");
        assert!(out.files[0].after.contains("司書"));
        assert_eq!(out.not_run, vec!["scenarios/main.yaml".to_string()]);
    }

    #[test]
    fn applying_writes_only_selected_unconflicted_files_in_their_line_endings() {
        let src = scratch("apply");
        std::fs::write(src.join(".lorekeel_source.json"), "{}").unwrap();
        let st = Staging::prepare(&src, &src.with_extension("st")).unwrap();
        let plan = vec![
            item("characters/alice.yaml", PlanAction::Create, "アリス"),
            item("characters/moka.yaml", PlanAction::Edit, "profile を書く"),
            item("scenarios/main.yaml", PlanAction::Edit, "cast に"),
        ];
        let mut turns = sd(r#"profile: """#, r#"profile: "司書""#, "a");
        turns.extend(sd(r#"profile: """#, r#"profile: "看板娘""#, "b"));
        turns.extend(sd(r"(?m)^locations:$", "cast: [alice, moka]\nlocations:", "c"));
        let chat = Scripted::new(turns);
        let (out, _) = run(&chat, "x", &plan, &st, &AtomicBool::new(false));
        assert_eq!(out.files.len(), 3);

        // 実行中に作者が moka をディスクで書き換えた = 衝突。
        std::fs::write(src.join("characters/moka.yaml"), "name: モカ\nprofile: \"手で書いた\"\n").unwrap();
        let all: Vec<String> = out.files.iter().map(|f| f.path.clone()).collect();
        let rep = apply_build(&src, &out.files, &all, true, &[".lorekeel_source.json"]);
        assert_eq!(rep.conflicts, vec!["characters/moka.yaml".to_string()]);
        assert!(std::fs::read_to_string(src.join("characters/moka.yaml")).unwrap().contains("手で書いた"), "黙って上書きしない");
        assert!(rep.written.contains(&"characters/alice.yaml".to_string()));
        let main = std::fs::read_to_string(src.join("scenarios/main.yaml")).unwrap();
        assert!(main.contains("cast: [alice, moka]\r\n"), "CRLF のファイルは CRLF で書く: {main:?}");
        assert!(!std::fs::read_to_string(src.join("characters/alice.yaml")).unwrap().contains('\r'), "新規は LF");
        assert!(rep.forked && !src.join(".lorekeel_source.json").exists(), "書けたら fork");

        // 選んだものだけ書く / 新規が既にできていれば衝突。
        let src2 = scratch("apply2");
        let rep2 = apply_build(&src2, &out.files, &["characters/alice.yaml".to_string()], false, &[]);
        assert_eq!(rep2.written, vec!["characters/alice.yaml".to_string()]);
        assert!(!std::fs::read_to_string(src2.join("scenarios/main.yaml")).unwrap().contains("cast"));
        let rep3 = apply_build(&src2, &out.files, &["characters/alice.yaml".to_string()], false, &[]);
        assert_eq!(rep3.conflicts, vec!["characters/alice.yaml".to_string()], "同名ができていたら書かない");
        assert!(!rep3.forked);
    }
}
