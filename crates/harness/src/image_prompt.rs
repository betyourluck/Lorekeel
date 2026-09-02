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
/// この一枚への追加指示 (spec 27 B-2)。
const DIRECTION_BUDGET: usize = 300;
/// **素材節に載せるスタイルの予算** (spec 27 B-1)。スタイルは最終プロンプトへ原文のまま
/// 前置きされるので、LLM に要るのは「矛盾しないための要旨」だけ。**切っても算入は続ける** —
/// 除外すると長いスタイル文字列で入力が無制限に伸び、予算の目的 (入力の有界性) が壊れる。
const STYLE_MATERIAL_BUDGET: usize = 300;
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
    /// ユーザーのスタイル接頭辞 (設定)。**最終プロンプトへ前置きされる文字列の要旨**で、
    /// ここに載るのは [`STYLE_MATERIAL_BUDGET`] まで (前置きされる側は全文)。
    pub user_prefix: String,
    /// この一枚への追加指示 (spec 27 B-2、揮発)。場面依存の意図は語りと合成が要るので LLM へ渡す。
    pub direction: String,
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
    direction: &str,
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
        user_prefix: take_chars(user_prefix, STYLE_MATERIAL_BUDGET),
        direction: take_chars(direction, DIRECTION_BUDGET),
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
            + r.direction.chars().count()
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
            "\n- 参照画像 {n} 枚が添付されます (人物の立ち絵や背景をまとめた資料)。**画像モデルは\
             それを直接見る**ので、見た目に合わせて髪色・髪型・目の色・服装を**具体的な語で**書いて\
             ください。ただし『参照画像』『設定画集』『資料』『reference sheet』のような\
             **参照そのものを指す語をプロンプトに書いてはいけません** — 画像モデルはそれを被写体の\
             指示と読み、資料集のような分割画像を作ります。参照に無い人物は profile の範囲で描きます。\n\
             - 描くのは**ひとつながりの 1 場面**です。分割・コマ割り・複数カット・並置・帯・枠線を\
             作らないよう明記してください (例: a single continuous scene, one frame, no panels or split \
             screen)。参照の無地背景・余白・枠・文字も構図に持ち込まず、場面の背景を画面いっぱいに\
             描くよう明記してください (例: the scene background fills the entire frame, no plain \
             backdrop or border)。",
            n = self.refs
        )
    }

    /// 素材 (user)。空の節は省く。
    pub fn user_prompt(&self) -> String {
        let mut s = String::new();
        if !self.user_prefix.is_empty() {
            // spec 27 B-1: スタイルは**最終プロンプトの先頭へ原文のまま前置きされる**。ここに
            // 載せるのは矛盾を避けるためで、書き写させるためではない (見せないと「水彩」の横で
            // photorealistic と書き、1 本のプロンプトが自己矛盾する)。
            s.push_str(&format!(
                "# スタイル指定 (プレイヤーから。**この文字列は最終プロンプトの先頭に原文のまま\
                 前置きされる**ので、重複して書かず、これと矛盾する画風語も書かないこと)\n{}\n\n",
                self.user_prefix
            ));
        }
        if !self.direction.is_empty() {
            s.push_str(&format!("# この一枚への指示 (プレイヤーから。必ず反映)\n{}\n\n", self.direction));
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

/// 最終プロンプトの合成 (spec 27 B-5、純関数)。
///
/// - `override_text` が非空なら**それだけ**を返す (手書きが最終形。スタイルを再び前置きしない
///   = ダイアログに見えているものがそのまま送られる)。
/// - そうでなければ `style` (ユーザーの様式指定・**原文のまま**) と `scene` (プロンプト書きの出力) を
///   区切って連結する。**スタイルが LLM を通らない**のがこの関数の存在理由 — 通すと言い換えられ、
///   タグ列を渡したときに原文が保たれない。
pub fn compose_image_prompt(style: &str, scene: &str, override_text: &str) -> String {
    let ov = override_text.trim();
    if !ov.is_empty() {
        return ov.to_string();
    }
    // 末尾の区切り記号は落としてから繋ぐ (`watercolor,` + `, ` で `,,` を作らない)。
    let style = style.trim().trim_end_matches([',', '、', '。']).trim();
    // 機械の守り (2026-09-03): プロンプト書きが「前置きされるので重複して書くな」を守らず
    // スタイルを出力の先頭へ写すことがある (タグ列のスタイルを画風タグとして写す)。規律は
    // 見せる側、ここは通さない側 — 先頭の写しを落としてから繋ぐ。
    let scene = strip_leading_style_echo(style, scene.trim());
    match (style.is_empty(), scene.is_empty()) {
        (true, _) => scene.to_string(),
        (false, true) => style.to_string(),
        (false, false) => format!("{style}, {scene}"),
    }
}

/// `scene` の先頭が `style` の写しなら、その写しと直後の区切りを落とした残りを返す。
/// 一致は**空白を無視し大小文字を同一視**して非空白文字の列で見る (LLM は空白と大小を揃えない)。
/// 写しの直後が語の途中 (英数字が続く) なら一致と見なさない (`photo` は `photograph` を剥がさない)。
/// `style` が空、または一致しなければ `scene` をそのまま返す。
fn strip_leading_style_echo<'a>(style: &str, scene: &'a str) -> &'a str {
    if style.is_empty() {
        return scene;
    }
    let mut want = style.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase);
    let mut cut = 0usize; // 一致した最後の文字の直後 (byte index)
    for (i, c) in scene.char_indices() {
        if c.is_whitespace() {
            continue;
        }
        match want.next() {
            None => break,
            Some(w) => {
                if c.to_lowercase().ne(std::iter::once(w)) {
                    return scene;
                }
                cut = i + c.len_utf8();
            }
        }
    }
    if want.next().is_some() {
        return scene; // scene が style より短い (途中で尽きた)
    }
    let rest = &scene[cut..];
    // 語の途中で一致していたら写しではない。
    if rest.chars().next().is_some_and(|c| c.is_alphanumeric()) {
        return scene;
    }
    rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | '、' | '。' | ';' | '；'))
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
            "",
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
        let tags = build_image_prompt_request(&sc, &state, "x", "", "", ImagePromptStyle::Tags, 0);
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
        let tags = build_image_prompt_request(&sc, &state, "x", "", "", ImagePromptStyle::Tags, 0);
        let sys = tags.system_prompt();
        assert!(sys.contains("年齢") && sys.contains("体格"), "tags は年齢・体格タグを要求する: {sys}");
        assert!(sys.contains("adult man"), "具体例で接地する (弱モデル対応): {sys}");
        let prose = build_image_prompt_request(&sc, &state, "x", "", "", ImagePromptStyle::Prose, 0);
        let psys = prose.system_prompt();
        assert!(!psys.contains("年齢") && !psys.contains("adult man"), "prose は不変: {psys}");
    }

    /// 【秘密は渡らない】hidden 属性 (正体: 人狼) も state_brief も本文に現れない。挿絵書きは GM ではない。
    #[test]
    fn request_never_carries_secrets() {
        let sc = scenario();
        let state = sc.initial_state(1);
        let req = build_image_prompt_request(&sc, &state, "静かな夜。", "", "", ImagePromptStyle::Prose, 0);
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
        let req = build_image_prompt_request(&sc, &state, &long, "", "", ImagePromptStyle::Tags, 0);
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
        let empty = build_image_prompt_request(&sc, &state, "", "", "", ImagePromptStyle::Tags, 0);
        assert!(empty.user_prompt().contains("まだ語りが無い"));
    }

    /// 【合成 (spec 27 B-5)】override は verbatim (スタイルを前置きしない = 見えているものが送られる)。
    /// 片方が空なら区切り文字が混入しない。末尾の区切り記号は重ねない。
    #[test]
    fn compose_puts_style_verbatim_in_front_unless_overridden() {
        assert_eq!(compose_image_prompt("watercolor", "a cat on a chair", ""), "watercolor, a cat on a chair");
        assert_eq!(compose_image_prompt("", "a cat", ""), "a cat", "スタイル空なら場面だけ");
        assert_eq!(compose_image_prompt("watercolor", "", ""), "watercolor", "場面空ならスタイルだけ");
        assert_eq!(compose_image_prompt("", "", ""), "");
        assert_eq!(compose_image_prompt("masterpiece,", "1girl", ""), "masterpiece, 1girl", "`,,` を作らない");
        // override は手書きが最終形 — スタイルも場面も混ぜない。
        assert_eq!(compose_image_prompt("watercolor", "a cat", "  my own prompt  "), "my own prompt");
        assert_eq!(compose_image_prompt("watercolor", "a cat", "   "), "watercolor, a cat", "空白だけは override でない");
    }

    /// 【この一枚への指示 (spec 27 B-2)】direction が user 節に出る・空なら節ごと出ない・
    /// **スタイル節は「前置きされるので重複させるな」の役割になる**・素材節の style は 300 字で切れる
    /// (前置きされる側は全文なので、切るのは LLM に見せる要旨だけ)。
    #[test]
    fn direction_and_style_material_roles() {
        let sc = scenario();
        let state = sc.initial_state(1);
        let none = build_image_prompt_request(&sc, &state, "夜。", "水彩", "", ImagePromptStyle::Prose, 0);
        assert!(!none.user_prompt().contains("この一枚への指示"), "空なら節ごと出ない");
        let with = build_image_prompt_request(&sc, &state, "夜。", "水彩", "引きの構図で", ImagePromptStyle::Prose, 0);
        let u = with.user_prompt();
        assert!(u.contains("# この一枚への指示") && u.contains("引きの構図で"), "{u}");
        assert!(u.contains("原文のまま") && u.contains("矛盾する画風語も書かない"), "スタイル節の役割: {u}");
        // 素材節の style は 300 字で切る (前置きされる全文とは別)。
        let long_style = "あ".repeat(500);
        let cut = build_image_prompt_request(&sc, &state, "夜。", &long_style, "", ImagePromptStyle::Prose, 0);
        assert!(cut.user_prefix.chars().count() <= STYLE_MATERIAL_BUDGET + 1, "{}", cut.user_prefix.chars().count());
        // direction も予算内 (**算入は続く** = 入力の有界性)。
        let long_dir = "い".repeat(500);
        let d = build_image_prompt_request(&sc, &state, "夜。", "", &long_dir, ImagePromptStyle::Prose, 0);
        assert!(d.direction.chars().count() <= DIRECTION_BUDGET + 1);
    }

    /// 【設定画集の規律 (spec 25)】refs=0 なら system/user とも従来文言と byte 一致 (規律を一切足さない)。
    /// refs>0 で末尾に 1 箇条だけ増え、枚数と「設定画集」「reference sheets」が出る。
    #[test]
    fn reference_sheets_rule_is_appended_only_when_present() {
        let sc = scenario();
        let state = sc.initial_state(1);
        let zero = build_image_prompt_request(&sc, &state, "静かな夜。", "anime", "", ImagePromptStyle::Prose, 0);
        let two = build_image_prompt_request(&sc, &state, "静かな夜。", "anime", "", ImagePromptStyle::Prose, 2);
        assert!(!zero.system_prompt().contains("設定画集") && !zero.system_prompt().contains("reference sheets"));
        assert_eq!(zero.user_prompt(), two.user_prompt(), "user 側は枚数で変わらない");
        let sys2 = two.system_prompt();
        assert!(sys2.starts_with(&zero.system_prompt()), "既存文言はそのまま・末尾に足すだけ");
        assert!(sys2.contains("参照画像 2 枚が添付されます"));
        // **参照そのものを指す語を書かせない** (failures #85): 「(as in the reference sheets)」を
        // 書けと指示していた頃、画像モデルがそれを被写体の指示と読み**資料集のような分割画像**を
        // 作った。規律は「参照の見た目に合わせて具体語で書け」+「参照を指す語は書くな」の対。
        assert!(!sys2.contains("as in the reference sheets"), "参照を指す語を書かせない: {sys2}");
        assert!(sys2.contains("参照そのものを指す語をプロンプトに書いてはいけません"));
        assert!(sys2.contains("ひとつながりの 1 場面") && sys2.contains("no panels or split"));
        assert!(
            sys2.contains("構図に持ち込まず") && sys2.contains("fills the entire frame"),
            "参照の無地背景が構図に漏れる (2026-08-22 Krea 2 実測) を抑える文言"
        );
        // 箇条は 2 つ増える (参照の指し方 / ひとつながりの 1 場面)。どちらも**参照があるときだけ**
        // 必要な規律で、0 枚では 1 文字も足さない (byte 一致は上の assert が固定)。
        assert_eq!(
            sys2.matches("
- ").count(),
            zero.system_prompt().matches("
- ").count() + 2,
            "箇条は 2 つ増える"
        );
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

    /// 【スタイルの写しを落とす (2026-09-03、ユーザー報告「スタイル指定が最終プロンプトに二重で出る」)】
    /// プロンプト書きには「前置きされるので重複して書くな」と渡しているが、規律だけでは守られない
    /// (タグ列のスタイルを画風タグとして出力の先頭へ写す)。合成は**機械の守り**として、出力の
    /// 先頭がスタイル原文と (空白・大小文字・末尾区切りを無視して) 一致するならその写しを落とす。
    /// 語の途中では切らない (`photo` は `photograph…` を剥がさない)。override は不変。
    #[test]
    fn compose_strips_a_leading_echo_of_the_style() {
        let style = "Photograph, looking away,  full body,";
        // 写し (大小文字と空白が違う・区切りの直後に本文)
        let scene = "photograph, looking away, full body, a man stands on a beach at dawn";
        assert_eq!(
            compose_image_prompt(style, scene, ""),
            "Photograph, looking away,  full body, a man stands on a beach at dawn"
        );
        // 写しの後の区切りが `.` や改行でも落ちる
        assert_eq!(
            compose_image_prompt(style, "Photograph, looking away, full body.\nA man stands.", ""),
            "Photograph, looking away,  full body, A man stands."
        );
        // 写しだけ (本文なし) ならスタイルだけになる
        assert_eq!(compose_image_prompt(style, "photograph, looking away, full body", ""), "Photograph, looking away,  full body");
        // 写しでない出力は不変
        assert_eq!(
            compose_image_prompt(style, "a man stands on a beach", ""),
            "Photograph, looking away,  full body, a man stands on a beach"
        );
        // 語の途中で一致しても剥がさない
        assert_eq!(compose_image_prompt("photo", "photograph of a man", ""), "photo, photograph of a man");
        // override は verbatim のまま (スタイルが入っていても触らない)
        assert_eq!(compose_image_prompt(style, "x", "photograph, photograph, x"), "photograph, photograph, x");
        // 空スタイルは何もしない
        assert_eq!(compose_image_prompt("", "a man", ""), "a man");
    }
}
