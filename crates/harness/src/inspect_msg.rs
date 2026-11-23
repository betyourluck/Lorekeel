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
            gm_core::ScenarioError::ChallengeMaxPerTurnZero { challenge } => format!(
                "挑戦「{challenge}」の max_per_turn が 0 です。一覧には出るのに選ぶと必ず却下されます\
                 （作者からは「たまに判定が出ない」ようにしか見えません）。\
                 挑めなくしたいなら requires を、回数を制限したいなら 1 以上を書いてください"
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
            // present に挙げたのに characters / cast に居ないキャラは、present_at が落とすので
            // その場所に一度も現れない (2026-11-23、spec 38 Phase B の実測)。
            gm_core::ScenarioError::UnknownEntityInPresent { location, entity } => format!(
                "場所「{location}」の present に挙がっている「{entity}」は、このシナリオの登場人物にいません。\
                 この場所には現れません（エラーも出ません）。characters/{entity}.yaml があるなら、\
                 シナリオの cast に {entity} を加えてください"
            ),
            // 出口の行き先 / start が宣言されていない場所 (2026-11-23、spec 38 Phase B の実測)。
            // move は受理されるので、出口も説明文も無い場所に立って先へ進めなくなる。
            gm_core::ScenarioError::UnknownLocationInExit { from, to } => format!(
                "場所「{from}」の出口が、宣言されていない場所「{to}」を指しています。\
                 そこへ移動すると出口も説明文も無いため先へ進めなくなります（エラーも出ません）。\
                 locations に「{to}」を足すか、出口の行き先を locations にある id にしてください"
            ),
            gm_core::ScenarioError::UnreachableLocation { location } => format!(
                "場所「{location}」へは、start から出口をたどっても、トリガーなどの move からも行けません。\
                 主人公はこの場所に一度も立てません。\
                 どこかの場所の exits に「- to: {location}」を足すか、要らない場所なら消してください"
            ),
            gm_core::ScenarioError::UnknownStartLocation { start } => format!(
                "start の「{start}」が locations にありません。開幕から出口も説明文も無い場所に立ちます。\
                 locations にある id にしてください"
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
            gm_core::ScenarioError::UnknownLocationInMoveCharacter { origin, entity, to } => format!(
                "{origin} の move_character が、{entity} を宣言されていない場所「{to}」へ行かせています。\
                 主人公はその場所に立てないので、{entity} には二度と会えなくなります（エラーも出ません）。\
                 locations に定義された id を確認してください"
            ),
            gm_core::ScenarioError::MoveCharacterNotACharacter { origin, entity } => format!(
                "{origin} の move_character の「{entity}」は、このシナリオの登場人物ではありません。\
                 何も起きません。主人公を動かすなら move、NPC なら characters / cast の id を確認してください"
            ),
            // spec 24: 画風指針は先頭だけ使われる (プレイは壊れない)。
            gm_core::ScenarioError::ImageStyleTooLong { chars } => format!(
                "image_style が {chars} 字あります (上限 {})。先頭 {} 字だけが挿絵のプロンプトに使われます — \
                 画風の要点を前に寄せるか、短くしてください",
                gm_core::IMAGE_STYLE_MAX_CHARS, gm_core::IMAGE_STYLE_MAX_CHARS
            ),
            other => format!("{other:?}"),
        })
        .collect()
}

