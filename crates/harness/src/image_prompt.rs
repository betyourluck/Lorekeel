//! 挿絵のプロンプト書き (spec 24, 2026-08-20) — 画像生成プロバイダへ渡す**画像プロンプト**を
//! GM の LLM に 1 回の `generate` で書かせるためのリクエスト組み立て (純関数・テスト可)。
//!
//! **素材は全てプレイヤーが既に見ているもの**: 直前の語り (会話ログ) / `Location.description`
//! (開幕ログに出る文。GM 向けの隠し説明は `Location` に存在しない) / この場にいる人物の
//! `profile` (プロフィールカード) / `world` / 作者の `image_style` / ユーザーの接頭辞。
//! `state_brief`・`hidden_*`・`secret_*`・`internal_*`・既成事実は**入れない** — 挿絵書きは GM
//! ではないので GM の秘密を見ない (PoC で固定)。
//!
//! エピローグ ([`crate::build_epilogue_request`]) と同型: 予算つきの素材 + 規律の system。

use gm_core::{GameState, Scenario, IMAGE_STYLE_MAX_CHARS, PLAYER};
use llm_client::ChatMessage;

/// プロンプトの様式 (契約 `prompt_writer`)。既定はプロバイダに倒す (呼び出し側が決める)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagePromptStyle {
    /// danbooru 形式・カンマ区切り・英語・50 語以内 (ComfyUI / A1111 向け)。
    Tags,
    /// 英語 1〜3 文 (OpenAI / Gemini 向け)。
    Prose,
}

/// 素材の予算 (文字数)。合計 2000 字以内に収める (契約 `prompt_writer`)。
const NARRATION_BUDGET: usize = 1200;
const WORLD_BUDGET: usize = 600;
const PROFILE_BUDGET: usize = 400;
/// 4 人目以降の profile 予算 (名前だけにはしない — 容姿の手掛かりは全員に残す)。
const PROFILE_SHORT_BUDGET: usize = 120;
const PROFILE_FULL_COUNT: usize = 3;
const LOCATION_BUDGET: usize = 400;
const TOTAL_BUDGET: usize = 2000;

/// 組み上がったリクエスト (純粋データ)。`messages()` で ChatMessage 列にする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePromptRequest {
    pub style: ImagePromptStyle,
    /// 直前の語り (予算で切る)。
    pub narration: String,
    /// 現在地の説明 (title と description)。
    pub location: String,
    /// この場にいる人物 (主人公が先頭、NPC は id 順 = 決定論): 上位 [`PROFILE_FULL_COUNT`] 名は
    /// profile 400 字まで、以降は 120 字まで (名前だけにはしない)。
    pub present: Vec<String>,
    pub world: String,
    /// 作者の画風指針 (`Scenario.image_style`、上限で切る)。
    pub author_style: String,
    /// ユーザーのスタイル接頭辞 (設定)。
    pub user_prefix: String,
    /// 添付する設定画集の枚数 (spec 25)。0 なら文言は spec 24 と byte 一致 (規律を一切足さない)。
    pub refs: usize,
}

fn take_chars(s: &str, budget: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= budget {
        return s.to_string();
    }
    let mut out: String = s.chars().take(budget).collect();
    out.push('…');
    out
}

/// 素材を組む (純関数)。`last_narration` は直前の受理ターンの語り (空なら場所説明だけで描く)。
pub fn build_image_prompt_request(
    scenario: &Scenario,
    state: &GameState,
    last_narration: &str,
    user_prefix: &str,
    style: ImagePromptStyle,
    refs: usize,
) -> ImagePromptRequest {
    let location = scenario
        .locations
        .get(&state.location)
        .map(|l| {
            let title = if l.title.trim().is_empty() { state.location.as_str() } else { l.title.as_str() };
            take_chars(&format!("{title} — {}", l.description), LOCATION_BUDGET)
        })
        .unwrap_or_default();

    // presence: 主人公を先頭に、この場にいる NPC (present_at = 実効 presence)。
    let mut present = Vec::new();
    let protagonist_name = if scenario.protagonist.name.trim().is_empty() {
        "主人公".to_string()
    } else {
        scenario.protagonist.name.clone()
    };
    present.push((protagonist_name, scenario.protagonist.profile.clone()));
    for id in scenario.present_at(state) {
        if id == PLAYER {
            continue;
        }
        if let Some(c) = scenario.characters.get(&id) {
            let name = if c.name.trim().is_empty() { id.clone() } else { c.name.clone() };
            present.push((name, c.profile.clone()));
        }
    }
    let present: Vec<String> = present
        .into_iter()
        .enumerate()
        .map(|(i, (name, profile))| {
            if profile.trim().is_empty() {
                name
            } else if i < PROFILE_FULL_COUNT {
                format!("{name}: {}", take_chars(&profile, PROFILE_BUDGET))
            } else {
                format!("{name}: {}", take_chars(&profile, PROFILE_SHORT_BUDGET))
            }
        })
        .collect();

    let mut req = ImagePromptRequest {
        style,
        narration: take_chars(last_narration, NARRATION_BUDGET),
        location,
        present,
        world: take_chars(&scenario.world, WORLD_BUDGET),
        author_style: take_chars(&scenario.image_style, IMAGE_STYLE_MAX_CHARS),
        user_prefix: user_prefix.trim().to_string(),
        refs,
    };
    // 合計予算: 溢れたら語り → world の順で削る (人物と場所は被写体なので最後まで残す)。
    let total = |r: &ImagePromptRequest| {
        r.narration.chars().count()
            + r.location.chars().count()
            + r.present.iter().map(|p| p.chars().count()).sum::<usize>()
            + r.world.chars().count()
            + r.author_style.chars().count()
            + r.user_prefix.chars().count()
    };
    if total(&req) > TOTAL_BUDGET {
        let over = total(&req) - TOTAL_BUDGET;
        let keep = req.narration.chars().count().saturating_sub(over);
        req.narration = take_chars(&req.narration, keep);
    }
    if total(&req) > TOTAL_BUDGET {
        let over = total(&req) - TOTAL_BUDGET;
        let keep = req.world.chars().count().saturating_sub(over);
        req.world = take_chars(&req.world, keep);
    }
    req
}

impl ImagePromptRequest {
    /// 規律 (system)。出力はプロンプトのみ — 前置き・説明・コードフェンス禁止 (CoT 除去 #30 同型)。
    pub fn system_prompt(&self) -> String {
        let form = match self.style {
            ImagePromptStyle::Tags => {
                "出力形式: danbooru 形式のタグ列。英語・カンマ区切り・50 語以内。\
                 人物は容姿と服装、続けて構図・光・画風のタグ。\
                 人物には年齢・体格のタグを必ず含める (adult man, mature male, 30 years old 等) — \
                 danbooru 語彙は年齢を指定しないと若年に倒れる。"
            }
            ImagePromptStyle::Prose => {
                "出力形式: 英語の自然文 1〜3 文。被写体 (人物の容姿と服装)・構図・光・画風を具体的に。"
            }
        };
        format!(
            "あなたは挿絵のプロンプトを書く画家のアシスタントです。与えられた場面の記録から、\
             画像生成モデルに渡す**画像プロンプトを 1 つだけ**書いてください。\n\
             - {form}\n\
             - 描くのは「直前の語り」の場面。いまその場にいる人物だけを描き、\
             いない人物・語りに無い人物を加えない。\n\
             - 人物の容姿・服装は profile と語りに書かれている範囲で。書かれていない衣装や\
             固有名詞を発明しない。\n\
             - 状態 (勝敗・生死・感情) を断定しない — 語りに描かれている以上のことを決めない。\n\
             - ネガティブプロンプト (描かないもの) は書かない。\n\
             - 出力はプロンプト本文のみ。前置き・説明・引用符・コードフェンスを付けない。{refs}",
            refs = self.refs_rule()
        )
    }

    /// 設定画集 (spec 25) があるときだけ足す規律。0 枚なら空文字 = spec 24 と byte 一致。
    fn refs_rule(&self) -> String {
        if self.refs == 0 {
            return String::new();
        }
        format!(
            "\n- 参照画像 {n} 枚は設定画集です (人物の立ち絵に名前が書かれ、背景がまとめられている)。\
             登場人物と場所の見た目はそれに従い、プロンプト内で (as in the reference sheets) と指して\
             ください。参照に無い人物は profile の範囲で描きます。参照の無地背景・余白・枠・文字は\
             構図に持ち込まず、場面の背景を画面いっぱいに描くよう明記してください\
             (例: the scene background fills the entire frame, no plain backdrop or border)。",
            n = self.refs
        )
    }

    /// 素材 (user)。空の節は省く。
    pub fn user_prompt(&self) -> String {
        let mut s = String::new();
        if !self.user_prefix.is_empty() {
            s.push_str(&format!("# スタイル指定 (プレイヤーから。必ず反映)\n{}\n\n", self.user_prefix));
        }
        if !self.author_style.is_empty() {
            s.push_str(&format!("# 画風 (作者から)\n{}\n\n", self.author_style));
        }
        if !self.world.is_empty() {
            s.push_str(&format!("# 世界観\n{}\n\n", self.world));
        }
        if !self.location.is_empty() {
            s.push_str(&format!("# 現在地\n{}\n\n", self.location));
        }
        if !self.present.is_empty() {
            s.push_str("# この場にいる人物\n");
            for p in &self.present {
                s.push_str(&format!("- {p}\n"));
            }
            s.push('\n');
        }
        if !self.narration.is_empty() {
            s.push_str(&format!("# 直前の語り (この場面を描く)\n{}\n\n", self.narration));
        } else {
            s.push_str("# 直前の語り\n(まだ語りが無い — 現在地の情景を描く)\n\n");
        }
        s.push_str("以上から、画像プロンプトを 1 つだけ出力してください。");
        s
    }
}

/// `generate` に渡す messages。
pub fn image_prompt_messages(request: &ImagePromptRequest) -> Vec<ChatMessage> {
    vec![
        ChatMessage::system(request.system_prompt()),
        ChatMessage::user(request.user_prompt()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario() -> Scenario {
        let yaml = r#"
title: 湖畔
world: 昭和初期の日本。湖畔の洋館。
protagonist: { name: 調査員, profile: 三十代の男。くたびれたコートに鞄。 }
image_style: 水彩画・淡い色
start: hall
locations:
  hall:
    title: 玄関ホール
    description: 埃をかぶったシャンデリア。扉は開いていた。
    present: [maid, butler, guest, extra]
    exits: []
characters:
  maid: { name: メイド, profile: 黒い給仕服に白いエプロン。眼鏡。, attributes: { 正体: 人狼 } }
  butler: { name: 執事, profile: 燕尾服の老人。 }
  guest: { name: 客, profile: 旅装の女。 }
  extra: { name: 端役, profile: 地味な男。 }
  absent: { name: 不在者, profile: 赤い外套。 }
hidden_attributes: [正体]
goal: { kind: flag_is, key: done, value: true }
allowed_flags: [done]
"#;
        serde_yaml::from_str(yaml).unwrap()
    }

    /// 【素材と予算】語り/場所/presence (主人公先頭・NPC は id 順。上位 3 名は profile 400 字・
    /// 4 人目以降は 120 字)/world/作者の画風/ユーザー接頭辞が入り、**居ない人物は入らない**。
    #[test]
    fn request_gathers_visible_materials_with_presence_budget() {
        let sc = scenario();
        let state = sc.initial_state(1);
        let req = build_image_prompt_request(
            &sc,
            &state,
            "メイドが紅茶を運んできた。執事は黙って窓を見ている。",
            "anime style",
            ImagePromptStyle::Prose,
            0,
        );
        assert_eq!(req.present.len(), 5, "主人公 + present 4 名");
        assert!(req.present[0].starts_with("調査員: 三十代"), "主人公が先頭・profile つき: {:?}", req.present);
        // NPC は id 順 (butler, extra, guest, maid) = 決定論。上位 3 名 (主人公込み) は 400 字枠。
        assert!(req.present[1].starts_with("執事: 燕尾服"), "{:?}", req.present);
        assert!(req.present[2].starts_with("端役: 地味な男"));
        assert!(req.present[3].starts_with("客: 旅装の女"), "4 人目以降も 120 字の profile は付く");
        assert!(req.present[4].starts_with("メイド: 黒い給仕服"));
        let text = req.user_prompt();
        assert!(!text.contains("不在者") && !text.contains("赤い外套"), "居ない人物は入らない");
        assert!(text.contains("玄関ホール — 埃をかぶった"));
        assert!(text.contains("昭和初期"));
        assert!(text.contains("水彩画・淡い色"));
        assert!(text.contains("anime style"));
        assert!(text.contains("紅茶を運んできた"));
        assert!(req.system_prompt().contains("自然文"), "様式 prose の形式指示");
        let tags = build_image_prompt_request(&sc, &state, "x", "", ImagePromptStyle::Tags, 0);
        assert!(tags.system_prompt().contains("danbooru"));
        assert!(!tags.user_prompt().contains("スタイル指定"), "空の接頭辞は節ごと省く");
        let msgs = image_prompt_messages(&req);
        assert_eq!(msgs.len(), 2);
    }

    /// 【tags の年齢・体格接地 (spec 26)】danbooru 語彙は年齢無指定だと若年に倒れる
    /// (2026-08-21 実測: `1boy` が三十代の主人公を子供に変えた) → tags の形式指示は
    /// 年齢・体格タグを必ず要求する。prose には足さない (byte 不変)。
    #[test]
    fn tags_form_demands_age_and_build_prose_unchanged() {
        let sc = scenario();
        let state = sc.initial_state(1);
        let tags = build_image_prompt_request(&sc, &state, "x", "", ImagePromptStyle::Tags, 0);
        let sys = tags.system_prompt();
        assert!(sys.contains("年齢") && sys.contains("体格"), "tags は年齢・体格タグを要求する: {sys}");
        assert!(sys.contains("adult man"), "具体例で接地する (弱モデル対応): {sys}");
        let prose = build_image_prompt_request(&sc, &state, "x", "", ImagePromptStyle::Prose, 0);
        let psys = prose.system_prompt();
        assert!(!psys.contains("年齢") && !psys.contains("adult man"), "prose は不変: {psys}");
    }

    /// 【秘密は渡らない】hidden 属性 (正体: 人狼) も state_brief も本文に現れない。挿絵書きは GM ではない。
    #[test]
    fn request_never_carries_secrets() {
        let sc = scenario();
        let state = sc.initial_state(1);
        let req = build_image_prompt_request(&sc, &state, "静かな夜。", "", ImagePromptStyle::Prose, 0);
        let all = format!("{}\n{}", req.system_prompt(), req.user_prompt());
        assert!(!all.contains("人狼"), "hidden 属性の値が本文に出ない: {all}");
        assert!(!all.contains("正体"), "hidden 属性のキーも出ない");
        assert!(!all.contains("フラグ") && !all.contains("done"), "state_brief 系の語彙を持ち込まない");
        assert!(req.system_prompt().contains("ネガティブプロンプト (描かないもの) は書かない"));
    }

    /// 【予算】語り 1200 字 / 画風 500 字 / 合計 2000 字で切る (被写体 = 人物と場所は最後まで残す)。
    #[test]
    fn request_respects_budgets() {
        let mut sc = scenario();
        sc.image_style = "あ".repeat(800);
        sc.world = "い".repeat(900);
        for c in sc.characters.values_mut() {
            c.profile = "え".repeat(700);
        }
        let state = sc.initial_state(1);
        let long = "う".repeat(3000);
        let req = build_image_prompt_request(&sc, &state, &long, "", ImagePromptStyle::Tags, 0);
        assert!(req.author_style.chars().count() <= IMAGE_STYLE_MAX_CHARS + 1);
        assert!(req.narration.chars().count() <= NARRATION_BUDGET + 1);
        let total = req.narration.chars().count()
            + req.location.chars().count()
            + req.present.iter().map(|p| p.chars().count()).sum::<usize>()
            + req.world.chars().count()
            + req.author_style.chars().count();
        assert!(total <= TOTAL_BUDGET + 2, "合計 {total}");
        assert!(!req.location.is_empty() && req.present.len() == 5, "被写体は削らない");
        assert!(req.present[1].chars().count() <= PROFILE_BUDGET + 10, "上位は 400 字枠");
        assert!(req.present[4].chars().count() <= PROFILE_SHORT_BUDGET + 10, "以降は 120 字枠: {}", req.present[4].chars().count());
        // 語りが空なら「現在地の情景を描く」へ倒す。
        let empty = build_image_prompt_request(&sc, &state, "", "", ImagePromptStyle::Tags, 0);
        assert!(empty.user_prompt().contains("まだ語りが無い"));
    }

    /// 【設定画集の規律 (spec 25)】refs=0 なら system/user とも従来文言と byte 一致 (規律を一切足さない)。
    /// refs>0 で末尾に 1 箇条だけ増え、枚数と「設定画集」「reference sheets」が出る。
    #[test]
    fn reference_sheets_rule_is_appended_only_when_present() {
        let sc = scenario();
        let state = sc.initial_state(1);
        let zero = build_image_prompt_request(&sc, &state, "静かな夜。", "anime", ImagePromptStyle::Prose, 0);
        let two = build_image_prompt_request(&sc, &state, "静かな夜。", "anime", ImagePromptStyle::Prose, 2);
        assert!(!zero.system_prompt().contains("設定画集") && !zero.system_prompt().contains("reference sheets"));
        assert_eq!(zero.user_prompt(), two.user_prompt(), "user 側は枚数で変わらない");
        let sys2 = two.system_prompt();
        assert!(sys2.starts_with(&zero.system_prompt()), "既存文言はそのまま・末尾に足すだけ");
        assert!(sys2.contains("参照画像 2 枚は設定画集") && sys2.contains("(as in the reference sheets)"));
        assert!(
            sys2.contains("構図に持ち込まず") && sys2.contains("fills the entire frame"),
            "参照の無地背景が構図に漏れる (2026-08-22 Krea 2 実測) を抑える文言"
        );
        assert_eq!(sys2.matches("
- ").count(), zero.system_prompt().matches("
- ").count() + 1, "箇条は 1 つだけ増える");
    }

    /// 【image_style の注入と lint】package.yaml の image_style が scenario へ注入され、
    /// 500 字超は lint (非 fatal) が名指しする。
    #[test]
    fn image_style_lint_and_manifest_injection() {
        let mut sc = scenario();
        assert!(sc.lints().is_empty(), "500 字以内は沈黙");
        sc.image_style = "x".repeat(501);
        let lints = sc.lints();
        assert!(
            lints.iter().any(|l| matches!(l, gm_core::ScenarioError::ImageStyleTooLong { chars } if *chars == 501)),
            "{lints:?}"
        );
        assert!(sc.validate().is_empty(), "lint であって load 拒否ではない");
        let msgs = crate::scenario_lint_messages(&sc);
        assert!(msgs.iter().any(|m| m.contains("image_style") && m.contains("501")), "{msgs:?}");
    }
}
