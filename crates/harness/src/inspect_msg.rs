//! `Scenario::lints()` の作者向け文面 (app の開幕 ⚠ と `play lint` が共用する)。

use gm_core::Scenario;

/// `Scenario::lints()` を**作者向けの表示文**にする (非 fatal — load は拒否せず警告に留める。
/// fatal にすると配布済み content が受領側で死ぬ)。
///
/// **文面をここに置く理由**: 受け手は GUI の開幕 ⚠ でも `play lint` でも同じ「作者」で、
/// 言うべきことも同じ。app 側と CLI 側に別々の文面を持つと、片方だけ直したときに必ずずれる
/// (この関数は app から移設した)。lint は**何がダメか**でなく**何をすれば直るか**を言う。
pub fn scenario_lint_messages(scenario: &Scenario) -> Vec<String> {
    scenario
        .lints()
        .into_iter()
        .map(|l| match l {
            gm_core::ScenarioError::FlagHintOnAuthoredOnly { flag } => format!(
                "フラグ「{flag}」の flag_hint は GM に届きません（トリガー/challenge が立てる専権フラグのため）。\
                 GM に立てさせるならトリガー/challenge 側の set_flag を外し、筋書きで立てるならヒントを外してください"
            ),
            // spec 21 同梱: 幻の場所を指す location_is は永久に false = その Gate は死んでいる。
            gm_core::ScenarioError::UnknownLocationInGate { origin, at } => format!(
                "{origin} の location_is が、宣言されていない場所「{at}」を指しています。\
                 この条件は永久に成立しません（挑戦なら一度も選べず、出口なら通れません）。\
                 locations に無い名前です — 所持品を対象にするなら has_item を使ってください"
            ),
            // presence_is が cast に無いキャラを指すと、present_at が落とすので永久に偽
            // (present: false なら永久に真)。どちらも「書いたのに効かない」。
            gm_core::ScenarioError::UnknownEntityInGate { origin, entity } => format!(
                "{origin} の presence_is が、このシナリオが知らないキャラ「{entity}」を指しています。\
                 この条件は永久に成立しません（present: false なら逆に常に成立します）。\
                 characters に定義されているか、cast に含まれているかを確認してください"
            ),
            // authored effects の死んだ参照。トリガー効果は検証を通らない (信頼済で apply_ops
            // 直行) ので、typo は「エラーも警告もなく判定が起きない」形で沈黙していた。
            gm_core::ScenarioError::UnknownChallengeInEffects { origin, challenge } => format!(
                "{origin} の attempt_challenge が、存在しない挑戦「{challenge}」を指しています。\
                 このトリガー/帰結は発火しても判定を起こしません（エラーも出ません）。\
                 challenges に定義された id を確認してください"
            ),
            gm_core::ScenarioError::UnknownContestInEffects { origin, contest } => format!(
                "{origin} の attempt_contest が、存在しない対決「{contest}」を指しています。\
                 幻の対決が進行中のまま居座り、以後の対決がすべて「対決の最中」で却下されます。\
                 contests に定義された id を確認してください"
            ),
            // 効果の move は出口も gate も見ないので、行き先の typo は「宣言されていない場所に
            // 立つ」= 出口も説明文も無いソフトロックになる (エラーも警告も出ない)。
            gm_core::ScenarioError::UnknownLocationInEffects { origin, to } => format!(
                "{origin} の move が、宣言されていない場所「{to}」を指しています。\
                 主人公はその場所に立ちますが、出口も説明文も無いため先へ進めなくなります（エラーも出ません）。\
                 locations に定義された id を確認してください"
            ),
            other => format!("{other:?}"),
        })
        .collect()
}

