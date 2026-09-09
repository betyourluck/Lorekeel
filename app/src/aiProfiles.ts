/**
 * 登録モデル (AiModelProfile) の保存と突き合わせ — **純関数だけ**。
 *
 * ストア本体から出してあるのは、ここが提示層で唯一テストできる部分だから
 * (`tour.ts` / `editorPath.ts` / `map.ts` と同じ枠)。`stores/game.ts` に置いたままだと、
 * テストが i18n と transport ごと引き込んで localStorage / window に触れて落ちる。
 */

// プロファイルから選んで「決定」で .env へ反映する形にする。**.env の書き込みは決定時のみ**
// (選択変更だけでは書かない)。API キーは平文で localStorage に入る (BYO-key・ローカル app)。
const AI_PROFILES_KEY = "kataribe.aiModelProfiles";
export interface AiModelProfile {
  id: string; // アプリ生成の主キー (name 重複を許すため)
  name: string; // 表示名 (重複可)
  model: string; // LLM_MODEL
  baseUrl: string; // LLM_BASE_URL
  apiKey: string; // LLM_API_KEY (平文・表示時マスク)
  useTools: boolean; // LLM_USE_TOOLS (ツール呼び出し)
  // 以下 2 つは **モデルごとに変えたい調整** (2026-09-10 ユーザー要望「他のモデルでは
  // LLM_EFFORT を効かせて Opus では効かせない」)。どちらも **空文字 = 未設定**で、
  // backend が空を書くと env_opt が None に落とすので「送らない」を表せる。
  effort: string; // LLM_EFFORT ("" | low | medium | high | xhigh | max)。**GM だけに効く**
  maxTokens: string; // LLM_MAX_TOKENS ("" = llm_client の既定 4096)。思考は**この上限を食う**
}
// localStorage から読む (壊れていれば空)。全項目を型で検査し、欠けは既定で補う (前方互換)。
export function loadAiProfiles(): AiModelProfile[] {
  try {
    const raw = localStorage.getItem(AI_PROFILES_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((p) => p && typeof p.id === "string" && typeof p.name === "string")
      .map((p) => ({
        id: p.id,
        name: p.name,
        model: typeof p.model === "string" ? p.model : "",
        baseUrl: typeof p.baseUrl === "string" ? p.baseUrl : "",
        apiKey: typeof p.apiKey === "string" ? p.apiKey : "",
        useTools: p.useTools !== false, // 既定 true
        // 前方互換: 2026-09-10 より前の登録には欄が無い = 未設定 (従来の挙動そのもの)。
        effort: typeof p.effort === "string" ? p.effort : "",
        maxTokens: typeof p.maxTokens === "string" ? p.maxTokens : "",
      }));
  } catch {
    return [];
  }
}
export function saveAiProfiles(list: AiModelProfile[]) {
  localStorage.setItem(AI_PROFILES_KEY, JSON.stringify(list));
}
// アプリ側の主キー生成 (name 重複を許すため)。WebView2 は crypto.randomUUID 対応。
export function newProfileId(): string {
  try {
    return crypto.randomUUID();
  } catch {
    return `p_${Date.now()}_${Math.floor(Math.random() * 1e9)}`;
  }
}
// プロファイルが現在の .env 設定と一致するか (初期表示で選択状態を復元する判定)。
// name/id は .env に無いので、**その登録が書く欄すべて**を突き合わせる。
// effort / maxTokens も含めるのは、含めないと「Opus を選択中」と見せながら思考の深さだけ
// 別の値が生きている、という嘘を作れるため (2026-09-07 に EDITOR_LLM で直したのと同型 —
// .env の手書きが有効なのに『GM と同じ』に見せない、の一族)。
export function profileMatchesConfig(
  p: AiModelProfile,
  cfg: {
    base_url: string;
    model: string;
    api_key: string;
    use_tools: boolean;
    effort?: string;
    max_tokens?: string;
  },
): boolean {
  return (
    p.baseUrl.trim() === cfg.base_url.trim() &&
    p.model.trim() === cfg.model.trim() &&
    p.apiKey.trim() === cfg.api_key.trim() &&
    p.useTools === cfg.use_tools &&
    p.effort.trim() === (cfg.effort ?? "").trim() &&
    p.maxTokens.trim() === (cfg.max_tokens ?? "").trim()
  );
}
