//! OpenAI 互換 adapter (GPT / Grok / さくら / ローカル互換サーバ) — spec 12 Phase A。
//!
//! canonical ⇄ wire の **encode/decode 純関数**。HTTP・リトライ・認証・sticky ヘッダ
//! (`x-grok-conv-id` #45) は client 核が担い、ここは形の翻訳だけを持つ
//! (壊れるのは ser/de なので PoC で固める)。

use crate::canonical::{ChatRequest, ChatResponse, Finish, ToolCall, ToolChoice, Usage};
use crate::config::{Effort, ToolMode};
use crate::error::LlmError;
use crate::wire;

/// canonical → OpenAI 互換 wire。
///
/// [`ToolMode`] の三値がここで形になる:
/// - [`Forced`](ToolMode::Forced): tools + 名前指定の `tool_choice` (既定・構造化出力の主経路)
/// - [`Auto`](ToolMode::Auto): tools + 素の `"auto"` (Meta は他の値を 400 で拒む)
/// - [`Off`](ToolMode::Off): tools を送らず schema を載せた [`json_instruction`] を
///   **messages 末尾の system** として積む (#29 さくら AI Engine / ローカル互換)
pub(crate) fn encode(req: &ChatRequest, mode: ToolMode) -> wire::ChatRequest {
    let mut messages = req.messages.clone();
    let (tools, tool_choice) = if req.tools.is_empty() {
        (Vec::new(), None)
    } else if mode == ToolMode::Off {
        messages.push(wire::ChatMessage::system(json_instruction(
            &req.tools[0].parameters,
        )));
        (Vec::new(), None)
    } else {
        let tools: Vec<_> = req
            .tools
            .iter()
            .map(|t| wire::Tool {
                kind: wire::ToolKind::Function,
                function: wire::FunctionDef {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect();
        // Auto では**強制せず必ず "auto"** — 名前指定も "required" も通らないサーバがある。
        // 提示するツールが emit_delta 1 本だけなので選択肢は実質 1 つで、呼ばなかった場合は
        // GM_SYSTEM の提出行 + parse::extract のフェンス JSON フォールバックが受ける。
        // Auto では**モデルがツールを使わない道を選べる** — 実機 (Meta) はツール定義を
        // 見た上で content に JSON を書き、しかも schema を知らないので `ops` を配列でなく
        // 文字列にした。ツールを提示するだけでは形は伝わらないので、Auto でも schema を
        // prompt に載せる (Off の json_instruction とは**文面が違う** — あちらは
        // 「このサーバはツール非対応」と言い切るので、Auto でそのまま使うとツール利用を
        // 自分で妨げる)。末尾に積むので安定プレフィックスは動かない = キャッシュ影響なし。
        if mode == ToolMode::Auto {
            messages.push(wire::ChatMessage::system(tool_or_json_instruction(
                &req.tools[0].parameters,
            )));
        }
        let choice = match (mode, &req.tool_choice) {
            (ToolMode::Auto, _) => Some(wire::ToolChoice::auto()),
            // v1 の利用は Specific (単一ツール強制) のみ。Auto/Required/None は送らない
            // (= サーバ既定)。
            (_, ToolChoice::Specific(name)) => Some(wire::ToolChoice::force(name.clone())),
            _ => None,
        };
        (tools, choice)
    };
    // 出力上限は常に片方の欄だけで送る (欄名の選択は uses_max_completion_tokens)。
    let (max_tokens, max_completion_tokens) = if uses_max_completion_tokens(&req.model) {
        (None, Some(req.max_tokens))
    } else {
        (Some(req.max_tokens), None)
    };
    wire::ChatRequest {
        model: req.model.clone(),
        messages,
        temperature: req.temperature,
        max_tokens,
        max_completion_tokens,
        reasoning_effort: reasoning_effort(&req.model, req.effort, !tools.is_empty()),
        tools,
        tool_choice,
    }
}

/// OpenAI の o 系推論モデルか (o1 / o3 / o4-mini ...)。
fn is_o_series(model: &str) -> bool {
    model.starts_with('o') && model.chars().nth(1).is_some_and(|c| c.is_ascii_digit())
}

/// 出力上限をどちらの欄名で送るかを決める (純粋)。
///
/// gpt-5 系と o 系は思考トークンを含めて上限を管理する方式へ移っており、旧欄 `max_tokens` を
/// 送ると 400 `unsupported_parameter` で拒否する。**全面置換にしない** — 互換サーバ
/// (llama.cpp / vLLM / gpt-oss 系) には新欄を知らないものがあり、旧欄を落とすと今度は
/// そちらの上限が消える。`reasoning_effort` と同じくモデル名で送り分ける。
pub(crate) fn uses_max_completion_tokens(model: &str) -> bool {
    model.starts_with("gpt-5") || is_o_series(model)
}

/// `reasoning_effort` を送るかどうかと、その値を決める (純粋)。
///
/// **推論制御を持つモデルにだけ送る** — 他モデルへ送るとキーを解釈できず 400 になる。
///
/// `sends_function_tools` は**この周で `tools` を実際に送るか**。gpt-5 系は
/// `/v1/chat/completions` で「function tools と思考の併用」を拒むため、その周だけ
/// `"none"` で固定する (下の分岐)。
///
/// - **gpt-5 系 + tools**: `"none"` を**明示**する。**キーを省いても回避できない** —
///   省くとサーバ側の既定の思考が効き、同じ 400 (`Function tools with reasoning_effort
///   are not supported`) になる。エラー本文が挙げる逃げ道 2 つのうち「`/v1/responses` へ
///   移る」は Chat Completions とは別 API でワイヤをもう 1 本持つことになるので採らない。
///   **tools を送らない周では触らない** — 制約は併用に掛かっており思考自体ではないので、
///   一律に止めるとツール無しの生成 (あらすじ要約等) の思考まで殺す。
/// - **Grok (4.3 / 4.5)**: 対象モデルには既定で送る (opt-out)。未送出だと xAI 側の既定
///   (4.3 = 常時思考 / 4.5 = high) が適用され、思考が max_tokens (合算上限) を食い潰して
///   空デルタ/タイムアウトになる (grok-4.3 実測、spec 12 Phase D)。
/// - **o 系**: 未設定なら `low`。
/// - それ以外 (grok-4-1-fast 系・gpt-4o・ローカル互換): 送らない。
///
/// LLM_EFFORT 明示は尊重し、xhigh/max は未対応プロバイダ向けに `high` へ丸める。
pub(crate) fn reasoning_effort(
    model: &str,
    effort: Option<Effort>,
    sends_function_tools: bool,
) -> Option<&'static str> {
    if model.starts_with("gpt-5") {
        return sends_function_tools.then_some("none");
    }
    let is_43 = model.starts_with("grok-4.3");
    let is_45 = model.starts_with("grok-4.5");
    if !is_43 && !is_45 && !is_o_series(model) {
        return None;
    }
    Some(match effort {
        None => {
            if is_43 {
                "none"
            } else {
                "low"
            }
        }
        Some(Effort::Low) => "low",
        Some(Effort::Medium) => "medium",
        Some(Effort::High) | Some(Effort::XHigh) | Some(Effort::Max) => "high",
    })
}

/// 出力上限で何も成立しなかった応答の検出 (spec 12 Phase D・rev4 Should c の判定条件を継承)。
///
/// **text 空 かつ tool_calls 空 かつ finish == Length** のときだけ
/// [`LlmError::OutputTruncated`] にする。原因は 2 通り — 推論モデルが budget を全部思考に
/// 使い切った場合と、生成物 (長い narration・大きな ops 配列) が上限で切れた場合。
///
/// **どちらも同じ入力なら同じ所で切れる**ので、以前のように `EmptyResponse` へ畳んで
/// 一過性 (再抽選) に乗せない — バックオフと課金だけが増え、画面には「空の応答」としか
/// 出ないので受領者が上限に当たったことに辿り着けない。`limit` を載せるのは、次の一手が
/// 「LLM_MAX_TOKENS をいくつまで上げるか」だから (現在値が分からないと上げ幅を決められない)。
///
/// length 以外の空 (通常の空応答) はここでは弾かない — 従来どおり generate / extract が surface。
pub(crate) fn reject_empty_reasoning(
    resp: ChatResponse,
    limit: u32,
) -> Result<ChatResponse, LlmError> {
    let text_empty = resp.text.as_deref().map_or(true, |t| t.trim().is_empty());
    if text_empty && resp.tool_calls.is_empty() && resp.finish == Finish::Length {
        return Err(LlmError::OutputTruncated { limit });
    }
    Ok(resp)
}

/// 400 の本文が `tool_choice` を名指ししているか (純粋)。
///
/// Meta は `only "auto" is supported for \`tool_choice\`. "none", "required", and named
/// function choices are not currently supported` と返す (`param: "tool_choice"`)。
/// **本文とパラメータ名の両方を見ない** — 互換サーバはエラー JSON の形が揃っておらず、
/// `param` を持たないものがあるので、部分文字列一致のほうが射程が広い。
/// 誤検知しても代償は「一段降格して 1 回再送する」だけで、降格先でも通れば実害はない。
pub(crate) fn blames_tool_choice(body: &str) -> bool {
    body.to_ascii_lowercase().contains("tool_choice")
}

/// OpenAI 互換 wire → canonical。
///
/// tool_calls の arguments (**JSON 文字列**) はここで **1 回だけ** parse して以後は
/// オブジェクトとして運ぶ (写経元 D2 — 二重エンコード/未パースの取り違えを境界で殺す)。
/// 壊れた arguments は **raw を保持した** Parse エラー (#34 同型・再生成の燃料)。
pub(crate) fn decode(resp: wire::ChatResponse) -> Result<ChatResponse, LlmError> {
    let usage = resp
        .usage
        .as_ref()
        .map(|u| Usage {
            prompt: u.prompt_tokens,
            completion: u.completion_tokens,
            cache_read: u
                .prompt_tokens_details
                .as_ref()
                .map(|d| d.cached_tokens)
                .unwrap_or(0),
        })
        .unwrap_or_default();

    let Some(choice) = resp.choices.into_iter().next() else {
        return Ok(ChatResponse { text: None, tool_calls: Vec::new(), finish: Finish::Other, usage });
    };

    let finish = match choice.finish_reason.as_deref() {
        Some("stop") => Finish::Stop,
        Some("tool_calls") => Finish::ToolUse,
        Some("length") => Finish::Length,
        _ => Finish::Other,
    };

    let mut tool_calls = Vec::new();
    for call in choice.message.tool_calls {
        let raw = call.function.arguments;
        let args = serde_json::from_str(&raw)
            .map_err(|source| LlmError::Parse { source, raw: raw.clone() })?;
        tool_calls.push(ToolCall {
            id: call.id.unwrap_or_default(),
            name: call.function.name.unwrap_or_default(),
            args,
        });
    }

    Ok(ChatResponse { text: choice.message.content, tool_calls, finish, usage })
}

/// `ToolMode::Auto` 用の指示文。**ツール利用を第一に促しつつ**、使わなかった場合の形も
/// 示す (`tool_choice` を強制できないサーバでは、モデルが content に書く道を選べるため)。
///
/// `ops` が配列であることを名指しするのは、実機の崩れが**まさにそこ**だったから
/// (Meta が `"ops": "\n"` を出した — schema を知らないまま tool 定義の形を推測した結果)。
/// `parse::fix_ops_as_string` (#40) が救済する崩れだが、**救済に頼る前に起こさせない**。
pub(crate) fn tool_or_json_instruction(schema: &serde_json::Value) -> String {
    format!(
        "出力の形式について: ツール `emit_delta` が使えます。**可能な限りツール呼び出しで提出**してください。\
        このサーバはツールの強制指定に対応していないため、ツールを使わずに本文で返す場合は、\
        次の JSON Schema に厳密に従う JSON オブジェクトを **1つだけ** 出力し、前置き・説明・\
        コードフェンスのラベル等を含めないでください。**`ops` は必ず配列です — 変化が無いターンは \
        `\"ops\": []` と書き、文字列や null にしないでください。**\n\
        JSON Schema:\n{}",
        serde_json::to_string(schema).unwrap_or_default()
    )
}

/// no-tools モードで「schema に従う JSON だけを出力せよ」と指示する system メッセージ本文。
/// tool_choice 非対応サーバ (さくら AI Engine / ローカル OpenAI 互換) 向け。schema は単一真実源。
pub(crate) fn json_instruction(schema: &serde_json::Value) -> String {
    format!(
        "重要: このサーバはツール呼び出し (function calling) に対応していません。\
        応答は次の JSON Schema に厳密に従う JSON オブジェクトを **1つだけ** 出力し、\
        前置き・説明・コードフェンスのラベル等、余計なテキストを一切含めないでください。\n\
        JSON Schema:\n{}",
        serde_json::to_string(schema).unwrap_or_default()
    )
}
