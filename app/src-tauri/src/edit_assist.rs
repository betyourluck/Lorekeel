//! AI 編集 (spec 29 Phase C) — 道具 5 本の**実体** (`read` / `grep` / `sd` / `diff` / `spec`) と、
//! 語彙から system の表を描く純関数。ループそのものは [`harness::edit_assist`] (純関数 +
//! fake で固定) で、ここは編集ルートの中だけを触る [`harness::edit_assist::ToolExecutor`]。
//!
//! 境界 (spec 29 決定 1〜6):
//! - 読めるのは編集モードの一覧 (各フォルダ直下の YAML) だけ。対象は**作業バッファ**、他は
//!   ディスク。一覧に無いパスは #39 の作法で「できない理由 + 代わり (作るのは作者)」を返す。
//! - 書けるのは対象だけ (`sd` は `path` を受けない)。`apply` は直前の同引数 preview 成功が前提
//!   (`last_preview` の状態機械 = コードで強制)。
//! - `sd apply` の結果に診断 (`editor_lint::lint_text`) を添える。
//! - `diff` は初期バッファ vs 作業バッファ (作者の未保存分は初期側に含まれるので混ざらない)。
//! - `spec` は同梱の話題断片 (ファイルシステムに触れない)。
//! - 道具の写経元は Fuseforks `tools/edit.rs` / `tools/fs.rs` (MPL-2.0・同一作者)。`sd` の
//!   diff 上限・一致 0 件・無変更の文言はそのまま、containment は Kataribe の編集ルートを使う。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use harness::edit_assist::{Diag, ToolExecutor, ToolReply};
use serde_json::Value;

use crate::editor;
use crate::editor_lint;
use crate::editor_vocab::EditorVocabulary;

/// `read` / `grep` の 1 ファイル上限 (バイト)。
const MAX_FILE_BYTES: u64 = 512 * 1024;
/// 道具の返りの上限 (文字)。超える diff は preview / apply とも実行しない (Fuseforks の契約:
/// 切り詰めた diff は「何が変わったか」を会話に残せない)。
const MAX_OUTPUT_CHARS: usize = 12_000;
/// grep の文脈行の上限。
const MAX_CONTEXT: u64 = 3;

/// 編集モードの category → 診断の kind (`package` だけが `manifest` へ)。
pub fn kind_of_category(category: &str) -> &str {
    if category == "package" { "manifest" } else { category }
}

/// `list_files` の一覧から対象の kind を引く。一覧に無ければ None。
pub fn kind_of(files: &[editor::EditorFileEntry], rel: &str) -> Option<String> {
    files.iter().find(|f| f.rel_path == rel).map(|f| kind_of_category(&f.category).to_string())
}

// =============================================================================
// 語彙 → system の表 (純関数)
// =============================================================================

/// この kind から配線で辿れる文脈 (型) の集合。`Gate` / `Op` に届けばバリアント表も出す。
fn reachable_contexts(v: &EditorVocabulary, kind: &str) -> Vec<String> {
    let Some(root) = v.roots.get(kind) else { return Vec::new() };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut order: Vec<String> = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(ctx) = stack.pop() {
        if !seen.insert(ctx.clone()) {
            continue;
        }
        order.push(ctx.clone());
        for w in v.wiring.iter().filter(|w| w.parent == ctx) {
            for child in std::iter::once(&w.child).chain(w.child_tagged.iter()) {
                if !seen.contains(child) {
                    stack.push(child.clone());
                }
            }
        }
    }
    order
}

/// 既知キー表 (型から導出した語彙を Markdown に)。Gate / Op は kind → キーの表。
pub fn keys_table(v: &EditorVocabulary, kind: &str) -> String {
    let mut out = String::new();
    for ctx in reachable_contexts(v, kind) {
        match ctx.as_str() {
            "Gate" => {
                out.push_str("## Gate (`kind` の値 → 書けるキー)\n");
                for (k, fields) in &v.gate_variant_keys {
                    out.push_str(&format!("- `{k}`: {}\n", fields.iter().map(|f| format!("`{}`", f.name)).collect::<Vec<_>>().join(", ")));
                }
            }
            "Op" => {
                out.push_str("## StateOp (`op` の値 → 書けるキー)\n");
                for (k, fields) in &v.op_variant_keys {
                    out.push_str(&format!("- `{k}`: {}\n", fields.iter().map(|f| format!("`{}`", f.name)).collect::<Vec<_>>().join(", ")));
                }
            }
            _ => {
                let Some(items) = v.contexts.get(&ctx) else { continue };
                out.push_str(&format!("## {ctx}\n"));
                for it in items {
                    match &it.doc {
                        Some(d) => out.push_str(&format!("- `{}` — {}\n", it.name, d)),
                        None => out.push_str(&format!("- `{}`\n", it.name)),
                    }
                }
            }
        }
    }
    out
}

/// id 表 (パッケージ内の宣言から集めた id。説明は作者の付けた表示名)。
pub fn ids_table(v: &EditorVocabulary) -> String {
    let mut out = String::new();
    for (cat, items) in &v.ids {
        if items.is_empty() {
            continue;
        }
        out.push_str(&format!("## {cat}\n"));
        for it in items {
            match &it.doc {
                Some(d) => out.push_str(&format!("- `{}` ({})\n", it.name, d)),
                None => out.push_str(&format!("- `{}`\n", it.name)),
            }
        }
    }
    if out.is_empty() {
        out.push_str("(まだ宣言が無い)\n");
    }
    out
}

// =============================================================================
// 道具の実体
// =============================================================================

/// 1 回の AI 編集の作業状態。
pub struct EditSession {
    root: PathBuf,
    target_rel: String,
    kind: String,
    files: Vec<String>,
    initial: String,
    buffer: String,
    /// 直前に成功した preview の (pattern, replacement, case_insensitive)。apply の前提。
    last_preview: Option<(String, String, bool)>,
}

impl EditSession {
    pub fn new(root: &Path, target_rel: &str, kind: &str, files: Vec<String>, initial_text: &str) -> Self {
        Self {
            root: root.to_path_buf(),
            target_rel: target_rel.to_string(),
            kind: kind.to_string(),
            files,
            initial: initial_text.to_string(),
            buffer: initial_text.to_string(),
            last_preview: None,
        }
    }

    fn not_in_list(&self, rel: &str) -> String {
        format!(
            "`{rel}` は編集対象の一覧にありません。AI 編集は既存ファイルの書き換え専用で、新しいファイルは作れません (作るのは作者です)。読めるのは system に載せた一覧の YAML だけです。"
        )
    }

    /// 一覧の中のファイルを読む。対象は作業バッファ、他はディスク。
    fn read_rel(&self, rel: &str) -> Result<String, String> {
        if rel == self.target_rel {
            return Ok(self.buffer.clone());
        }
        if !self.files.iter().any(|f| f == rel) {
            return Err(self.not_in_list(rel));
        }
        let path = editor::resolve_in_root(&self.root, rel)?;
        let meta = path.metadata().map_err(|e| format!("`{rel}` を読めません: {e}"))?;
        if meta.len() > MAX_FILE_BYTES {
            return Err(format!("`{rel}` が大きすぎます (上限 {MAX_FILE_BYTES} bytes)。"));
        }
        std::fs::read_to_string(&path).map_err(|e| format!("`{rel}` を読めません: {e}"))
    }

    fn tool_read(&self, args: &Value) -> ToolReply {
        let rel = args.get("path").and_then(Value::as_str).unwrap_or(&self.target_rel).to_string();
        match self.read_rel(&rel) {
            Ok(text) => {
                let mut body = String::new();
                for (i, line) in text.lines().enumerate() {
                    body.push_str(&format!("{}: {}\n", i + 1, line));
                }
                if body.is_empty() {
                    body.push_str("(空のファイル)\n");
                }
                ToolReply { body: clip(body), ok: true }
            }
            Err(e) => ToolReply { body: e, ok: false },
        }
    }

    fn tool_grep(&self, args: &Value) -> ToolReply {
        let Some(pattern) = args.get("pattern").and_then(Value::as_str) else {
            return ToolReply { body: "引数 `pattern` が必要です。".into(), ok: false };
        };
        let ci = args.get("case_insensitive").and_then(Value::as_bool).unwrap_or(false);
        let count_only = args.get("count_only").and_then(Value::as_bool).unwrap_or(false);
        let context = args.get("context").and_then(Value::as_u64).unwrap_or(0);
        if context > MAX_CONTEXT {
            return ToolReply { body: format!("`context` は 0〜{MAX_CONTEXT} で指定してください (指定値: {context})。"), ok: false };
        }
        let regex = match regex::RegexBuilder::new(pattern).case_insensitive(ci).size_limit(1 << 20).build() {
            Ok(r) => r,
            Err(e) => return ToolReply { body: format!("正規表現として解釈できません: {e}\nパターンを直して再試行してください。"), ok: false },
        };
        let targets: Vec<String> = match args.get("path").and_then(Value::as_str) {
            Some(p) => {
                if p != self.target_rel && !self.files.iter().any(|f| f == p) {
                    return ToolReply { body: self.not_in_list(p), ok: false };
                }
                vec![p.to_string()]
            }
            None => self.files.clone(),
        };
        let mut out = String::new();
        let mut total = 0usize;
        let mut per_file: Vec<(String, usize)> = Vec::new();
        for rel in &targets {
            let Ok(text) = self.read_rel(rel) else { continue };
            let lines: Vec<&str> = text.lines().collect();
            let hits: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| regex.is_match(l)).map(|(i, _)| i).collect();
            if hits.is_empty() {
                continue;
            }
            total += hits.len();
            per_file.push((rel.clone(), hits.len()));
            if count_only {
                continue;
            }
            let ctx = context as usize;
            let mut shown: BTreeSet<usize> = BTreeSet::new();
            for &h in &hits {
                let lo = h.saturating_sub(ctx);
                let hi = (h + ctx).min(lines.len() - 1);
                for (i, line) in lines.iter().enumerate().take(hi + 1).skip(lo) {
                    if !shown.insert(i) {
                        continue;
                    }
                    let sep = if hits.contains(&i) { ':' } else { '-' };
                    out.push_str(&format!("{rel}{sep}{}{sep} {line}\n", i + 1));
                }
            }
        }
        if total == 0 {
            return ToolReply { body: format!("`{pattern}` に一致する行はありません。"), ok: true };
        }
        if count_only {
            let mut body = String::new();
            for (rel, n) in &per_file {
                body.push_str(&format!("{rel}: {n}\n"));
            }
            body.push_str(&format!("合計 {total} 件\n"));
            return ToolReply { body, ok: true };
        }
        ToolReply { body: clip(out), ok: true }
    }

    fn tool_sd(&mut self, args: &Value) -> ToolReply {
        if args.get("path").is_some() {
            return ToolReply {
                body: format!("`sd` は編集対象 (`{}`) だけを書き換えます。`path` は受けません — 他のファイルを直したいときは、そのファイルを開いた AI 編集で依頼してください。", self.target_rel),
                ok: false,
            };
        }
        let (Some(pattern), Some(replacement)) = (args.get("pattern").and_then(Value::as_str), args.get("replacement").and_then(Value::as_str)) else {
            return ToolReply { body: "引数 `pattern` / `replacement` が必要です。".into(), ok: false };
        };
        let apply = args.get("apply").and_then(Value::as_bool).unwrap_or(false);
        let ci = args.get("case_insensitive").and_then(Value::as_bool).unwrap_or(false);
        let key = (pattern.to_string(), replacement.to_string(), ci);
        if apply && self.last_preview.as_ref() != Some(&key) {
            return ToolReply {
                body: "apply の前に、同じ pattern / replacement / case_insensitive で preview (apply を省く) を通してください。差分を見てから書く、が手順です。".into(),
                ok: false,
            };
        }
        let regex = match regex::RegexBuilder::new(pattern).case_insensitive(ci).size_limit(1 << 20).build() {
            Ok(r) => r,
            Err(e) => return ToolReply { body: format!("正規表現として解釈できません: {e}\nパターンを直して再試行してください。"), ok: false },
        };
        let match_count = regex.find_iter(&self.buffer).count();
        if match_count == 0 {
            return ToolReply { body: format!("`{}` に一致はありません (`read` で現在の本文を確かめてください)。書き込みは行っていません。", self.target_rel), ok: false };
        }
        let replaced = regex.replace_all(&self.buffer, replacement).into_owned();
        if replaced == self.buffer {
            return ToolReply { body: format!("{match_count} 件が一致しましたが、置換後も内容が同一です。変更なし (書き込みは行っていません)。"), ok: false };
        }
        let diff = unified_diff(&self.target_rel, &self.buffer, &replaced);
        if diff.chars().count() > MAX_OUTPUT_CHARS {
            return ToolReply {
                body: format!("差分が大きすぎるため実行しません ({match_count} 件の置換で diff が上限 {MAX_OUTPUT_CHARS} 字を超えます)。パターンを具体化するか、範囲を分けて置換してください。"),
                ok: false,
            };
        }
        if apply {
            self.buffer = replaced;
            self.last_preview = None;
            let diags = self.diagnostics();
            let mut body = format!("適用済み: {match_count} 件を置換しました。\n{diff}\n");
            if diags.is_empty() {
                body.push_str("診断: 問題なし。\n");
            } else {
                body.push_str("診断:\n");
                for d in &diags {
                    let at = match (d.line, &d.path) {
                        (Some(l), _) => format!(" (行 {l})"),
                        (None, Some(p)) => format!(" ({p})"),
                        _ => String::new(),
                    };
                    body.push_str(&format!("- [{}]{at} {}\n", d.severity, d.message));
                }
            }
            ToolReply { body, ok: true }
        } else {
            self.last_preview = Some(key);
            ToolReply {
                body: format!("preview (未適用): {match_count} 件を置換します。この内容で良ければ同じ引数に `apply: true` を足して書き込んでください。\n{diff}"),
                ok: true,
            }
        }
    }

    fn tool_diff(&self) -> ToolReply {
        if self.initial == self.buffer {
            return ToolReply { body: "変更なし (初期バッファと作業バッファは同一)。".into(), ok: true };
        }
        ToolReply { body: clip(unified_diff(&self.target_rel, &self.initial, &self.buffer)), ok: true }
    }

    fn tool_spec(&self, args: &Value) -> ToolReply {
        let Some(topic) = args.get("topic").and_then(Value::as_str) else {
            return ToolReply { body: "引数 `topic` が必要です。".into(), ok: false };
        };
        let idx = harness::package_spec::index();
        match idx.topics.iter().find(|t| t.id == topic) {
            Some(t) => match harness::package_spec::slice(&t.slice) {
                Some(text) => ToolReply { body: harness::package_spec::expand(text), ok: true },
                None => ToolReply { body: format!("話題 `{topic}` の断片が見つかりません。"), ok: false },
            },
            None => {
                let ids: Vec<&str> = idx.topics.iter().map(|t| t.id.as_str()).collect();
                ToolReply { body: format!("`{topic}` という話題はありません。引けるのは: {}", ids.join(", ")), ok: false }
            }
        }
    }
}

impl ToolExecutor for EditSession {
    fn call(&mut self, name: &str, args: &Value) -> ToolReply {
        match name {
            "read" => self.tool_read(args),
            "grep" => self.tool_grep(args),
            "sd" => self.tool_sd(args),
            "diff" => self.tool_diff(),
            "spec" => self.tool_spec(args),
            other => ToolReply { body: format!("`{other}` という道具はありません。使えるのは read / grep / sd / diff / spec です。"), ok: false },
        }
    }

    fn diagnostics(&self) -> Vec<Diag> {
        match editor_lint::lint_text(&self.kind, &self.buffer) {
            Ok(list) => list
                .into_iter()
                .map(|d| Diag { severity: d.severity, message: d.message, line: d.line, path: d.path })
                .collect(),
            Err(e) => vec![Diag { severity: "error".into(), message: format!("診断に失敗: {e}"), line: None, path: None }],
        }
    }

    fn text(&self) -> String {
        self.buffer.clone()
    }
}

fn unified_diff(display: &str, before: &str, after: &str) -> String {
    similar::TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(2)
        .header(&format!("a/{display}"), &format!("b/{display}"))
        .to_string()
}

fn clip(s: String) -> String {
    if s.chars().count() <= MAX_OUTPUT_CHARS {
        return s;
    }
    let head: String = s.chars().take(MAX_OUTPUT_CHARS).collect();
    format!("{head}\n…(上限 {MAX_OUTPUT_CHARS} 字で切り詰め。path や pattern で絞ってください)\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lorekeel_edit_assist_{name}_{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(dir.join("scenarios")).unwrap();
        std::fs::create_dir_all(dir.join("characters")).unwrap();
        std::fs::write(dir.join("package.yaml"), "title: T\nentry: scenarios/main.yaml\nplayer:\n  stats: { hp: 10 }\n").unwrap();
        std::fs::write(dir.join("scenarios/main.yaml"), "title: T\nstart: hall\nlocations:\n  hall:\n    description: 広間\n").unwrap();
        std::fs::write(dir.join("characters/moka.yaml"), "name: モカ\nstats: { 好感度: 0 }\n").unwrap();
        std::fs::write(dir.join(".env"), "LLM_API_KEY=secret\n").unwrap();
        dir
    }

    fn session(dir: &Path) -> EditSession {
        EditSession::new(
            dir,
            "package.yaml",
            "manifest",
            vec!["package.yaml".into(), "scenarios/main.yaml".into(), "characters/moka.yaml".into()],
            "title: T\nentry: scenarios/main.yaml\nplayer:\n  stats: { hp: 10 }\n",
        )
    }

    /// 【境界】一覧に無いパス (`.env`・存在しない新規名) は #39 の作法で断り、対象は作業バッファ、
    /// 他は保存済みを返す。
    #[test]
    fn read_and_grep_stay_inside_the_listed_yaml_only() {
        let dir = scratch("read");
        let mut s = session(&dir);
        let env = s.call("read", &json!({"path": ".env"}));
        assert!(!env.ok && env.body.contains("新しいファイルは作れません") && !env.body.contains("secret"));
        let new = s.call("read", &json!({"path": "characters/new.yaml"}));
        assert!(!new.ok && new.body.contains("作るのは作者"));
        let up = s.call("read", &json!({"path": "../package.yaml"}));
        assert!(!up.ok);
        let other = s.call("read", &json!({"path": "characters/moka.yaml"}));
        assert!(other.ok && other.body.starts_with("1: name: モカ"));
        // 対象はバッファ (置換後に read すると置換後が見える)。
        s.call("sd", &json!({"pattern": "hp: 10", "replacement": "hp: 12"}));
        s.call("sd", &json!({"pattern": "hp: 10", "replacement": "hp: 12", "apply": true}));
        let target = s.call("read", &json!({}));
        assert!(target.body.contains("hp: 12") && !target.body.contains("hp: 10"));
        // grep は一覧全体 (対象はバッファ) を横断し、count_only は件数だけ。
        let g = s.call("grep", &json!({"pattern": "title"}));
        assert!(g.body.contains("package.yaml:1: title: T") && g.body.contains("scenarios/main.yaml:1: title: T"));
        let c = s.call("grep", &json!({"pattern": "hp: 12", "count_only": true}));
        assert!(c.body.contains("package.yaml: 1") && c.body.contains("合計 1 件"));
        let ctx = s.call("grep", &json!({"pattern": "start", "path": "scenarios/main.yaml", "context": 1}));
        assert!(ctx.body.contains("scenarios/main.yaml-1- title: T") && ctx.body.contains("scenarios/main.yaml:2: start: hall"));
        assert!(!s.call("grep", &json!({"pattern": "x", "context": 9})).ok);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 【sd の状態機械】preview 無しの apply は拒否、引数が違う apply も拒否、同引数なら通る。
    /// 全一致を置換し件数を返す。`path` は受けない。diff は初期 vs 作業。
    #[test]
    fn sd_requires_a_matching_preview_before_apply_and_reports_counts() {
        let dir = scratch("sd");
        let mut s = session(&dir);
        let early = s.call("sd", &json!({"pattern": "T", "replacement": "U", "apply": true}));
        assert!(!early.ok && early.body.contains("preview"));
        let pv = s.call("sd", &json!({"pattern": "(?m)^title: T$", "replacement": "title: U"}));
        assert!(pv.ok && pv.body.contains("1 件") && pv.body.contains("-title: T") && pv.body.contains("+title: U"));
        assert_eq!(s.text(), s.initial, "preview は書かない");
        let mismatch = s.call("sd", &json!({"pattern": "(?m)^title: T$", "replacement": "title: V", "apply": true}));
        assert!(!mismatch.ok, "引数が違う apply は拒否");
        let ap = s.call("sd", &json!({"pattern": "(?m)^title: T$", "replacement": "title: U", "apply": true}));
        assert!(ap.ok && ap.body.contains("適用済み: 1 件") && ap.body.contains("診断: 問題なし"));
        assert!(s.text().starts_with("title: U"));
        // apply 後は last_preview が消える (もう一度 apply するなら preview から)。
        assert!(!s.call("sd", &json!({"pattern": "(?m)^title: U$", "replacement": "title: W", "apply": true})).ok);
        // 0 件・無変更・path 指定。
        assert!(!s.call("sd", &json!({"pattern": "zzz", "replacement": "y"})).ok);
        assert!(!s.call("sd", &json!({"pattern": "U", "replacement": "U"})).ok);
        assert!(!s.call("sd", &json!({"pattern": "a", "replacement": "b", "path": "scenarios/main.yaml"})).ok);
        // 全一致置換 (2 箇所) と diff。
        s.call("sd", &json!({"pattern": "T\\b", "replacement": "Z"}));
        let d = s.call("diff", &json!({}));
        assert!(d.body.contains("-title: T") && d.body.contains("+title: U"), "{}", d.body);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 【診断が返りに載る】壊す置換を apply すると parse エラーが結果に付き、diagnostics() でも見える。
    #[test]
    fn apply_attaches_diagnostics_from_the_lint() {
        let dir = scratch("diag");
        let mut s = session(&dir);
        s.call("sd", &json!({"pattern": "stats: \\{ hp: 10 \\}", "replacement": "stats: { hp: 10"}));
        let ap = s.call("sd", &json!({"pattern": "stats: \\{ hp: 10 \\}", "replacement": "stats: { hp: 10", "apply": true}));
        assert!(ap.ok && ap.body.contains("- [error]"), "{}", ap.body);
        assert_eq!(s.diagnostics().len(), 1);
        assert_eq!(s.diagnostics()[0].severity, "error");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 【spec】話題断片を id で引け、列挙のマーカーは展開済み。無い id は一覧を添えて断る。
    #[test]
    fn spec_tool_returns_expanded_topic_slices() {
        let dir = scratch("spec");
        let mut s = session(&dir);
        let p = s.call("spec", &json!({"topic": "percentile"}));
        assert!(p.ok && p.body.contains("#### d100 ロールアンダー判定"));
        let none = s.call("spec", &json!({"topic": "nope"}));
        assert!(!none.ok && none.body.contains("percentile"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 【live】実経路 (実キー・実モデル) で 1 依頼を通す。同梱 `lakeside_manor` を一時フォルダへ
    /// 写し、package.yaml の HP を 12 にする依頼を回す。見るもの: 呼び出し列 / 報告 / 診断 /
    /// 本文の差分。`APP_ENV_PATH="$APPDATA/jp.lorekeel.app/.env" cargo test live_edit -- --ignored --nocapture`
    #[test]
    #[ignore = "実キー (LLM_* / EDITOR_LLM_*) が要る live テスト"]
    fn live_edit_manifest_hp_on_lakeside() {
        use harness::edit_assist as ea;
        match std::env::var("APP_ENV_PATH") {
            Ok(p) => {
                dotenvy::from_path_override(&p).expect("APP_ENV_PATH が読める");
            }
            Err(_) => {
                dotenvy::dotenv().ok();
            }
        }
        let Ok(base) = llm_client::LlmConfig::from_env() else {
            eprintln!("skip: LLM_* が無い");
            return;
        };
        let config = llm_client::LlmConfig::editor_from_env(&base).unwrap().unwrap_or(base);
        eprintln!("model={} provider={:?}", config.model, config.provider);
        let client = llm_client::LlmClient::new(config).unwrap();

        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/lakeside_manor");
        let dir = scratch("live");
        for rel in ["package.yaml", "scenarios/main.yaml"] {
            std::fs::copy(src.join(rel), dir.join(rel)).unwrap();
        }
        std::fs::remove_dir_all(dir.join("characters")).ok();
        let entries = editor::list_files(&dir);
        let files: Vec<String> = entries.iter().map(|e| e.rel_path.clone()).collect();
        let kind = kind_of(&entries, "package.yaml").unwrap();
        let initial = std::fs::read_to_string(dir.join("package.yaml")).unwrap();
        let vocab = crate::editor_vocab::build_vocabulary(&dir);
        let req = ea::EditRequest {
            kind: kind.clone(),
            target_rel: "package.yaml".into(),
            files: files.clone(),
            keys_table: keys_table(&vocab, &kind),
            ids_table: ids_table(&vocab),
            instruction: "主人公の HP を 12 にしてください。他は変えないでください。".into(),
            initial_text: initial.clone(),
        };
        let msgs = ea::build_messages(&req);
        eprintln!("system chars={} user chars={}", msgs[0].content.chars().count(), msgs[1].content.chars().count());
        let mut session = EditSession::new(&dir, "package.yaml", &kind, files, &initial);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let t0 = std::time::Instant::now();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let out = rt
            .block_on(ea::run_edit_loop(&client, msgs, ea::tool_specs(), &mut session, &initial, &cancel, &mut |l| eprintln!("  > {l}")))
            .expect("ループが通る");
        eprintln!(
            "--- {:.1}s iterations={} prompt={} completion={} cache_read={} stopped={:?}\n報告: {}\n診断: {:?}\n",
            t0.elapsed().as_secs_f32(), out.iterations, out.prompt_tokens, out.completion_tokens, out.cache_read, out.stopped, out.summary, out.diagnostics
        );
        eprintln!("{}", unified_diff("package.yaml", &initial, &out.text));
        assert!(out.changed, "本文が変わっていない");
        assert!(out.text.contains("HP: 12"), "HP が 12 になっていない:\n{}", out.text);
        assert!(!out.diagnostics.iter().any(|d| d.severity == "error"), "{:?}", out.diagnostics);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 【表】kind から配線で辿れる文脈だけが載り、scenario には Gate / Op の表が付く。
    #[test]
    fn keys_table_follows_the_wiring_from_the_kind_root() {
        let v = crate::editor_vocab::build_vocabulary(Path::new("nonexistent"));
        let sc = keys_table(&v, "scenario");
        assert!(sc.contains("## Scenario\n") && sc.contains("## Location\n") && sc.contains("## Gate (") && sc.contains("- `has_item`:"));
        assert!(!sc.contains("## Manifest\n"), "scenario から manifest は辿れない");
        let m = keys_table(&v, "manifest");
        assert!(m.contains("## Manifest\n") && m.contains("## PlayerDef\n") && !m.contains("## Trigger\n"));
        assert_eq!(kind_of_category("package"), "manifest");
        assert_eq!(kind_of_category("scenario"), "scenario");
        assert!(ids_table(&v).contains("宣言が無い"));
    }
}
