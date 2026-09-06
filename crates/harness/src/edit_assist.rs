//! AI 編集 (spec 29 Phase C) — 編集モードで **1 ファイルを対象に、道具で直させる**ループの
//! 純関数部。素材の組み立て ([`build_messages`])・道具の宣言 ([`tool_specs`])・話題断片の選択
//! ([`select_topics`])・往復のループ ([`run_edit_loop`]) を持ち、**道具の実体は持たない**
//! (ファイルの読み書きは app の編集ルートの責務 = [`ToolExecutor`] で依存性逆転。
//! `DeltaProposer` と同じ形で、PoC は fake で回す)。
//!
//! 規律 (spec 29 決定 1〜10): 編集のみ・対象 1 つ・`sd` は preview → apply・置換のたびに診断・
//! 周回上限 [`MAX_ITERATIONS`] + 上限の外に修復 1 周・反復検知は (道具, 引数, 結果) の一致・
//! 診断 error が残っても本文は差し替える (呼び出し側が `changed` と `diagnostics` を見せる)。

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

use llm_client::{ChatMessage, ChatTurn, LlmClient, LlmError, ToolSpec};
use serde::Serialize;
use serde_json::{json, Value};

use crate::package_spec::{self, Topic};

/// LLM 呼び出しの周回上限 (spec 29 決定 8)。修復 1 周・まとめ 1 周は**この外**。
///
/// **2026-09-06 に 12 → 20 へ** (`play edit` の実測: scenario へ challenge を 1 つ足す依頼が
/// 置換 3 箇所 = preview + apply で 6 周、read と spec と失敗 1 回を足して 12 周ちょうどで
/// 打ち切られた。中身は正しく診断ゼロだったのに「打ち切り」と報告された)。1 周の入力は
/// 履歴の再送で ~27K トークンだが 9 割以上がキャッシュ読みなので、上限を上げる代償は小さい。
pub const MAX_ITERATIONS: u32 = 20;

/// 道具 5 本の名前 (spec 29 A 節)。
pub const TOOL_NAMES: [&str; 5] = ["read", "grep", "sd", "diff", "spec"];

/// 素材 (app が編集ルートから集めて渡す)。**ここに無いものは system に入らない**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRequest {
    /// 編集モードのファイル種別 (`manifest` / `scenario` / `character` / `memoria` / `campaign`)。
    pub kind: String,
    /// 対象ファイル (編集ルート相対)。
    pub target_rel: String,
    /// 読める YAML の一覧 (対象を含む)。`read` / `grep` はこの閉集合の中だけ。
    pub files: Vec<String>,
    /// 型から導出した既知キー表 (app が語彙から描く)。
    pub keys_table: String,
    /// 他ファイルから集めた id 表 (app が語彙から描く)。
    pub ids_table: String,
    /// 作者の指示。
    pub instruction: String,
    /// 初期バッファ (エディタの現在本文、未保存込み)。
    pub initial_text: String,
}

/// 指示と本文から話題断片を選ぶ (辞書 = 最適化。漏れは `spec` 道具が拾う)。
/// 1 文字の語 (`*`) は指示にだけ当てる — 本文の YAML には `*` が別の意味で出うる。
pub fn select_topics<'a>(kind: &str, instruction: &str, text: &str) -> Vec<&'a Topic> {
    let instr = instruction.to_lowercase();
    let body = text.to_lowercase();
    package_spec::index()
        .topics
        .iter()
        .filter(|t| t.kind == kind)
        .filter(|t| {
            t.keywords.iter().any(|k| {
                let k = k.to_lowercase();
                instr.contains(&k) || (k.chars().count() >= 2 && body.contains(&k))
            })
        })
        .collect()
}

/// `spec` 道具で引ける話題の一覧 (この kind のもの。既に載せた話題は「(掲載済み)」と印す)。
pub fn topic_menu(kind: &str, included: &[&str]) -> String {
    let mut out = String::new();
    for t in package_spec::index().topics.iter().filter(|t| t.kind == kind) {
        let mark = if included.contains(&t.id.as_str()) { " (掲載済み)" } else { "" };
        out.push_str(&format!("- `{}`{mark} — {}\n", t.id, t.summary));
    }
    out
}

fn base_slices(kind: &str) -> String {
    let idx = package_spec::index();
    let mut out = String::new();
    for id in idx.common.iter().chain(idx.base.get(kind).into_iter().flatten()) {
        if let Some(text) = package_spec::slice(id) {
            out.push_str(&package_spec::expand(text));
            out.push('\n');
        }
    }
    out
}

/// system プロンプト (純関数)。構成は spec 29 B 節: 役割 + 規律 / 対象と一覧 / 常に効く仕様 /
/// 指示に関係する話題 / `spec` で引ける話題 / 既知キー表 / id 表。
pub fn system_prompt(req: &EditRequest, topics: &[&Topic]) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "あなたは Lorekeel (LLM をナレーターにした TRPG エンジン) のパッケージを編集するアシスタントです。\n\
         対象ファイルは 1 つだけ — `{}` (種別: {})。作者の指示に従って**この 1 ファイルだけ**を直します。\n\n",
        req.target_rel, req.kind
    ));
    s.push_str(
        "# 道具と手順\n\
         - まず `read` で本文を確かめ、必要なら `grep` で当たりを付ける (対象ファイルは現在の作業本文、他のファイルは保存済みの本文が返る)。\n\
         - 書き換えは `sd` だけ。**先に preview** (`apply` を省く) で差分と一致数を見て、同じ引数で `apply: true` を呼ぶ。preview を通していない apply は拒否される。**独立した置換が複数あるなら、1 周でまとめて preview し、次の周でまとめて apply してよい** (周回に上限がある)。\n\
         - `sd` の pattern は正規表現 (Rust regex 構文。`$1` でキャプチャ参照、リテラルの `$` は `$$`、`(?m)` で行頭行末、`(?s)` で `.` が改行に当たる)。**一致した箇所は全部置換**される — 一致数を見て意図と違えば pattern を絞る。\n\
         - **追加 (挿入) は短い一意な 1 行をアンカーにする** — 例: `(?m)^goals:$` を「新しいブロック + `goals:`」に置換する。長い複数行のリテラルを pattern にすると、空白や改行の違いで一致しない。既存の行を直すときも、一致数が 1 になる最短の pattern を選ぶ。\n\
         - `sd apply` の結果には診断 (parse エラー / 未知キー) が付く。error を残したまま終えない。\n\
         - `diff` で自分の累積の差分を確かめられる。`spec` で話題の仕様を引ける (下の一覧)。\n\
         - **新しいファイルは作れない** (作るのは作者)。対象以外のファイルは書き換えられない (読むのは可)。\n\
         - 下の表に無いキーは未知キーとして警告される。表に無い id は死んだ参照になる。**捏造しない**。\n\
         - 作者が書いた語り (description / narration / profile / world 等) は、指示に無い限り書き換えない。\n\
         - 終わったら道具を呼ばず、何をどう直したか (直せなかったなら何が残っているか) を 2〜3 行で報告する。\n\n",
    );
    s.push_str("# 対象と一覧\n");
    s.push_str(&format!("- 編集対象 (書き換えられるのはこれだけ): `{}`\n", req.target_rel));
    s.push_str("- 読める YAML (`read` / `grep` の範囲):\n");
    for f in &req.files {
        s.push_str(&format!("  - `{f}`\n"));
    }
    s.push('\n');
    s.push_str("# 仕様 (この種別に常に効くもの)\n\n");
    s.push_str(&base_slices(&req.kind));
    if !topics.is_empty() {
        s.push_str("\n# 仕様 (指示に関係する話題)\n\n");
        for t in topics {
            if let Some(text) = package_spec::slice(&t.slice) {
                s.push_str(&package_spec::expand(text));
                s.push('\n');
            }
        }
    }
    let included: Vec<&str> = topics.iter().map(|t| t.id.as_str()).collect();
    let menu = topic_menu(&req.kind, &included);
    if !menu.is_empty() {
        s.push_str("\n# `spec` で引ける話題 (必要なら `spec` 道具に id を渡す)\n");
        s.push_str(&menu);
    }
    s.push_str("\n# このファイルで書けるキー (エンジンの型から導出。**これ以外は未知キーとして警告される**)\n");
    s.push_str(&req.keys_table);
    s.push_str("\n# 参照できる id (パッケージ内の宣言から収集。**これ以外は死んだ参照**)\n");
    s.push_str(&req.ids_table);
    s
}

/// user メッセージ (対象の本文 + 指示)。
pub fn user_prompt(req: &EditRequest) -> String {
    format!(
        "# 対象 `{}` の現在の本文\n```yaml\n{}\n```\n\n# 指示\n{}",
        req.target_rel,
        req.initial_text.trim_end_matches('\n'),
        req.instruction.trim()
    )
}

/// messages を組む (話題の選択込み)。
pub fn build_messages(req: &EditRequest) -> Vec<ChatMessage> {
    let topics = select_topics(&req.kind, &req.instruction, &req.initial_text);
    vec![ChatMessage::system(system_prompt(req, &topics)), ChatMessage::user(user_prompt(req))]
}

/// 道具 5 本の宣言 (JSON Schema は手書き — schemars の対象は正本の型であって道具ではない)。
/// 失敗文言の作法 (Fuseforks #39) は道具の実体側 (app) が持つ。
pub fn tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "read".into(),
            description: "ファイルの本文を行番号つきで読む。path を省くと編集対象 (現在の作業本文)。他のファイルは保存済みの本文。読めるのは一覧の YAML だけ".into(),
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "相対パス。省略 = 編集対象" } },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "grep".into(),
            description: "正規表現に一致する行を探す (`パス:行: 内容`)。全文を読む前に当たりを付ける。path で 1 ファイルに絞れる。count_only で件数だけ".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "正規表現 (Rust regex 構文)" },
                    "path": { "type": "string", "description": "絞り込む相対パス。省略 = 一覧の全 YAML" },
                    "case_insensitive": { "type": "boolean", "description": "大文字小文字を無視 (既定 false)" },
                    "context": { "type": "integer", "description": "一致行の前後も返す行数 (0〜3、既定 0)" },
                    "count_only": { "type": "boolean", "description": "件数だけ返す (既定 false)" }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "sd".into(),
            description: "編集対象を正規表現で置換する (対象以外は不可)。既定は preview = 差分と一致数を返すだけで書かない。同じ引数で apply: true にすると作業本文へ書き、診断 (parse エラー / 未知キー) を添えて返す。preview 無しの apply は拒否".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "検索する正規表現 (Rust regex 構文)" },
                    "replacement": { "type": "string", "description": "置換文字列。$1 / $name でキャプチャ参照、リテラルの $ は $$" },
                    "apply": { "type": "boolean", "description": "true で書く。省略 = preview" },
                    "case_insensitive": { "type": "boolean", "description": "大文字小文字を無視 (既定 false)" }
                },
                "required": ["pattern", "replacement"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "diff".into(),
            description: "編集を始めた時点の本文と現在の作業本文の差分 (unified diff)。自分の累積の変更を確かめる".into(),
            parameters: json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        },
        ToolSpec {
            name: "spec".into(),
            description: "作者向け仕様の話題断片を引く。system の「spec で引ける話題」の id を渡す".into(),
            parameters: json!({
                "type": "object",
                "properties": { "topic": { "type": "string", "description": "話題の id" } },
                "required": ["topic"],
                "additionalProperties": false
            }),
        },
    ]
}

// =============================================================================
// ループ
// =============================================================================

/// LLM の 1 往復 (依存性逆転 — 実体は [`LlmClient::chat`]、PoC は scripted fake)。
pub trait ToolChat {
    fn chat(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolSpec>,
    ) -> impl Future<Output = Result<ChatTurn, LlmError>> + Send;
}

impl ToolChat for LlmClient {
    async fn chat(&self, messages: Vec<ChatMessage>, tools: Vec<ToolSpec>) -> Result<ChatTurn, LlmError> {
        LlmClient::chat(self, messages, tools).await
    }
}

/// 診断 1 件 (app の `EditorDiagnostic` の写し — harness は app の型を知らない)。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Diag {
    /// "error" | "warning"
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// 道具の返り。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolReply {
    pub body: String,
    pub ok: bool,
}

/// 道具の実体 (app の編集ルートが実装)。
pub trait ToolExecutor {
    fn call(&mut self, name: &str, args: &Value) -> ToolReply;
    /// 現在の作業本文の診断 (`lint_editor_text`)。
    fn diagnostics(&self) -> Vec<Diag>;
    /// 現在の作業本文。
    fn text(&self) -> String;
}

/// 打ち切りの理由。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stopped {
    Limit,
    Repeat,
    Cancel,
}

/// 進行ログ 1 行 (呼び出し 1 本)。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CallLog {
    pub tool: String,
    pub args_brief: String,
    pub ok: bool,
}

/// ループの結果 (spec 29 B 節の返り)。
#[derive(Debug, Clone, Serialize)]
pub struct EditOutcome {
    pub text: String,
    pub changed: bool,
    pub summary: String,
    pub calls: Vec<CallLog>,
    pub diagnostics: Vec<Diag>,
    pub iterations: u32,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_read: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
}

/// 進行ログ用の引数要約 (道具ごとに要点だけ)。
pub fn args_brief(name: &str, args: &Value) -> String {
    let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let clip = |t: String| -> String {
        if t.chars().count() > 60 { format!("{}…", t.chars().take(60).collect::<String>()) } else { t }
    };
    match name {
        "read" => clip(if s("path").is_empty() { "(対象)".into() } else { s("path") }),
        "grep" => clip(s("pattern")),
        "sd" => {
            let apply = args.get("apply").and_then(Value::as_bool).unwrap_or(false);
            clip(format!("{} {}", if apply { "apply" } else { "preview" }, s("pattern")))
        }
        "spec" => clip(s("topic")),
        _ => String::new(),
    }
}

fn add_usage(out: &mut EditOutcome, turn: &ChatTurn) {
    out.prompt_tokens += turn.usage.prompt;
    out.completion_tokens += turn.usage.completion;
    out.cache_read += turn.usage.cache_read;
}

/// 往復のループ。`chat` → 呼び出しを `exec` で実行 → 結果を積む → 繰り返し。呼び出しが空に
/// なったら終了 (本文 = 作者への報告)。終了後に error 診断が残っていれば**上限の外で修復 1 周**。
/// 反復検知 = 直前と同じ (道具, 引数, 結果) → `Stopped::Repeat`。`cancel` は周回境界で効く。
pub async fn run_edit_loop<C: ToolChat>(
    chat: &C,
    mut messages: Vec<ChatMessage>,
    tools: Vec<ToolSpec>,
    exec: &mut (dyn ToolExecutor + Send),
    initial_text: &str,
    cancel: &AtomicBool,
    progress: &mut (dyn FnMut(String) + Send),
) -> Result<EditOutcome, LlmError> {
    let mut out = EditOutcome {
        text: String::new(),
        changed: false,
        summary: String::new(),
        calls: Vec::new(),
        diagnostics: Vec::new(),
        iterations: 0,
        prompt_tokens: 0,
        completion_tokens: 0,
        cache_read: 0,
        stopped: None,
    };
    let mut last_key: Option<(String, String, String)> = None;
    let mut completed = false;

    // 1 周: 呼び出し列を実行して積む。反復を検知したら true を返す。
    fn execute_round(
        turn: &ChatTurn,
        exec: &mut (dyn ToolExecutor + Send),
        messages: &mut Vec<ChatMessage>,
        out: &mut EditOutcome,
        last_key: &mut Option<(String, String, String)>,
        progress: &mut (dyn FnMut(String) + Send),
    ) -> bool {
        messages.push(ChatMessage::assistant_tool_calls(
            turn.text.clone().unwrap_or_default(),
            turn.tool_calls.clone(),
        ));
        for call in &turn.tool_calls {
            let reply = exec.call(&call.name, &call.args);
            let brief = args_brief(&call.name, &call.args);
            // 失敗は理由の先頭行を添える (Phase E 実測: Grok が同じ失敗を 2 度繰り返して止まったとき、
            // 進行ログには ✗ しか無く、何に一致しなかったのかが CLI からも GUI からも読めなかった)。
            let why = if reply.ok {
                String::new()
            } else {
                let first: String = reply.body.lines().next().unwrap_or("").chars().take(80).collect();
                format!(" ✗ {first}")
            };
            progress(format!("{} {}{}", call.name, brief, why));
            out.calls.push(CallLog { tool: call.name.clone(), args_brief: brief, ok: reply.ok });
            let key = (call.name.clone(), call.args.to_string(), reply.body.clone());
            messages.push(ChatMessage::tool_result(&call.id, &call.name, reply.body));
            if last_key.as_ref() == Some(&key) {
                return true;
            }
            *last_key = Some(key);
        }
        false
    }

    for it in 0..MAX_ITERATIONS {
        if cancel.load(Ordering::Relaxed) {
            out.stopped = Some(Stopped::Cancel);
            break;
        }
        if it + 1 == MAX_ITERATIONS && !cancel.load(Ordering::Relaxed) {
            // 上限の最後の 1 周は**道具なしのまとめ** — 途中までの変更を捨てず、やり遂げた分を
            // 報告させる (実測: 上限に当たった依頼の中身は正しかったのに定型文の「打ち切り」が
            // 報告に差し替わり、失敗に見えた)。道具を渡さないので呼び出しは起こらない。
            messages.push(ChatMessage::user(
                "道具の呼び出しが上限に達しました。これ以上は書き換えられません。ここまでに何をどう直したか (残っていることがあれば何が残っているか) を 2〜3 行で報告してください。",
            ));
            progress("まとめの 1 周 (上限)".into());
            let turn = chat.chat(messages.clone(), Vec::new()).await?;
            out.iterations += 1;
            add_usage(&mut out, &turn);
            out.summary = turn.text.unwrap_or_default();
            out.stopped = Some(Stopped::Limit);
            break;
        }
        let turn = chat.chat(messages.clone(), tools.clone()).await?;
        out.iterations += 1;
        add_usage(&mut out, &turn);
        if turn.tool_calls.is_empty() {
            out.summary = turn.text.unwrap_or_default();
            completed = true;
            break;
        }
        if execute_round(&turn, exec, &mut messages, &mut out, &mut last_key, progress) {
            out.stopped = Some(Stopped::Repeat);
            break;
        }
    }

    // 修復 1 周 (上限の外・決定 8/9)。正常終了かつ error が残るときだけ。
    if completed && !cancel.load(Ordering::Relaxed) {
        let diags = exec.diagnostics();
        if diags.iter().any(|d| d.severity == "error") {
            let list: Vec<String> = diags.iter().map(|d| format!("- [{}] {}", d.severity, d.message)).collect();
            messages.push(ChatMessage::user(format!(
                "まだ壊れています (最終診断)。`sd` で直して、最後に報告してください:\n{}",
                list.join("\n")
            )));
            progress("修復の 1 周".into());
            let turn = chat.chat(messages.clone(), tools.clone()).await?;
            out.iterations += 1;
            add_usage(&mut out, &turn);
            if turn.tool_calls.is_empty() {
                if let Some(t) = turn.text.filter(|t| !t.trim().is_empty()) {
                    out.summary = t;
                }
            } else {
                execute_round(&turn, exec, &mut messages, &mut out, &mut last_key, progress);
            }
        }
    }

    out.text = exec.text();
    out.changed = out.text != initial_text;
    out.diagnostics = exec.diagnostics();
    if out.summary.trim().is_empty() {
        out.summary = match out.stopped {
            Some(Stopped::Limit) => format!("道具の呼び出しが上限 ({MAX_ITERATIONS} 周) に達したので打ち切りました (まとめの報告なし)。"),
            Some(Stopped::Repeat) => "同じ呼び出しが同じ結果で繰り返されたので打ち切りました。".into(),
            Some(Stopped::Cancel) => "取り消されました。".into(),
            None => "(報告なし)".into(),
        };
    }
    Ok(out)
}

// =============================================================================
// PoC
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use llm_client::{Finish, ToolCall, Usage};
    use std::sync::Mutex;

    fn req(kind: &str, instruction: &str, text: &str) -> EditRequest {
        EditRequest {
            kind: kind.into(),
            target_rel: if kind == "manifest" { "package.yaml".into() } else { "scenarios/main.yaml".into() },
            files: vec!["package.yaml".into(), "scenarios/main.yaml".into(), "characters/moka.yaml".into()],
            keys_table: "## Scenario\n- `title`\n- `start`\n".into(),
            ids_table: "## locations\n- `classroom` (放課後の教室)\n".into(),
            instruction: instruction.into(),
            initial_text: text.into(),
        }
    }

    /// 【素材の固定】system に入るのは 7 節だけで、渡した部品以外の文字列は現れない。
    /// 話題は指示の語で選ばれ (SAN → percentile)、無関係な話題は載らず `spec` の一覧に回る。
    #[test]
    fn system_is_composed_only_from_known_parts_and_topics_follow_the_instruction() {
        let r = req("scenario", "湖畔に SAN 判定の challenge を 1 つ足して", "title: 湖畔\nstart: hall\n");
        let msgs = build_messages(&r);
        assert_eq!(msgs.len(), 2);
        let sys = &msgs[0].content;
        for head in [
            "# 道具と手順",
            "# 対象と一覧",
            "# 仕様 (この種別に常に効くもの)",
            "# 仕様 (指示に関係する話題)",
            "# `spec` で引ける話題",
            "# このファイルで書けるキー",
            "# 参照できる id",
        ] {
            assert!(sys.contains(head), "{head} が無い");
        }
        // 種別の基本断片 (scenario 本体 + Gate + op) と共通 (大原則) が展開済みで載る。
        assert!(sys.contains("## scenarios/*.yaml (Scenario)"));
        assert!(sys.contains("## 大原則"));
        assert!(sys.contains("| `has_item` |"), "Gate の列挙が展開されている");
        assert!(!sys.contains("<!-- vocab:"));
        // 指示の「SAN」で percentile が選ばれ、掲載済みと印される。contests は載らず一覧に残る。
        assert!(sys.contains("#### d100 ロールアンダー判定"));
        assert!(sys.contains("`percentile` (掲載済み)"));
        assert!(!sys.contains("#### 対決 (`contests`)"));
        assert!(sys.contains("- `contests` — "));
        // 表と一覧はそのまま載る。本文と指示は user 側だけ。
        assert!(sys.contains("## locations\n- `classroom`"));
        assert!(sys.contains("- `characters/moka.yaml`"));
        assert!(!sys.contains("title: 湖畔"), "本文は system に入らない");
        assert!(msgs[1].content.contains("title: 湖畔") && msgs[1].content.contains("SAN 判定"));
        // manifest では scenario の断片も話題も出ない。
        let m = build_messages(&req("manifest", "主人公の hp を 12 に", "title: x\n"));
        assert!(m[0].content.contains("## package.yaml (PackageManifest)"));
        assert!(!m[0].content.contains("## scenarios/*.yaml"));
        assert!(!m[0].content.contains("`percentile`"));
        assert!(m[0].content.contains("- `facts_policy` — "));
    }

    /// 本文のキーからも話題が選ばれる (`contests:` が本文に在れば対決の断片が載る)。
    /// 1 文字の語 (`*`) は本文には当てない。
    #[test]
    fn topics_are_selected_from_text_keys_but_single_char_keywords_only_from_instruction() {
        let by_text = select_topics("scenario", "誤字を直して", "contests:\n  brawl: {}\n");
        assert!(by_text.iter().any(|t| t.id == "contests"));
        let star_in_text = select_topics("scenario", "誤字を直して", "cast: ['*']\n");
        assert!(!star_in_text.iter().any(|t| t.id == "wildcard"));
        let star_in_instruction = select_topics("scenario", "cast を * にして", "");
        assert!(star_in_instruction.iter().any(|t| t.id == "wildcard"));
    }

    #[test]
    fn tool_specs_cover_the_five_tools_with_object_schemas() {
        let specs = tool_specs();
        let names: Vec<&str> = specs.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, TOOL_NAMES);
        for t in &specs {
            assert_eq!(t.parameters["type"], "object", "{}", t.name);
            assert_eq!(t.parameters["additionalProperties"], false, "{}", t.name);
        }
        assert_eq!(tool_specs()[2].parameters["required"], json!(["pattern", "replacement"]));
    }

    // --- fake ---
    struct Scripted(Mutex<Vec<ChatTurn>>);
    impl ToolChat for Scripted {
        async fn chat(&self, _m: Vec<ChatMessage>, _t: Vec<ToolSpec>) -> Result<ChatTurn, LlmError> {
            let mut q = self.0.lock().unwrap();
            if q.is_empty() {
                return Ok(text_turn("(台本切れ)"));
            }
            Ok(q.remove(0))
        }
    }
    fn text_turn(t: &str) -> ChatTurn {
        ChatTurn { text: Some(t.into()), tool_calls: vec![], finish: Finish::Stop, usage: Usage { prompt: 10, completion: 2, cache_read: 4 } }
    }
    fn call_turn(calls: Vec<(&str, &str, Value)>) -> ChatTurn {
        ChatTurn {
            text: None,
            tool_calls: calls.into_iter().map(|(id, n, a)| ToolCall { id: id.into(), name: n.into(), args: a, thought_signature: None }).collect(),
            finish: Finish::ToolUse,
            usage: Usage { prompt: 100, completion: 5, cache_read: 0 },
        }
    }
    /// 道具の fake: sd apply で本文を書き換え、診断は本文に "broken" が在れば error。
    struct FakeExec { text: String, log: Vec<String> }
    impl ToolExecutor for FakeExec {
        fn call(&mut self, name: &str, args: &Value) -> ToolReply {
            self.log.push(name.to_string());
            match name {
                "read" => ToolReply { body: format!("1: {}", self.text), ok: true },
                "sd" => {
                    let apply = args["apply"].as_bool().unwrap_or(false);
                    if apply {
                        self.text = args["replacement"].as_str().unwrap_or("").to_string();
                        ToolReply { body: "適用済み".into(), ok: true }
                    } else {
                        ToolReply { body: "preview".into(), ok: true }
                    }
                }
                _ => ToolReply { body: "?".into(), ok: false },
            }
        }
        fn diagnostics(&self) -> Vec<Diag> {
            if self.text.contains("broken") {
                vec![Diag { severity: "error".into(), message: "壊れている".into(), line: Some(1), path: None }]
            } else {
                vec![]
            }
        }
        fn text(&self) -> String { self.text.clone() }
    }

    fn run(turns: Vec<ChatTurn>, exec: &mut FakeExec, cancel: &AtomicBool) -> EditOutcome {
        let chat = Scripted(Mutex::new(turns));
        let mut lines = Vec::new();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let initial = exec.text.clone();
        rt.block_on(run_edit_loop(
            &chat,
            vec![ChatMessage::system("s"), ChatMessage::user("u")],
            tool_specs(),
            exec,
            &initial,
            cancel,
            &mut |l| lines.push(l),
        ))
        .unwrap()
    }

    /// 【往復】read → sd preview → sd apply → 報告。呼び出しは順に実行され、本文が差し替わり、
    /// usage が合算され、報告が summary になる。
    #[test]
    fn loop_executes_calls_in_order_and_returns_the_report() {
        let mut exec = FakeExec { text: "hp: 10".into(), log: vec![] };
        let out = run(
            vec![
                call_turn(vec![("1", "read", json!({}))]),
                call_turn(vec![("2", "sd", json!({"pattern": "hp: 10", "replacement": "hp: 12"}))]),
                call_turn(vec![("3", "sd", json!({"pattern": "hp: 10", "replacement": "hp: 12", "apply": true}))]),
                text_turn("hp を 12 にしました。"),
            ],
            &mut exec,
            &AtomicBool::new(false),
        );
        assert_eq!(exec.log, ["read", "sd", "sd"]);
        assert_eq!(out.text, "hp: 12");
        assert!(out.changed);
        assert_eq!(out.summary, "hp を 12 にしました。");
        assert_eq!(out.iterations, 4);
        assert_eq!(out.prompt_tokens, 310);
        assert_eq!(out.cache_read, 4);
        assert!(out.stopped.is_none());
        assert_eq!(out.calls[2].args_brief, "apply hp: 10");
        assert!(out.diagnostics.is_empty());
    }

    /// 【反復検知】同じ (道具, 引数, 結果) が続いたら止める。preview → apply は引数が違うので止めない。
    #[test]
    fn repeat_of_identical_call_and_result_stops_the_loop() {
        let mut exec = FakeExec { text: "x".into(), log: vec![] };
        let out = run(
            vec![
                call_turn(vec![("1", "read", json!({}))]),
                call_turn(vec![("2", "read", json!({}))]),
                text_turn("never"),
            ],
            &mut exec,
            &AtomicBool::new(false),
        );
        assert_eq!(out.stopped, Some(Stopped::Repeat));
        assert_eq!(exec.log.len(), 2);
        assert!(out.summary.contains("繰り返された"));
    }

    /// 【上限】道具つきの周は MAX−1 回まで、最後の 1 周は**道具なしのまとめ** (報告が summary に
    /// なる = やり遂げた分が失敗に見えない)。chat は MAX 回で止まり、それ以上は呼ばれない。
    #[test]
    fn iteration_limit_ends_with_a_tool_free_wrap_up() {
        let mut exec = FakeExec { text: "x".into(), log: vec![] };
        let mut turns: Vec<ChatTurn> = (0..MAX_ITERATIONS - 1)
            .map(|i| call_turn(vec![(&format!("c{i}"), "read", json!({"path": format!("f{i}")}))]))
            .collect();
        turns.push(text_turn("ここまでで 2 箇所を直しました。"));
        turns.push(call_turn(vec![("never", "read", json!({}))]));
        let out = run(turns, &mut exec, &AtomicBool::new(false));
        assert_eq!(out.stopped, Some(Stopped::Limit));
        assert_eq!(out.iterations, MAX_ITERATIONS);
        assert_eq!(exec.log.len(), (MAX_ITERATIONS - 1) as usize, "まとめの周は道具を持たない");
        assert_eq!(out.summary, "ここまでで 2 箇所を直しました。");
    }

    /// 【修復 1 周】正常終了しても error 診断が残れば上限の外で 1 周だけ投げ、直れば diagnostics が空になる。
    #[test]
    fn repair_round_runs_once_outside_the_limit_when_errors_remain() {
        let mut exec = FakeExec { text: "ok".into(), log: vec![] };
        let out = run(
            vec![
                call_turn(vec![("1", "sd", json!({"pattern": "ok", "replacement": "broken"}))]),
                call_turn(vec![("2", "sd", json!({"pattern": "ok", "replacement": "broken", "apply": true}))]),
                text_turn("直しました (実は壊れている)"),
                // 修復の 1 周: apply で直す。
                call_turn(vec![("3", "sd", json!({"pattern": "broken", "replacement": "fixed", "apply": true}))]),
                text_turn("(呼ばれない)"),
            ],
            &mut exec,
            &AtomicBool::new(false),
        );
        assert_eq!(out.text, "fixed");
        assert!(out.diagnostics.is_empty(), "修復周で直った");
        assert_eq!(out.iterations, 4, "本線 3 + 修復 1");
        assert_eq!(out.summary, "直しました (実は壊れている)", "修復周に本文が無ければ前の報告を保つ");
        assert!(out.stopped.is_none());
    }

    /// 【診断が残っても差し替える】修復周でも直らなければ text は最新の作業本文で diagnostics つき。
    #[test]
    fn unrepaired_errors_still_return_the_edited_text_with_diagnostics() {
        let mut exec = FakeExec { text: "ok".into(), log: vec![] };
        let out = run(
            vec![
                call_turn(vec![("1", "sd", json!({"pattern": "ok", "replacement": "broken"}))]),
                call_turn(vec![("2", "sd", json!({"pattern": "ok", "replacement": "broken", "apply": true}))]),
                text_turn("done"),
                text_turn("直せませんでした。"),
            ],
            &mut exec,
            &AtomicBool::new(false),
        );
        assert_eq!(out.text, "broken");
        assert!(out.changed);
        assert_eq!(out.diagnostics.len(), 1);
        assert_eq!(out.summary, "直せませんでした。");
    }

    /// 【キャンセル】周回境界で効き、本文は現状のまま返る。
    #[test]
    fn cancel_flag_stops_at_the_next_boundary() {
        let mut exec = FakeExec { text: "x".into(), log: vec![] };
        let out = run(vec![call_turn(vec![("1", "read", json!({}))])], &mut exec, &AtomicBool::new(true));
        assert_eq!(out.stopped, Some(Stopped::Cancel));
        assert_eq!(out.iterations, 0);
        assert!(!out.changed);
    }
}
