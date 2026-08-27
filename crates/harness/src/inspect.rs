//! パッケージの検分 (`play lint <dir>`)。**LLM 呼び出しゼロ・API キー不要**の静的検査。
//!
//! 動機は生成の検証ループ。`package_spec.md` を渡して LLM にパッケージを書かせる流儀では、
//! 「作らせる → 検める」を何度も回すことになるが、従来これを見る唯一の経路は
//! **アプリに入れて新しいゲームを始める** (開幕の ⚠) だった — API キーが要り、
//! ターンを 1 回消費する。生成ループには重すぎる。
//!
//! **機構は全部既にある** (`unknown_key_lints` / `Scenario::validate` / `Scenario::lints` /
//! `manifest_lints` / `character_lints`)。ここはそれらを 1 つの入口へ集めるだけで、
//! 新しい検査規則は作らない。
//!
//! **境界 (これが言えないこと)**: `validate` も `lints` も**各機構を単独で**検査するので、
//! 独立に正しい機構どうしの相互作用は原理的に捕まらない。実例 (2026-07-09): 同じフラグに
//! `flag_hints` (GM に見せて立てさせる) と `hidden_flags` (語彙から隠す) を併用すると、
//! GM がそのフラグを知り得ず真エンドが到達不能になる — validate は無反応で、発見は
//! **全ゴールへの到達経路を手でたどる**ことでしか起きなかった。緑は「読める」であって
//! 「遊べる」ではない。

use std::path::Path;

use crate::error::HarnessError;

/// 1 対象ぶんの検分結果。
#[derive(Debug, Clone, Default)]
pub struct InspectReport {
    /// 表示名 (与えられたパス)。
    pub target: String,
    /// **読み込みを拒否する**破れ (幻フラグ・幻 goal 等の閉世界の破れ、parse 失敗、file 不在)。
    pub errors: Vec<String>,
    /// 非 fatal な作者向け警告 (未知キー・死んだ参照・意図どおり動かない書き方)。
    pub warnings: Vec<String>,
}

impl InspectReport {
    fn new(target: &Path) -> Self {
        Self { target: target.display().to_string(), ..Self::default() }
    }

    /// エラーも警告も無いか。
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty() && self.warnings.is_empty()
    }
}

/// `characters/*.yaml` を 1 枚ずつ検分する (ファイル名を接頭に付ける)。
///
/// **load より先に**呼ぶ — シナリオ側が load に失敗しても、キャラ定義の欄名の誤りは見えてほしい
/// (生成物では両方同時に壊れていることが普通にある)。
fn character_warnings(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir.join("characters")) else {
        return out; // characters/ が無いのは正常 (inline キャストのパッケージ)
    };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort(); // 決定論的な出力順 (read_dir の順は OS 依存)
    for path in paths {
        if !matches!(path.extension().and_then(|e| e.to_str()), Some("yaml" | "yml")) {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&path) else { continue };
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
        out.extend(
            crate::package::character_lints(&src)
                .into_iter()
                .map(|w| format!("characters/{name}: {w}")),
        );
    }
    out
}

/// シナリオ file の生テキストに未知キー lint を掛ける (`id: path` を接頭に付ける)。
///
/// campaign の各モジュールは `load_module_injected` が生テキストを返さないため、
/// これまで**未知キー lint の射程外**だった (`LoadedCampaignPackage.warnings` の注記どおり)。
/// 検分側は自分で読むので、ここで塞げる。
fn scenario_key_warnings(path: &Path, label: &str) -> Vec<String> {
    let Ok(src) = std::fs::read_to_string(path) else {
        return vec![format!("{label}: 読み込めません ({})", path.display())];
    };
    gm_core::unknown_key_lints(&src).into_iter().map(|w| format!("{label}: {w}")).collect()
}

/// パッケージフォルダを検分する (単発 entry / campaign entry の両対応)。
///
/// 検査の順は「壊れていても見えるもの」から: characters → manifest → entry/モジュール。
pub fn inspect_package(dir: &Path) -> InspectReport {
    let mut report = InspectReport::new(dir);
    report.warnings.extend(character_warnings(dir));

    let manifest = match crate::package::read_manifest(dir) {
        Ok(m) => m,
        Err(e) => {
            report.errors.push(describe(&e));
            return report; // manifest が読めなければ entry が決まらない
        }
    };

    if crate::package::is_campaign_entry(&manifest.entry) {
        match crate::package::load_campaign_package(dir) {
            Ok(loaded) => {
                report.warnings.extend(loaded.warnings);
                // **全モジュール**を歩く (load_campaign_package が組むのは開始モジュールだけ)。
                for (id, rel) in &loaded.campaign.modules {
                    let label = format!("modules.{id}");
                    report.warnings.extend(scenario_key_warnings(&dir.join(rel), &label));
                    match crate::campaign::load_module_injected(
                        &loaded.campaign,
                        dir,
                        &loaded.manifest,
                        id,
                    ) {
                        Ok(scenario) => report
                            .warnings
                            .extend(crate::scenario_lint_messages(&scenario)
                                .into_iter()
                                .map(|l| format!("{label}: {l}"))),
                        Err(e) => report.errors.push(format!("{label}: {}", describe(&e))),
                    }
                }
            }
            Err(e) => report.errors.push(describe(&e)),
        }
    } else {
        match crate::package::load_package(dir) {
            // load_package の warnings は manifest lint + entry の未知キーまで。
            // `Scenario::lints` (死んだ参照・専権フラグへの flag_hint 等) はここで足す —
            // campaign 分岐は元から足していたのに単発分岐だけ抜けていた既存の穴
            // (app の開幕 ⚠ は別経路で足すので隠れていた。spec 28 Phase B で発見)。
            Ok(loaded) => {
                report.warnings.extend(loaded.warnings);
                report.warnings.extend(
                    crate::scenario_lint_messages(&loaded.scenario)
                        .into_iter()
                        .map(|l| format!("{}: {l}", loaded.manifest.entry)),
                );
            }
            Err(e) => report.errors.push(describe(&e)),
        }
    }
    report
}

/// 単体のシナリオ file を検分する (パッケージに入れる前の下書き用)。
///
/// `characters/` や `package.yaml` を伴わないので **cast 注入も package 注入も掛からない**。
/// 外部キャラ前提の盤面はここでは「幻 entity」に見えることがある — フォルダに組んでから
/// [`inspect_package`] で見るのが本筋で、こちらは形の早見。
pub fn inspect_scenario_file(path: &Path) -> InspectReport {
    let mut report = InspectReport::new(path);
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            report.errors.push(format!("読み込めません: {e}"));
            return report;
        }
    };
    report.warnings.extend(gm_core::unknown_key_lints(&src));
    match serde_yaml::from_str::<gm_core::Scenario>(&src) {
        Ok(scenario) => {
            report.errors.extend(scenario.validate().iter().map(|e| format!("{e:?}")));
            report.warnings.extend(crate::scenario_lint_messages(&scenario));
        }
        Err(e) => report.errors.push(format!("YAML として読めません: {e}")),
    }
    report
}

/// パスの形から検分の入口を選ぶ (フォルダ = パッケージ / file = シナリオ単体)。
pub fn inspect(path: &Path) -> InspectReport {
    if path.is_dir() {
        inspect_package(path)
    } else {
        inspect_scenario_file(path)
    }
}

/// `HarnessError` を 1 行に畳む (source 連鎖まで出す — 「読み込めません」だけでは直せない)。
fn describe(err: &HarnessError) -> String {
    let mut out = err.to_string();
    let mut cause: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(err);
    while let Some(inner) = cause {
        out.push_str(": ");
        out.push_str(&inner.to_string());
        cause = inner.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages").join(name)
    }

    /// 【偽陽性ゼロ】同梱パッケージは単発も campaign も検分で無傷であること。
    /// ここが鳴ると検分器自身が信用を失う (作者は警告を読まなくなる)。
    #[test]
    fn bundled_packages_are_clean_under_inspection() {
        for name in ["friday_lemmon", "lakeside_manor", "dice_trial", "escape"] {
            let r = inspect_package(&pkg(name));
            assert!(r.errors.is_empty(), "{name}: {:?}", r.errors);
            assert!(r.warnings.is_empty(), "{name}: {:?}", r.warnings);
            assert!(r.is_clean());
        }
    }

    /// 【campaign の全モジュールを歩く】開始モジュールだけでなく、地図に載る全部を見る。
    /// escape は study → cellar / forest の 3 本立てで、従来 lint が届いていたのは開始だけ。
    #[test]
    fn campaign_inspection_walks_every_module() {
        let loaded = crate::package::load_campaign_package(&pkg("escape")).expect("読める");
        assert!(loaded.campaign.modules.len() >= 2, "複数モジュールの盤面であること");
        // 全モジュールが注入込みで組めること (どれか 1 本が壊れていれば errors に出る)。
        let r = inspect_package(&pkg("escape"));
        assert!(r.errors.is_empty(), "{:?}", r.errors);
    }

    /// 【キャラ定義の欄名 typo を名指しする】ここは長らく完全に無検査だった。
    /// `stats` を `stat` と書くと serde が黙って無視し、そのキャラは数値を 1 つも持たない。
    #[test]
    fn character_field_typos_are_named() {
        let broken = "name: アリス\nstat:\n  好感度: 0\nprofil: 人見知り\n";
        let w = crate::package::character_lints(broken);
        let joined = w.join(" / ");
        assert!(joined.contains("stat"), "未知キーを名指し: {joined}");
        assert!(joined.contains("profil"), "未知キーを名指し: {joined}");
        // 近い既知キーを提案する (綴りの揺れは「何が正しいか」まで言わないと直せない)。
        assert!(joined.contains("stats") && joined.contains("profile"), "提案を出す: {joined}");

        // 正しいキャラ定義では鳴らない (境界つき stat 宣言を含む)。
        let ok = "name: アリス\nstats:\n  好感度: { initial: 0, min: 0, max: 100 }\nprofile: 人見知り\ntaboos: {}\n";
        assert!(crate::package::character_lints(ok).is_empty(), "{:?}", crate::package::character_lints(ok));
    }

    /// 【シナリオ単体】フォルダに組む前の下書きも形だけは見られる。
    #[test]
    fn a_bare_scenario_file_reports_unknown_keys() {
        let dir = std::env::temp_dir().join("kataribe_inspect_poc");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("draft.yaml");
        // `entry` は Scenario の欄ではない (package.yaml のもの) = 典型的な混同。
        std::fs::write(
            &path,
            "title: 下書き\nentry: x\nstart: room\nlocations:\n  room:\n    description: 部屋\n",
        )
        .unwrap();
        let r = inspect_scenario_file(&path);
        assert!(r.warnings.iter().any(|w| w.contains("entry")), "{:?}", r.warnings);
        let _ = std::fs::remove_file(&path);
    }

    /// 【単発 entry も `Scenario::lints` を報告する】(spec 28 Phase B で発見した既存の穴)。
    /// campaign 分岐は `scenario_lint_messages` を足していたのに、単発分岐は
    /// `load_package` の warnings (= manifest lint + 未知キーのみ) しか見ておらず、
    /// 死んだ参照・専権フラグへの flag_hint 等の lint が **`play lint` では出ていなかった**
    /// (app の開幕 ⚠ は別経路で足すので隠れていた)。
    #[test]
    fn single_entry_package_reports_scenario_lints() {
        let dir = std::env::temp_dir()
            .join(format!("kataribe_inspect_lints_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("scenarios")).unwrap();
        std::fs::write(dir.join("package.yaml"), "title: t\nentry: scenarios/main.yaml\n").unwrap();
        // flag_hints をトリガー専権フラグに付ける = FlagHintOnAuthoredOnly (lint・非 fatal)。
        std::fs::write(
            dir.join("scenarios/main.yaml"),
            concat!(
                "title: 盤面\nstart: room\n",
                "goal: { kind: flag_is, key: done, value: true }\n",
                "locations:\n  room:\n    description: 部屋\n",
                "allowed_flags: [done]\n",
                "flag_hints:\n  done: 済んだら立つ\n",
                "triggers:\n  - id: t1\n    when: { kind: flag_is, key: done, value: true }\n",
                "    narration: ✦\n    effects:\n      - { op: set_flag, key: done, value: true }\n",
            ),
        )
        .unwrap();

        let r = inspect_package(&dir);
        assert!(r.errors.is_empty(), "lint は非 fatal のはず: {:?}", r.errors);
        assert!(
            r.warnings.iter().any(|w| w.contains("flag_hint") && w.contains("done")),
            "単発 entry の Scenario::lints が warnings に載る: {:?}",
            r.warnings
        );
        // 帰属: entry の相対パスが接頭に付く (エディタ層 2 のファイル紐づけの素)。
        assert!(
            r.warnings.iter().any(|w| w.starts_with("scenarios/main.yaml: ")),
            "entry パスで帰属できる形: {:?}",
            r.warnings
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
