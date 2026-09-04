import { defineStore } from "pinia";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { tableHooks, transport } from "../transport";
import { t } from "../i18n";
import type { EditorVocabulary } from "../editorCompletion";
import { fileToWebp } from "../imageConvert";
import * as tts from "../tts";
import {
  currentSlot,
  imageStamp,
  loadImageGenSettings,
  saveImageGenSettings,
  toBackendConfig,
  type ImageGenSettings,
  type ProviderSlot,
} from "../imageGen";
import type {
  GameView,
  TurnView,
  StateView,
  LogEntry,
  CharacterView,
  RemoteList,
  InstalledPackage,
  PackageUpdate,
  UpdateResult,
  DecisionView,
  DecisionResultView,
  ContestView,
  ContestRoundView,
  StatRollView,
  SynopsisView,
  LogLineView,
  SlotView,
  MapView,
  FactView,
  FactsOpView,
} from "../types/api";

// d100 ロールアンダーの成功度 (spec 16) の表示ラベル。内部 id は英語 (ログ検索・セーブ安定)、
// 表示はこの言語表で差し替え可能。未知 id は素通し (前方互換)。
export function degreeLabel(degree: string): string {
  const key = `log.degree${degree.charAt(0).toUpperCase()}${degree.slice(1)}`;
  const label = t(key);
  return label === key ? degree : label;
}

// 可変量ダイス (roll_stat) の監査行 (spec 16): 「player SAN -4 (1d6=4)」。
export function statRollLine(sr: StatRollView): string {
  const bonus = sr.bonus !== 0 ? (sr.bonus > 0 ? `+${sr.bonus}` : `${sr.bonus}`) : "";
  const amount = sr.amount >= 0 ? `+${sr.amount}` : `${sr.amount}`;
  return `${sr.entity} ${sr.key} ${amount} (${sr.count}d${sr.sides}${bonus}=${sr.rolls.join("+")})`;
}

// アセット ID → asset:// URL のキャッシュ (spec 23 Phase A: DTO は絶対パスでなく ID を運ぶ)。
// 解決は resolve_asset_path command (**transport seam の外** — 各クライアントが自分の
// ローカルパッケージで解決する Multiplayer 契約 asset_wire)。key = `${kind}/${id}`。
// 解決失敗 (ファイル不在・不正 ID) は null を覚え、表示スキップでプレイ継続。
// ID はパッケージ内でのみ一意なので、パッケージが替わる applyGameView でクリアする。
type AssetKind = "images" | "audios";
const assetUrlCache = new Map<string, string | null>();

/** prefetch 済みの ID を同期で URL に引く (未 prefetch / 解決失敗は null = 表示スキップ)。 */
export function assetUrl(kind: AssetKind, id: string | null | undefined): string | null {
  if (!id) return null;
  return assetUrlCache.get(`${kind}/${id}`) ?? null;
}

/** view/turn に載っている ID 群を先に解決してキャッシュへ (以後の assetUrl は同期で引ける)。 */
async function prefetchAssets(
  entries: Array<[AssetKind, string | null | undefined]>,
): Promise<void> {
  const wanted = new Set<string>();
  for (const [kind, id] of entries) {
    if (id && !assetUrlCache.has(`${kind}/${id}`)) wanted.add(`${kind}/${id}`);
  }
  await Promise.all(
    [...wanted].map(async (key) => {
      const slash = key.indexOf("/");
      const kind = key.slice(0, slash);
      const id = key.slice(slash + 1);
      try {
        const p = await invoke<string | null>("resolve_asset_path", { kind, id });
        assetUrlCache.set(key, p ? convertFileSrc(p) : null);
      } catch {
        assetUrlCache.set(key, null); // セッション未開始等 — 表示スキップで続行
      }
    }),
  );
}

/** GameView/TurnView 共通のアセット欄 (背景/BGM/顔アイコン/マップ CG) を集める。 */
function collectViewAssets(v: {
  background?: string | null;
  bgm?: string | null;
  present_characters?: Array<{ icon: string | null }>;
  map?: MapView | null;
}): Array<[AssetKind, string | null | undefined]> {
  const out: Array<[AssetKind, string | null | undefined]> = [
    ["images", v.background],
    ["audios", v.bgm],
  ];
  for (const c of v.present_characters ?? []) out.push(["images", c.icon]);
  for (const n of v.map?.nodes ?? []) out.push(["images", n.image]);
  return out;
}

/** ビート (イベント CG/SE) と判定 (結末 SE) のアセット欄を集める。 */
function collectTurnAssets(
  beats: Array<{ image?: string | null; sound?: string | null }>,
  checks: Array<{ sound?: string | null }>,
): Array<[AssetKind, string | null | undefined]> {
  const out: Array<[AssetKind, string | null | undefined]> = [];
  for (const b of beats) {
    out.push(["images", b.image]);
    out.push(["audios", b.sound]);
  }
  for (const c of checks) out.push(["audios", c.sound]);
  return out;
}

// localStorage キー: ユーザーが選べるパッケージフォルダのパス一覧 (配布物の置き場)。
const PACKAGES_KEY = "kataribe.packagePaths";
// 前回プレイした (new_game/resume/ロードで実際に開いた) パッケージのパス。
// 起動時のコンボリスト初期選択に使う (ユーザーFB 2026-07-21)。
const LAST_PLAYED_KEY = "kataribe.lastPlayedPackage";
function loadLastPlayed(): string {
  return localStorage.getItem(LAST_PLAYED_KEY) || "";
}
function saveLastPlayed(path: string) {
  localStorage.setItem(LAST_PLAYED_KEY, path);
}
// 前回追加したパッケージの「親フォルダ」。参照ダイアログの初期ディレクトリに使う
// (多くの人は同じ親フォルダの下に複数パッケージを置くので、次回そこから選べる)。
const LAST_PKG_PARENT_KEY = "kataribe.lastPackageParent";
function loadLastPackageParent(): string {
  return localStorage.getItem(LAST_PKG_PARENT_KEY) || "";
}
/** パスの親フォルダを返す (Windows `\` と Unix `/` の両区切りに対応、末尾区切りは無視)。 */
function parentDir(path: string): string {
  const p = path.trim().replace(/[/\\]+$/, "");
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i > 0 ? p.slice(0, i) : "";
}
// 背景の明るさ (0=暗幕最大で真っ暗 〜 100=暗幕なしで画像そのまま)。既定は中間の 50。
const BG_BRIGHTNESS_KEY = "kataribe.bgBrightness";
function loadBgBrightness(): number {
  const v = Number(localStorage.getItem(BG_BRIGHTNESS_KEY));
  return Number.isFinite(v) && v >= 0 && v <= 100 ? v : 50;
}
/**
 * 会話ペイン (物語の舞台) の配色。**アプリのテーマとは独立**に持つ。
 *
 * 動機: 舞台は語りが読まれる場所で、UI クローム (ヘッダ・卓バー・入力欄) とは
 * 求めるものが違う。ライト/ダークを切り替えるたびに舞台の見え方まで変わると、
 * 同じ盤面が別物に見えてしまう (ユーザーFB 2026-07-23「テーマが変わったときに
 * 影響を受け易すぎる」)。**背景画像のある盤面では従来から dark に固定**していたので、
 * 既定 `dark` はその一貫化でもある。`auto` を選べば従来どおりテーマに従う。
 */
export type PaneTheme = "dark" | "light" | "auto";
const PANE_THEME_KEY = "kataribe.paneTheme";
function loadPaneTheme(): PaneTheme {
  const v = localStorage.getItem(PANE_THEME_KEY);
  return v === "light" || v === "auto" ? v : "dark";
}

// 音量 0..100 (BGM ループと SE one-shot に共通でかかる)。既定は中間の 50。
const AUDIO_VOLUME_KEY = "kataribe.audioVolume";
const AUDIO_MUTED_KEY = "kataribe.audioMuted";
function loadAudioVolume(): number {
  const v = Number(localStorage.getItem(AUDIO_VOLUME_KEY));
  return Number.isFinite(v) && v >= 0 && v <= 100 ? v : 50;
}
function loadAudioMuted(): boolean {
  return localStorage.getItem(AUDIO_MUTED_KEY) === "true";
}
// ダイスの開帳演出 (spec 18 Phase A)。既定 on。off = 従来の即時開示 (作者テスト向け)。
/** 多人数プレイの卓状態 (spec 23 Phase C)。solo = 従来どおり。 */
export interface MultiState {
  role: "solo" | "host" | "guest";
  roomCode: string;
  /** set_participants 済み = 入力窓運用に入った。 */
  started: boolean;
  /** 自分の peer_id (提出・宛先別 view の鍵)。 */
  myPeerId: string;
  /** ホストの表示名 (ゲスト画面用)。 */
  hostName: string;
  /** 席一覧 (ホスト画面用。hello 済みのゲスト + 自分)。 */
  seats: { peerId: string; displayName: string; packageMatch: string; connected: boolean; entityId: string }[];
  /** 入力窓の現況 (「入力待ち: ○○」)。 */
  inputStatus: { submitted: string[]; waiting: string[] } | null;
  /** パッケージ照合の警告 (mismatch/unknown。プレイは止めない)。 */
  packageWarning: string | null;
  /** 接続状態 (ゲスト: ホストへの DataChannel)。 */
  connected: boolean;
  /** reveal_order の追従位置 (この卓で既にローカル適用した開帳数)。 */
  revealApplied: number;
  /** タイマー残り秒 (timer_sync 受信 / ホストのカウントダウン)。null = タイマー無し。 */
  timerRemaining: number | null;
  /** 確定した割り当て (卓開始後)。entity → 操作プレイヤー名 + 席色 (可視化用)。
   *  `peerId` は音声レベルを席へ写すための鍵 (ホストは "host" 固定)。 */
  assignments: { peerId: string; entityId: string; displayName: string; color: string }[];
  /**
   * パッケージ中継の現況 (契約 `package_relay`)。ホストは預ける側、ゲストは受け取る側。
   * `off` = 中継を使わない卓 (未着手・手動選択の fallback) / `failed` = サーバ不達等で
   * 手動選択へ落ちた。プレイ自体は止めない。
   */
  relay: "off" | "uploading" | "downloading" | "ready" | "failed";
  /** 自動再接続の試行回数 (null = 再接続していない)。切れたら黙って諦めず取りに行く。 */
  reconnecting: number | null;
  /** マイクが入っているか (spec 23 Phase D)。**OFF は完全解放** = デバイスを掴んでいない。 */
  micOn: boolean;
  /** 発話レベル (entityId → 0..1)。席色リングの脈動の素材。~12Hz で更新。 */
  voiceLevels: Record<string, number>;
}

/** 編集モード (spec 28 Phase A)。骨格は「backend に編集ルートを登録し、相対パスだけで
 *  読み書きする」— frontend は表示とダーティ管理だけを持つ。 */
export interface EditorState {
  /** 編集モード中か (鉛筆トグル)。 */
  on: boolean;
  /** 編集対象のパッケージパス (open した瞬間の packagePath を凍結。プレイ中トーストの照合にも使う)。 */
  root: string;
  /** 正規化した絶対パス (backend 由来)。メディアのサムネイル URL の起点。 */
  absRoot: string;
  /** ファイル一覧 (backend list_files の写し。カテゴリでグルーピング表示)。 */
  files: { relPath: string; category: string }[];
  /** 書庫由来か (`.kataribe_source.json` あり) — バッジ + 初回保存のフォーク確認。 */
  fromSite: boolean;
  /** 開いているファイルの相対パス ("" = 未選択)。 */
  current: string;
  /** エディタ本文 (v-model)。 */
  text: string;
  /** 最後に保存 (または読み込み) した本文。text との差 = ダーティ。 */
  savedText: string;
  /** 保存の往復中 (Ctrl+S 連打の二重送信防止)。 */
  saving: boolean;
  /** 層 2 診断 (spec 28 Phase B): パッケージ全体の inspect 結果。編集モード入場時と
   *  保存成功時に更新。file が引けた行はクリックでそのファイルへ。 */
  issues: { file: string | null; severity: string; message: string }[];
  /** 補完語彙 (spec 28 Phase C)。ソースはディスクの保存済み状態 — 入場時と保存成功時に
   *  取り直す (未保存バッファの id は次の保存まで補完に出ない = spec の明記事項)。 */
  vocab: EditorVocabulary | null;
  /** 参照専用のメディア一覧 (images/ audios/ のアセット ID)。**編集対象ではない** —
   *  YAML の image/bgm/sound/icon 欄に書く名前を作者に見せるための一覧 (spec 28 追補)。 */
  media: { relPath: string; category: string }[];
  /** ファイルタブの表示 (テキスト = 編集 / メディア = 参照とアセット管理)。 */
  view: "text" | "media";
  /** メディアの版 (クロップで**名前は同じまま中身が替わる**ので、サムネイル URL の
   *  キャッシュ破棄に使う。failures #86 と同型の問題)。 */
  mediaRev: number;
  /** 開いているファイルの元の改行コード。**CodeMirror は内部を常に LF に正規化する**ので、
   *  CRLF のまま比較すると「開いただけで ●」になる (実機で発覚)。読み込みで覚えて LF 化し、
   *  保存で戻す — ディスクの改行を変えない (変えると tree_hash と diff が荒れる)。 */
  eol: "\n" | "\r\n";
}

export function freshEditorState(): EditorState {
  return {
    on: false,
    root: "",
    absRoot: "",
    files: [],
    fromSite: false,
    current: "",
    text: "",
    savedText: "",
    saving: false,
    issues: [],
    vocab: null,
    media: [],
    view: "text",
    mediaRev: 0,
    eol: "\n",
  };
}

/** 席色 (participants 宣言順)。青=1人目 / 赤=2人目 / 黄=3人目… (ユーザーFB 2026-07-23)。 */
export const SEAT_COLORS = ["#3b82f6", "#ef4444", "#eab308", "#22c55e", "#a855f7"];

export function freshMultiState(): MultiState {
  return {
    role: "solo",
    roomCode: "",
    started: false,
    myPeerId: "",
    hostName: "",
    seats: [],
    inputStatus: null,
    packageWarning: null,
    connected: false,
    revealApplied: 0,
    timerRemaining: null,
    assignments: [],
    relay: "off",
    reconnecting: null,
    micOn: false,
    voiceLevels: {},
  };
}

const DICE_REVEAL_KEY = "kataribe.diceReveal";
function loadDiceReveal(): boolean {
  return localStorage.getItem(DICE_REVEAL_KEY) !== "false";
}
// --- 本文テキスト設定 (GM の語りの見た目。提示層のみ・localStorage 永続) ---
const MSG_FONT_KEY = "kataribe.msgFont";
// 本文 (会話ログ) の文字サイズ px。**0 = UI に合わせる** (既定・従来挙動) — UI の基準
// フォント (kataribe.fontScale = root の font-size) と独立に選べる (2026-08-31 ユーザーFB
// 「UI と本文がまとめて大きくなるのを分けたい」)。適用は会話ログ container の inline
// fontSize 1 箇所 — 語り系の段落はサイズクラスを持たず container を継承し、メタラベル
// (text-xs 等) は rem = UI 側に残るので、本文だけが独立して動く。
const MSG_SIZE_KEY = "kataribe.msgSize";
// spec 28: エディタ本文の文字サイズ (3 段: 0=小 1=中 2=大)。**既定は中** (ユーザーFB
// 2026-08-28 — 従来の 13px は「小」に相当し、既定としては小さすぎた)。
const EDITOR_FONT_KEY = "kataribe.editorFontStep";
export const EDITOR_FONT_SIZES = [13, 15, 18] as const;
function loadEditorFontStep(): number {
  const v = Number(localStorage.getItem(EDITOR_FONT_KEY));
  return Number.isInteger(v) && v >= 0 && v <= 2 ? v : 1; // 既定 = 中
}
const MSG_COLOR_KEY = "kataribe.msgColor";
const MSG_SHADOW_KEY = "kataribe.msgShadow";
const AUTHORED_COLOR_KEY = "kataribe.authoredColor";
/** 既定の本文色 (tailwind の parchment)。カラーピッカーの初期値と「既定に戻す」に使う。 */
export const DEFAULT_MSG_COLOR = "#e8ddc8";
/**
 * 既定の**システム文**の色 (tailwind の glow 寄り)。シナリオ側が示す文 (場所の説明・
 * 結末文・判定の結末) は GM の即興と混ぜると読み分けられないので、既定から別色にする。
 *
 * **id は `authored`・表示名は「システム文」**(この repo の id=機械用キー / title=表示名の
 * 流儀)。id が provenance を指すのは、**どの文をこの色にするかの判定規則が「作者が書いたか」
 * だから** — 一方プレイヤーから見えるのは「システムが示す文」という役割なので、UI では
 * そちらを名乗る。なお `kind: "system"` は別物 (⚠ 警告・章替わり等の中央寄せ標識行)。
 */
export const DEFAULT_AUTHORED_COLOR = "#f0d9a8";
/** 本文フォントの選択肢 (id → CSS font-family)。OS 同梱フォントへのフォールバック連鎖で環境差を吸収。 */
export const MESSAGE_FONTS: { id: string; label: string; family: string }[] = [
  { id: "default", label: "標準 (UI と同じ)", family: "" },
  {
    id: "mincho",
    label: "明朝",
    family: '"Yu Mincho", "游明朝", "Hiragino Mincho ProN", "MS PMincho", serif',
  },
  {
    id: "gothic",
    label: "ゴシック",
    family: '"Yu Gothic", "游ゴシック", "Hiragino Kaku Gothic ProN", "Meiryo", sans-serif',
  },
  {
    id: "maru",
    label: "丸ゴシック",
    family: '"HG丸ｺﾞｼｯｸM-PRO", "Hiragino Maru Gothic ProN", "Yu Gothic", sans-serif',
  },
];
function loadMsgFont(): string {
  const v = localStorage.getItem(MSG_FONT_KEY) || "default";
  return MESSAGE_FONTS.some((f) => f.id === v) ? v : "default";
}
function loadMsgSize(): number {
  const v = Number(localStorage.getItem(MSG_SIZE_KEY));
  return Number.isFinite(v) && v >= 12 && v <= 32 ? v : 0; // 範囲外・未設定 = UI に合わせる
}
function loadMsgColor(): string {
  return localStorage.getItem(MSG_COLOR_KEY) || "";
}
function loadMsgShadow(): number {
  const v = Number(localStorage.getItem(MSG_SHADOW_KEY));
  return Number.isFinite(v) && v >= 0 && v <= 100 ? v : 0;
}
// ビート (✦) / 想起 (┊) ブロックに敷く黒背景の濃さ 0..100 (0=なし)。色付き文字が
// 背景画像に溶けて読みにくい問題への手当て。本文 (語り) には敷かない。
const BEAT_BG_KEY = "kataribe.beatBgOpacity";
function loadBeatBgOpacity(): number {
  const v = Number(localStorage.getItem(BEAT_BG_KEY));
  return Number.isFinite(v) && v >= 0 && v <= 100 ? v : 40;
}

// 右ペイン (状態パネル) の幅 px。ドラッグハンドルで可変・localStorage 永続。
const PANEL_WIDTH_KEY = "kataribe.panelWidth";
export const PANEL_WIDTH_MIN = 200;
export const PANEL_WIDTH_MAX = 640;
function loadPanelWidth(): number {
  const v = Number(localStorage.getItem(PANEL_WIDTH_KEY));
  return Number.isFinite(v) && v >= PANEL_WIDTH_MIN && v <= PANEL_WIDTH_MAX ? v : 256; // 既定 w-64
}

// 会話ログのテキスト保存先フォルダ (空 = backend の既定 app_data_dir/logs)。
const LOG_DIR_KEY = "kataribe.logDir";
function loadLogDir(): string {
  return localStorage.getItem(LOG_DIR_KEY) || "";
}

// 初回起動時のパッケージ一覧は **空**。
//
// かつて `packages/escape` を「同梱パッケージ」の既定として置いていたが、**同梱していない**
// (`bundle.resources` は未設定でインストーラーは packages/ を含まない)。repo 直下で
// 開発している間だけ相対パスが解決していたので気づけず、まっさらな環境にインストーラーで
// 入れると**存在しないシナリオが一覧に居座る**状態になっていた (2026-07-23 ユーザー報告)。
// 開発環境が「配布物には無いもの」を暗黙に持っている、という #73 (dev では CSP が効かない)
// と同じ形の非対称。一覧は書庫から取るか手で追加するものなので、既定は空が正しい。

// 起動時のコンボリスト初期選択: 前回プレイしたパッケージ > 表示の一番上。
// コンボリストは「新しい順」(追加順の逆) で表示するので、一番上 = packagePaths の末尾要素
// (App.vue の packagesNewestFirst と対の知識 — 表示順を変えるならここも揃えること)。
function initialPackagePath(paths: string[]): string {
  const last = loadLastPlayed();
  if (last && paths.includes(last)) return last;
  return paths[paths.length - 1] ?? "";
}

// --- AI モデルプロファイル (複数の LLM 設定を登録・切替。localStorage 永続) ---
// 動機: ヘビーユーザーは複数モデルを試す。従来は .env を手で書き換えていたのを、登録済み
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
// name/id は .env に無いので接続を決める 4 項目 (trim 済) で突き合わせる。
export function profileMatchesConfig(
  p: AiModelProfile,
  cfg: { base_url: string; model: string; api_key: string; use_tools: boolean },
): boolean {
  return (
    p.baseUrl.trim() === cfg.base_url.trim() &&
    p.model.trim() === cfg.model.trim() &&
    p.apiKey.trim() === cfg.api_key.trim() &&
    p.useTools === cfg.use_tools
  );
}

// --- 配布サイト「Kataribe 書庫」(spec 05 Phase C) ---
// サイト URL は設定項目 (既定 = 公式)。自前サーバも指せる = Outcasts 固有ロックインを避ける。
const SITE_URL_KEY = "kataribe.siteUrl";
// 既定は新ホスト (2026-09-01)。**既存ユーザーは影響を受けない** — localStorage に保存済みの
// 値が優先され、既定は「一度も設定していない人」にしか効かない。
export const DEFAULT_SITE_URL = "https://lorekeel.outcasts.jp";
/**
 * 公式書庫の**同一実体を指すホスト名**。改名 (2026-08-28) で新ホストを足し、旧ホストは
 * 退役させていない (同じ DB を配っている)。
 *
 * 既定を新ホストへ替えた以上、旧ホストから取得したパッケージ (出所メタの `site_url` が旧名)
 * を持つ人が新既定に載ると、**「取得済み」判定が外れて同じ配布物を `_2` で二重に据える**。
 * 更新照会側の同名の表は backend (`app/src-tauri/src/lib.rs` の `ARCHIVE_ALIASES`) にあり、
 * **増やすときは両方触ること** (deployment の事実なので型からは導けない)。
 */
const ARCHIVE_ALIASES = ["https://kataribe.outcasts.jp", "https://lorekeel.outcasts.jp"];
/** 別名を正規形へ畳む。**完全一致の表引き**（置換ではない = 接尾辞細工は畳まれない）。 */
function canonicalSite(url: string): string {
  const u = url.replace(/\/+$/, "");
  return ARCHIVE_ALIASES.includes(u) ? ARCHIVE_ALIASES[1] : u;
}
function loadSiteUrl(): string {
  return localStorage.getItem(SITE_URL_KEY) || DEFAULT_SITE_URL;
}
/** 書庫の固定 6 カテゴリ (outcast Spec 23。id はサーバのキー、label は表示名)。 */
export const SITE_CATEGORIES: { id: string; label: string }[] = [
  { id: "", label: "すべて" },
  { id: "mystery", label: "推理ゲーム" },
  { id: "escape", label: "脱出ゲーム" },
  { id: "daily", label: "現代日常" },
  { id: "horror", label: "ホラー" },
  { id: "fantasy", label: "ファンタジー" },
  { id: "sf_cyber", label: "SF・サイバー" },
];

// backend `list_packages` が返す1項目 (フォルダ一覧表示用)。
export interface PackageEntry {
  path: string;
  title: string;
  description: string;
  playable: boolean; // manifest が読めれば true (単発・campaign-entry 双方)。読込エラー時のみ false
  error: string | null;
  // オートセーブが在ればその時点のターン数 (「続きから (turn N)」ボタンの提示素)。無ければ null。
  autosave_turn: number | null;
  // 手動セーブスロット (spec 07 Phase D) が 1 つでも在るか (削除確認に使う)。
  has_slots: boolean;
  // 出所メタ (spec 17) が在れば取得元サイト / 書庫 id。手動配置・自作は null。
  source_site: string | null;
  source_id: string | null;
}

// localStorage からパス一覧を読む (未設定・壊れていれば空 = まっさらな状態)。
function loadPaths(): string[] {
  try {
    const raw = localStorage.getItem(PACKAGES_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed) && parsed.every((p) => typeof p === "string")) return parsed;
    }
  } catch {
    /* 壊れた localStorage は無視して既定へ */
  }
  return [];
}
function savePaths(paths: string[]) {
  localStorage.setItem(PACKAGES_KEY, JSON.stringify(paths));
}

interface GameState {
  started: boolean;
  title: string;
  log: LogEntry[];
  state: StateView | null;
  loading: boolean;
  error: string | null;
  // 現在の背景画像 (asset:// URL)。場所/イベントで差し替え。無ければ null。
  background: string | null;
  // 現在地のループ BGM (asset:// URL)。場所変化で差し替え。無ければ null。
  bgm: string | null;
  // 現在地に居る NPC (顔アイコン行)。icon は asset:// URL 化済み。
  presentCharacters: CharacterView[];
  // 背景の明るさ 0..100 (大きいほど画像が明るく見える=暗幕が薄い)。グラフィック設定。
  bgBrightness: number;
  /** 会話ペインの配色 (アプリのテーマと独立。既定 dark)。 */
  paneTheme: PaneTheme;
  // 本文フォント (MESSAGE_FONTS の id)。表示設定。
  msgFont: string;
  // 本文の文字サイズ px (0 = UI に合わせる)。UI の基準フォントと独立。表示設定。
  msgSize: number;
  // 本文の文字色 (hex)。空 = テーマ既定 (parchment)。表示設定。
  msgColor: string;
  // 作者が書いた確定文の色 (空 = 既定)。
  authoredColor: string;
  // 本文の影の濃さ 0..100 (0=なし)。背景画像の上の可読性向上。表示設定。
  msgShadow: number;
  // ビート (✦) / 想起 (┊) ブロックの黒背景の濃さ 0..100 (0=なし)。表示設定。
  beatBgOpacity: number;
  // --- ダイスの開帳 (spec 18 Phase A・提示層のみ) ---
  // 開帳演出のオン/オフ (localStorage 永続)。off = 従来の即時開示。
  diceReveal: boolean;
  // 開帳待ちダイスの後ろに積まれるはずだった行 (ビート/goal バナー/エピローグ等)。
  // 結果を先に漏らさないため全開帳まで保留し、開帳完了で flush する。frontend 揮発。
  pendingTail: LogEntry[];
  // 保留行に付随する SE (ビート効果音)。flush 時に one-shot 再生。
  pendingSe: (string | null)[];
  // 保留中の見た目 (イベント CG 背景 / BGM)。発火 CG は結果の漏洩そのものなので開帳まで遅延。
  pendingVisual: { background: string | null; bgm: string | null } | null;
  // 開帳待ちの読み上げ (エピローグ)。**結末を語る文なので、ダイスが伏せられたまま
  // 喋ると出目の帰結を音声で漏らす** — pendingTail と同じ契機で解き放つ。
  pendingSpeech: string | null;
  // --- 決断つき判定 (spec 18 Phase B) ---
  // 決断待ち (受け入れ/押す/払う)。非 null の間は入力を締め、全開帳後に決断パネルを出す。
  decision: DecisionView | null;
  // 決断の確定を backend に送信中 (パネルのボタンを二重押しさせない)。
  deciding: boolean;
  // 進行中の対決 (spec 18 Phase C)。非 null の間 ⚔ パネルを出し入力を締める。
  contest: ContestView | null;
  // ラウンド送信中 (⚔ ボタンの二重押し防止)。
  fighting: boolean;
  // --- 多人数プレイ (spec 23 Phase C) ---
  multi: MultiState;
  // 右ペイン (状態パネル) の幅 px。ドラッグハンドルで可変。
  panelWidth: number;
  // 音量 0..100 (BGM/SE 共通)。サウンド設定。
  audioVolume: number;
  // ミュート (true なら音を出さない)。サウンド設定。
  audioMuted: boolean;
  // コンボリストで選択中のパッケージのパス (次に開始/ロードする対象)。
  packagePath: string;
  // いま実際にプレイ中のゲームのパッケージパス (session の真実)。コンボリストの選択
  // (packagePath) とは独立 — 選択だけ切り替えても動かない。セーブはこのゲームに対して行う
  // ので、packagePath がこれと食い違う間はセーブを無効化する (保存先の取り違え防止)。
  // プレイ前は空。new_game/resume/load_slot が applyGameView で確定させる。
  activePackagePath: string;
  // localStorage が保持するパッケージフォルダのパス一覧。
  packagePaths: string[];
  // 前回追加したパッケージの親フォルダ (参照ダイアログの初期ディレクトリ)。無ければ空。
  lastPackageParent: string;
  // 各パスの manifest を読んだ一覧 view (backend list_packages の結果)。
  packages: PackageEntry[];
  // --- 配布サイト (spec 05 Phase C) ---
  // 書庫サイトの URL (設定項目、localStorage 永続)。
  siteUrl: string;
  // 書庫の一覧応答 (fetch 済みのページ)。未取得なら null。
  remote: RemoteList | null;
  // 一覧 fetch / 取得中フラグとエラー (ダイアログの表示分岐)。
  remoteLoading: boolean;
  remoteError: string | null;
  // 取得 (DL→展開) 中のパッケージ id。null なら待機。
  installingId: string | null;
  // --- パッケージ更新 (spec 17 Phase C) ---
  // 「更新あり」のパッケージ (path をキーに一覧行と突き合わせる)。照会失敗は沈黙 = 空のまま。
  packageUpdates: PackageUpdate[];
  // 上書き更新中のパス。null なら待機 (同時に 1 件 = backend の排他と対)。
  updatingPath: string | null;
  // 会話ログのテキスト保存先フォルダ (空 = 既定)。設定「ログ」タブで指定。
  logDir: string;
  // ログ保存/フォルダ操作の一時トースト (App.vue が数秒表示して消す)。
  logToast: string;
  /** エディタ本文の文字サイズ段 (0=小 1=中 2=大)。localStorage 永続・既定は中。 */
  editorFontStep: number;
  /** 改名の要求 (エディタヘッダの F2 / ダブルクリック → StatePanel が行内編集を開く)。
   *  **名前を打つ場所は一覧の行だけ**に保つための橋渡し — 入力欄が二箇所にあると
   *  確定/取り消しの流儀が割れる。処理したら null に戻す。 */
  editorRenameRequest: string | null;
  // --- 編集モード (spec 28 Phase A) ---
  editor: EditorState;
  // 使用中の AI モデル名 (TitleBar バッジ + OS ウィンドウタイトル)。get_llm_config から取得。
  llmModel: string;
  // 配布サイトに現在版より新しいアプリがあるか (TitleBar の「最新版があります」表示)。
  updateAvailable: boolean;
  // 配布サイトの最新版タグ (表示用。例 "v0.3.3")。
  latestVersion: string;
  // 開発者モード (KATARIBE_DEV_MODE)。ON で GM に「テストプレイ・<meta:> 質問可」を刷り込む。
  devMode: boolean;
  // キャッシュ連続 miss の警告を出したか (エッジトリガー latch。ヒット復帰で再武装)。
  cacheWarned: boolean;
  // あらすじ (spec 10)。圧縮済み章の全量 (append-only — TurnView の差分を push して伸ばす)。
  synopsis: SynopsisView[];
  // 「最近の出来事」= 未圧縮 chronicle の 1 行要約列 (あらすじタブの下段)。
  recentLog: LogLineView[];
  // 既成事実 (spec 20)。backend がスコア降順で返す全量スナップショット (既成事実タブに表示)。
  facts: FactView[];
  // 既成事実のユーザー権限 (spec 20): open=ユーザーが宣言できる / locked=非表示 (既定)。
  factsPolicy: string;
  // --- 画像生成 / 挿絵 (spec 24) — 提示層だけの状態。セーブには入らない (揮発)。 ---
  // 設定 (非秘密、localStorage)。enabled が操作列の表示条件。
  imageGen: ImageGenSettings;
  // 直近に生成した挿絵 (表示用 data URL + 書かれたプロンプト)。null = 無し。
  generatedImage: { dataUrl: string; prompt: string } | null;
  imageDirection: string;
  /** 参照ストックの現物 (spec 27 A-7)。ダイアログを開いたときに読む。 */
  refStock: { dir: string; picked: [string, number][]; skipped: [string, string][]; max: number } | null;
  /**
   * 参照ストックの版 (failures #86)。**ファイル名は前詰めで固定なのに中身は入れ替わる**ので、
   * `asset://` の URL だけではキャッシュが割れず、削除しても古い絵が出たままになる
   * (実測: ref1 を消すと B が ref1 へ詰まるのに画面は A のまま = 末尾が消えたように見える)。
   * 変更のたびに繰り上げ、サムネイルの URL に付けて割る。
   */
  refStockRev: number;
  /** ローカルファイルの変換・送信中 (spec 27 追補)。 */
  refStockBusy: boolean;
  // 生成中 (ボタン無効 + スピナー)。
  imageBusy: boolean;
  // 生成要求の世代 (古い完了を無視する frontend 側の二層目。backend は scene_seq で守る)。
  imageRequestId: number;
  // 挿絵の表示/非表示・文字の表示/非表示 (セッション内の一時状態。localStorage に持たない —
  // 次回起動で文字が消えていたら事故)。
  showGeneratedImage: boolean;
  showText: boolean;
  // 読み上げ機能を使うか (設定サウンドタブのチェックボックス、localStorage 永続・既定 OFF)。
  // 旧 use_tts (作者宣言ゲート) は 2026-08-31 に撤去 — imageGen.enabled と同型の
  // プレイヤー側スイッチ一本になった。false なら操作を一切出さない。
  ttsFeature: boolean;
  // クイックトグルの ON/OFF (会話ペイン右下、localStorage 永続、未設定は ON)。
  // 機能スイッチの内側で効く (切れば記憶する)。
  ttsEnabled: boolean;
  // backend があらすじ圧縮中 (synopsis-compacting イベント)。ローディング文言を切り替える。
  compacting: boolean;
  // backend がエピローグ生成中 (epilogue-writing イベント、spec 11)。同じくローディング文言用。
  writingEpilogue: boolean;
  // マップ (spec 15) — 訪問済み+1歩先の有向グラフ。移動/遷移で backend が差し替える。
  map: MapView;
  // 自前の確認ダイアログ (WebView2 の window.confirm は tauri://localhost の URL を出すため自作)。
  // null なら非表示。askConfirm() がこれをセットし、ConfirmDialog が OK/キャンセルで解決する。
  confirmDialog: { message: string; confirmLabel: string; noCancel?: boolean } | null;
}

// 確認ダイアログの解決子 (Pinia state に関数を持たせず、モジュールローカルで保持)。
let confirmResolver: ((ok: boolean) => void) | null = null;

export const useGameStore = defineStore("game", {
  state: (): GameState => {
    const paths = loadPaths();
    return {
      started: false,
      title: "",
      log: [],
      state: null,
      loading: false,
      error: null,
      background: null,
      bgm: null,
      presentCharacters: [],
      bgBrightness: loadBgBrightness(),
      paneTheme: loadPaneTheme(),
      diceReveal: loadDiceReveal(),
      pendingTail: [],
      pendingSe: [],
      pendingVisual: null,
      pendingSpeech: null,
      decision: null,
      deciding: false,
      contest: null,
      fighting: false,
      multi: freshMultiState(),
      msgFont: loadMsgFont(),
      msgSize: loadMsgSize(),
      msgColor: loadMsgColor(),
      authoredColor: localStorage.getItem(AUTHORED_COLOR_KEY) ?? "",
      msgShadow: loadMsgShadow(),
      beatBgOpacity: loadBeatBgOpacity(),
      panelWidth: loadPanelWidth(),
      audioVolume: loadAudioVolume(),
      audioMuted: loadAudioMuted(),
      packagePath: initialPackagePath(paths),
      activePackagePath: "",
      packagePaths: paths,
      lastPackageParent: loadLastPackageParent(),
      packages: [],
      siteUrl: loadSiteUrl(),
      remote: null,
      remoteLoading: false,
      remoteError: null,
      installingId: null,
      packageUpdates: [],
      updatingPath: null,
      logDir: loadLogDir(),
      logToast: "",
      editorFontStep: loadEditorFontStep(),
      editorRenameRequest: null,
      editor: freshEditorState(),
      llmModel: "",
      updateAvailable: false,
      latestVersion: "",
      devMode: false,
      cacheWarned: false,
      synopsis: [],
      recentLog: [],
      facts: [],
      // 既定は locked = 宣言のない盤面では既成事実タブを出さない (GM 専用の内部記憶)。
      factsPolicy: "locked",
      ttsFeature: tts.loadFeature(),
      ttsEnabled: tts.loadEnabled(),
      imageGen: loadImageGenSettings(),
      generatedImage: null,
      // この一枚への追加指示 (spec 27 B-2)。**揮発** — localStorage に入れない。恒久にしたい
      // 様式指定は設定のスタイル欄が受ける (消し忘れた一言が以後の全生成に効き続けるのを防ぐ)。
      imageDirection: "",
      refStock: null,
      refStockRev: 0,
      refStockBusy: false,
      imageBusy: false,
      imageRequestId: 0,
      showGeneratedImage: true,
      showText: true,
      compacting: false,
      writingEpilogue: false,
      map: { nodes: [], edges: [] },
      confirmDialog: null,
    };
  },

  getters: {
    // ゴール到達済みか (入力を締める判断に使う)。
    cleared: (s): boolean => s.state?.goal_reached ?? false,
    // 編集モードで未保存の変更があるか (● マーカー / 4 契機の確認の判定)。
    editorDirty: (s): boolean =>
      s.editor.on && s.editor.current !== "" && s.editor.text !== s.editor.savedText,
    // 開帳待ちのダイスが残っているか (spec 18 Phase A: 全部開くまで入力欄を締める)。
    hasUnrevealedDice: (s): boolean =>
      s.log.some(
        (e) =>
          (e.kind === "rolls" && e.revealed < e.rolls.length) ||
          (e.kind === "checks" && e.revealed < e.checks.length) ||
          (e.kind === "statrolls" && e.revealed < e.stat_rolls.length),
      ),
    // 決断パネルを出すか (spec 18 Phase B): 決断待ちがあり、開帳がすべて済んでいる。
    // (開帳前に選択肢が見えたら、失敗したことがカードより先に漏れる。)
    showDecision(): boolean {
      return this.decision !== null && !this.hasUnrevealedDice;
    },
    // 対決パネルを出すか (spec 18 Phase C): 進行中で、開帳と決断が済んでいる。
    showContest(): boolean {
      return this.contest !== null && !this.hasUnrevealedDice && this.decision === null;
    },
    // いま開けるダイス行 (log の index)。開帳は古い方から直列 = 常に最初の未開帳 entry のみ。
    revealTargetIndex: (s): number =>
      s.log.findIndex(
        (e) =>
          (e.kind === "rolls" && e.revealed < e.rolls.length) ||
          (e.kind === "checks" && e.revealed < e.checks.length) ||
          (e.kind === "statrolls" && e.revealed < e.stat_rolls.length),
      ),
    // 会話ペインに敷く背景スタイル (画像の上に暗幕を重ねて文字可読性を確保)。
    // 暗幕の濃さは bgBrightness で可変 (明るいほど薄い暗幕)。
    backgroundStyle: (s): Record<string, string> => {
      if (!s.background) return {};
      const base = Math.max(0, Math.min(1, (100 - s.bgBrightness) / 100));
      const top = (base * 0.9).toFixed(3);
      const bot = base.toFixed(3);
      return {
        backgroundImage: `linear-gradient(rgba(20,16,12,${top}), rgba(20,16,12,${bot})), url("${s.background}")`,
        backgroundSize: "cover",
        backgroundPosition: "center",
      };
    },
    /**
     * 会話ペインに実際に敷く data-theme。背景画像がある盤面は**従来どおり dark 固定**
     * (暗幕の上に濃色文字を置かない)。`auto` は null = 上位のテーマを継ぐ。
     */
    paneThemeAttr: (s): string | null => {
      if (s.background) return "dark";
      return s.paneTheme === "auto" ? null : s.paneTheme;
    },
    // 実効音量 0..1 (BGM/SE 共通)。ミュート時は 0。<audio>.volume と new Audio に渡す。
    audioGain: (s): number => (s.audioMuted ? 0 : Math.max(0, Math.min(1, s.audioVolume / 100))),
    // 会話ログ container のスタイル (本文フォント + 本文サイズ、inherit で語り系要素へ)。
    // サイズ 0 = UI に合わせる (root の font-size を継ぐ = 従来挙動)。
    messageAreaStyle: (s): Record<string, string> => {
      const style: Record<string, string> = {};
      const family = MESSAGE_FONTS.find((f) => f.id === s.msgFont)?.family ?? "";
      if (family) style.fontFamily = family;
      if (s.msgSize > 0) style.fontSize = `${s.msgSize}px`;
      return style;
    },
    // 本文 (語り系要素) の色 + 影。inline style なので class (text-parchment 等) より優先される。
    narrationStyle: (s): Record<string, string> => {
      const style: Record<string, string> = {};
      if (s.msgColor) style.color = s.msgColor;
      if (s.msgShadow > 0) {
        const a = s.msgShadow / 100;
        // 二層の影: 輪郭 (下 1px) + にじみ (広め)。濃さはスライダーに比例。
        style.textShadow =
          `0 1px ${(1 + a * 5).toFixed(1)}px rgba(0,0,0,${(a * 0.95).toFixed(2)}), ` +
          `0 0 ${Math.round(a * 14)}px rgba(0,0,0,${(a * 0.6).toFixed(2)})`;
      }
      return style;
    },
    // 作者が書いた確定文のスタイル。影は本文と共通 (背景画像への可読性の手当ては同じ)、
    // 色だけ別に持つ = 「作者の意図」と「GM の即興」を読み手が見分けられる。
    authoredStyle(s): Record<string, string> {
      const style: Record<string, string> = { ...this.narrationStyle };
      style.color = s.authoredColor || DEFAULT_AUTHORED_COLOR;
      return style;
    },
    // ビート/想起ブロックに敷く黒の透過背景 (0 なら敷かない)。ember/glow の色付き文字が
    // 背景画像に溶ける読みにくさへの手当て。本文 (語り) はそのまま (narrationStyle の影が担当)。
    beatBgStyle: (s): Record<string, string> =>
      s.beatBgOpacity > 0
        ? { backgroundColor: `rgba(0,0,0,${(s.beatBgOpacity / 100).toFixed(2)})` }
        : {},
  },

  actions: {
    // 読み上げ機能そのものの ON/OFF (設定サウンドタブのチェックボックス)。OFF で操作列ごと
    // 消えるので、今喋っているものも止める (設定と体感を一致させる)。
    setTtsFeature(on: boolean): void {
      this.ttsFeature = on;
      tts.saveFeature(on);
      if (!on) tts.stop();
    },
    // 読み上げの ON/OFF。OFF にした瞬間、今喋っているものも止める (設定と体感を一致させる)。
    toggleTts(): void {
      this.ttsEnabled = !this.ttsEnabled;
      tts.saveEnabled(this.ttsEnabled);
      if (!this.ttsEnabled) tts.stop();
    },
    // 今の読み上げを飛ばす。**物語は進めない** — 音を切るだけ (提示層の操作)。
    skipTts(): void {
      tts.stop();
    },

    // --- 画像生成 / 挿絵 (spec 24) -------------------------------------------------------
    // 共有部を部分更新して永続化 (設定タブから)。プロバイダ切替もここ — スロットは触らない
    // (spec 26: 切替は表示スロットの切替だけ、値は失われない)。
    setImageGen(patch: Partial<ImageGenSettings>): void {
      this.imageGen = { ...this.imageGen, ...patch };
      saveImageGenSettings(this.imageGen);
    },
    // 現プロバイダのスロットを部分更新して永続化 (spec 26)。
    setImageGenSlot(patch: Partial<ProviderSlot>): void {
      const p = this.imageGen.provider;
      this.imageGen = {
        ...this.imageGen,
        perProvider: {
          ...this.imageGen.perProvider,
          [p]: { ...currentSlot(this.imageGen), ...patch },
        },
      };
      saveImageGenSettings(this.imageGen);
    },
    // 挿絵を生成する。処理中は押せない (busy)・何度でも押せる (差し替え)。
    // HTTP は backend (キーは WebView に無い・CSP 対象外)。古い完了は requestId で無視。
    // promptOverride を渡すと backend はプロンプト書きを呼ばず、その文字列を verbatim で送る
    // (spec 27 B-3)。**1 回きり**なので store には残さない。
    async generateImage(promptOverride?: string): Promise<void> {
      if (!this.started || this.imageBusy) return;
      const reqId = ++this.imageRequestId;
      this.imageBusy = true;
      try {
        const view = await invoke<{ data_url: string; prompt: string; mime: string }>("generate_image", {
          config: toBackendConfig(this.imageGen),
          direction: this.imageDirection.trim() || null,
          promptOverride: promptOverride?.trim() || null,
        });
        if (reqId !== this.imageRequestId) return; // 場面が変わった (新規/ロード/遷移)
        this.generatedImage = { dataUrl: view.data_url, prompt: view.prompt };
        this.showGeneratedImage = true;
      } catch (e) {
        if (reqId === this.imageRequestId) this.logToast = t("store.imageFailed", { error: String(e) });
      } finally {
        if (reqId === this.imageRequestId) this.imageBusy = false;
      }
    },
    // 直近の挿絵を保存 (backend が原本 bytes を設定フォルダへ書く)。
    async saveGeneratedImage(): Promise<void> {
      if (!this.generatedImage) return;
      try {
        const path = await invoke<string>("save_generated_image", {
          folder: this.imageGen.folder,
          stamp: imageStamp(),
        });
        this.logToast = t("store.imageSaved", { path });
      } catch (e) {
        this.logToast = t("store.saveFailed", { error: String(e) });
      }
    },
    toggleGeneratedImage(): void {
      this.showGeneratedImage = !this.showGeneratedImage;
    },
    toggleText(): void {
      this.showText = !this.showText;
    },
    /**
     * 挿絵まわりの**揮発物**を一括で捨てる (spec 24 の挿絵 + spec 27 の「この一枚への指示」)。
     *
     * **一箇所に寄せてあるのが要点。** 捨てる契機は「新規/再開/ロード」と「章の遷移」の 2 つ
     * あり、以前は同じ列挙が両方に手書きで生えていた — その状態で spec 27 が揮発物を 1 つ
     * 増やしたとき、**両方とも書き忘れて指示が新しいゲームへ持ち越された** (ユーザー報告)。
     * 撤去だけでなく**追加でも「同じ規律が 2 箇所にある」形は落ちる**ので、列挙を 1 本にした。
     */
    dropVolatileImage(): void {
      this.generatedImage = null;
      this.imageDirection = "";
      this.imageRequestId++; // 進行中の生成の結果を捨てる (古い完了は reqId で無視される)
    },

    // --- 参照ストック (spec 27 A) -------------------------------------------------------
    // 4 つとも backend が更新後の一覧を返すので、frontend は state を組み立てない
    // (画面と送信内容の唯一の真実源はセッションフォルダの現物)。
    async loadRefStock(): Promise<void> {
      if (!this.started) {
        this.refStock = null;
        return;
      }
      try {
        this.refStock = await invoke("list_settings_sheets", { provider: this.imageGen.provider });
        this.refStockRev++;
      } catch (e) {
        this.refStock = null;
        this.logToast = String(e);
      }
    },
    async putRefSlot(slot: number): Promise<void> {
      await this.refStockCommand("set_reference_slot", { slot });
    },
    async deleteRefSlot(slot: number): Promise<void> {
      await this.refStockCommand("delete_reference_slot", { slot });
    },
    /**
     * ローカルの画像を枠へ (spec 27 追補、2026-08-24)。WebView で WebP へ変換・縮小してから
     * **raw body** で送る (JSON の number[] は 8MB で数千万要素)。枠番号とプロバイダはヘッダ。
     * 変換は Chromium のネイティブ WebP エンコーダ = Rust 側に image crate も libwebp も足さない。
     */
    async putRefFile(slot: number, file: File): Promise<void> {
      const { fileToWebp } = await import("../imageConvert");
      await this.putRefBytes(slot, await fileToWebp(file).catch((e) => {
        this.logToast = t("refStock.fileFailed", { error: String(e) });
        throw e;
      }));
    },
    /** 変換済みバイト列を枠へ入れる (ローカル取り込みとクロップの共通部)。
     *  枠は拡張子を選ばない — backend が先頭バイトで mime を嗅ぎ分ける。 */
    async putRefBytes(slot: number, bytes: Uint8Array): Promise<void> {
      if (this.refStockBusy) return;
      this.refStockBusy = true;
      try {
        this.refStock = await invoke("put_reference_bytes", bytes, {
          headers: { "x-slot": String(slot), "x-provider": this.imageGen.provider },
        });
        // 前詰めで**名前が同じまま中身が入れ替わる**ので URL キャッシュを破る (failures #86)。
        this.refStockRev++;
      } catch (e) {
        this.logToast = t("refStock.fileFailed", { error: String(e) });
      } finally {
        this.refStockBusy = false;
      }
    },
    /** 同梱アセット (顔アイコン) を参照の枠へ。ID だけ送り bytes は backend が読む。 */
    async putRefFromAsset(slot: number, icon: string): Promise<void> {
      if (this.refStockBusy) return;
      this.refStockBusy = true;
      try {
        this.refStock = await invoke("put_reference_from_asset", {
          provider: this.imageGen.provider,
          slot,
          icon,
        });
        this.refStockRev++;
      } catch (e) {
        this.logToast = t("refStock.fileFailed", { error: String(e) });
      } finally {
        this.refStockBusy = false;
      }
    },
    async reseedRefStock(): Promise<void> {
      await this.refStockCommand("reseed_reference_stock", {});
    },
    async refStockCommand(cmd: string, args: Record<string, unknown>): Promise<void> {
      try {
        this.refStock = await invoke(cmd, { provider: this.imageGen.provider, ...args });
        this.refStockRev++; // 名前が同じまま中身が変わる (前詰め) ので URL を割る
      } catch (e) {
        this.logToast = String(e);
      }
    },

    async openImageFolder(): Promise<void> {
      try {
        await invoke("open_image_folder", { folder: this.imageGen.folder });
      } catch (e) {
        this.logToast = t("store.openFolderFailed", { error: String(e) });
      }
    },

    /**
     * ダイスの seed を振り直す (プレイヤーの meta 操作。save/load と同じ層)。
     * 押した時点では保存しない — 次に保存される時に新しい seed が書かれる。
     * 分岐した事実は会話ログに残す (いつ筋が変わったかを後から辿れるように)。
     */
    async resetSeed() {
      if (!this.started) return;
      if (!(await this.askConfirm(t("state.resetSeedConfirm"), t("state.resetSeed")))) return;
      try {
        await invoke<number>("reset_seed");
        this.log.push({ kind: "system", text: t("state.resetSeedDone") });
        this.logToast = t("state.resetSeedDone");
      } catch (e) {
        this.logToast = String(e);
      }
    },

    // 自前の確認ダイアログを開き、ユーザーの選択 (OK=true / キャンセル=false) を Promise で返す。
    // WebView2 の window.confirm は本文に tauri://localhost を混ぜてしまうので、これで置き換える。
    // 二重呼び出し (前の確認が未解決) は前をキャンセル扱いで畳んでから開く。
    /**
     * 改名 (Kataribe → Lorekeel) の一度きりの告知。**旧インストールの痕跡が在るときだけ**出す
     * — 通知済みフラグの有無だけで判定すると、新規ユーザーにも「改名しました」が出て、
     * 知らない旧名を知らせることになる。フラグは localStorage (プレフィックスは据え置き:
     * ここを替えると packagePaths などユーザーの設定が全部初期値に戻る)。
     */
    async announceRename(): Promise<void> {
      if (localStorage.getItem("kataribe.renameNoticed")) return;
      let notice: { old_dir?: string | null; migrated?: boolean; error?: string | null };
      try {
        notice = await invoke("rename_notice");
      } catch {
        return; // Tauri 外では何もしない
      }
      if (!notice.old_dir) return; // 新規インストール
      const body = notice.error
        ? t("rename.failed", { dir: notice.old_dir, error: notice.error })
        : notice.migrated
          ? t("rename.migrated", { dir: notice.old_dir })
          : t("rename.alreadyThere", { dir: notice.old_dir });
      localStorage.setItem("kataribe.renameNoticed", "1");
      await this.askConfirm(`${t("rename.body")}

${body}`, t("rename.ok"), true);
    },

    askConfirm(message: string, confirmLabel?: string, noCancel = false): Promise<boolean> {
      if (confirmResolver) {
        confirmResolver(false);
        confirmResolver = null;
      }
      this.confirmDialog = { message, confirmLabel: confirmLabel ?? t("confirm.ok"), noCancel };
      return new Promise<boolean>((resolve) => {
        confirmResolver = resolve;
      });
    },
    // ConfirmDialog のボタンから呼ぶ。ダイアログを閉じて Promise を解決する。
    resolveConfirm(ok: boolean) {
      this.confirmDialog = null;
      confirmResolver?.(ok);
      confirmResolver = null;
    },

    // ------------------------------------------------------------------
    // 編集モード (spec 28 Phase A)
    // ------------------------------------------------------------------

    /** 鉛筆トグル。ON = backend に編集ルートを登録して一覧を受ける / OFF = exitEditor。 */
    async toggleEditor() {
      if (this.editor.on) {
        await this.exitEditor();
        return;
      }
      // 一次ガード (卓中・未選択はボタン側 disable と二層)。
      if (!this.packagePath || this.multi.role !== "solo") return;
      try {
        const view = await invoke<{
          files: { rel_path: string; category: string }[];
          media: { rel_path: string; category: string }[];
          from_site: boolean;
          root: string;
        }>("open_editor", { path: this.packagePath });
        this.editor = {
          ...freshEditorState(),
          on: true,
          root: this.packagePath,
          absRoot: view.root,
          files: view.files.map((f) => ({ relPath: f.rel_path, category: f.category })),
          media: view.media.map((f) => ({ relPath: f.rel_path, category: f.category })),
          fromSite: view.from_site,
        };
        // 入場時にも層 2 を一度走らせる (開幕 ⚠ の内容を直しに来る動線 — 保存する前に
        // 何が悪いかが見えていないと、直す対象を別画面で覚えてくる羽目になる)。
        void this.refreshEditorIssues();
        void this.refreshEditorVocab();
      } catch (e) {
        this.logToast = String(e);
      }
    },

    /** 層 2 診断の更新 (spec 28 Phase B)。失敗は沈黙 (層 1 が主・こちらは補助)。 */
    async refreshEditorIssues() {
      if (!this.editor.on) return;
      try {
        this.editor.issues = await invoke<EditorState["issues"]>("inspect_editor_package");
      } catch {
        /* 沈黙 */
      }
    },

    /** 新規ファイル作成 (spec 28 Phase D、4 カテゴリ)。作成 → 一覧差し替え → そのまま開く。 */
    async createEditorFile(category: string, stem: string) {
      const ed = this.editor;
      if (!ed.on) return;
      // 未保存の確認は作成の**前** (作ってから切替をキャンセルされると、
      // 開かれないファイルだけが残って紛らわしい)。
      if (this.editorDirty) {
        if (!(await this.askConfirm(t("editor.discardConfirm"), t("editor.discardOk")))) return;
      }
      const fork = ed.fromSite;
      if (fork && !(await this.askConfirm(t("editor.forkConfirm"), t("editor.forkOk")))) return;
      try {
        const res = await invoke<{
          rel_path: string;
          files: { rel_path: string; category: string }[];
          forked: boolean;
          fork_warning: string | null;
        }>("create_editor_file", { category, stem, fork });
        ed.files = res.files.map((f) => ({ relPath: f.rel_path, category: f.category }));
        if (res.forked) ed.fromSite = false;
        if (res.fork_warning) this.logToast = res.fork_warning;
        // 作った雛形をそのまま開く (dirty は上で確認済みなので素通り)。
        // 改行の正規化は openEditorFile と同じ (雛形は LF だが、前のファイルの eol を
        // 引きずると保存で改行コードが化ける)。
        const raw = await invoke<string>("read_editor_file", { relPath: res.rel_path });
        ed.eol = raw.includes("\r\n") ? "\r\n" : "\n";
        ed.current = res.rel_path;
        ed.text = raw.replace(/\r\n/g, "\n");
        ed.savedText = ed.text;
        // 新ファイルは診断・語彙の対象 (cast に足せば entities にも載る)。
        void this.refreshEditorIssues();
        void this.refreshEditorVocab();
      } catch (e) {
        this.logToast = String(e);
      }
    },

    /**
     * ファイル名の変更 (2026-08-28)。同じフォルダの中だけ。
     * **参照は追随しない** — シナリオを改名すれば entry/modules が、キャラなら cast/present が
     * 指す先を失う。壊れることは層 2 の inspect が報告する (削除と同じ判断) ので、
     * ここでは確認を挟まず即実行する (VS Code の流儀。取り消しは名前を戻せばよい)。
     */
    async renameEditorFile(relPath: string, newName: string) {
      const ed = this.editor;
      if (!ed.on || !newName.trim()) return;
      const fork = ed.fromSite;
      if (fork && !(await this.askConfirm(t("editor.forkConfirm"), t("editor.forkOk")))) return;
      try {
        const res = await invoke<{
          rel_path: string;
          files: { rel_path: string; category: string }[];
          media: { rel_path: string; category: string }[];
          forked: boolean;
          fork_warning: string | null;
        }>("rename_editor_file", { relPath, newName: newName.trim(), fork });
        ed.files = res.files.map((f) => ({ relPath: f.rel_path, category: f.category }));
        ed.media = res.media.map((f) => ({ relPath: f.rel_path, category: f.category }));
        if (res.forked) ed.fromSite = false;
        if (res.fork_warning) this.logToast = res.fork_warning;
        // 開いていたファイルの名前が変わったら追随する (中身は同じ = dirty は保つ)。
        if (ed.current === relPath) ed.current = res.rel_path;
        void this.refreshEditorIssues();
        void this.refreshEditorVocab();
      } catch (e) {
        this.logToast = String(e);
      }
    },

    /** ファイル削除 (2026-08-27 に v1 昇格)。不可逆の確認 → (書庫由来なら) フォーク確認。 */
    async deleteEditorFile(relPath: string) {
      const ed = this.editor;
      if (!ed.on) return;
      if (!(await this.askConfirm(t("editor.deleteConfirm", { file: relPath }), t("editor.deleteOk")))) return;
      const fork = ed.fromSite;
      if (fork && !(await this.askConfirm(t("editor.forkConfirm"), t("editor.forkOk")))) return;
      try {
        const res = await invoke<{
          files: { rel_path: string; category: string }[];
          media: { rel_path: string; category: string }[];
          forked: boolean;
          fork_warning: string | null;
        }>("delete_editor_file", { relPath, fork });
        // 削除は YAML にもメディアにも効くので、返りの両方を写す (2026-09-04 ユーザー報告:
        // files だけ写していたのでメディアを消しても一覧が変わらず「無反応」に見えた)。
        ed.files = res.files.map((f) => ({ relPath: f.rel_path, category: f.category }));
        ed.media = res.media.map((f) => ({ relPath: f.rel_path, category: f.category }));
        if (res.forked) ed.fromSite = false;
        if (res.fork_warning) this.logToast = res.fork_warning;
        else this.logToast = t("editor.deleted", { file: relPath });
        // 開いていたファイルを消したらエディタを空へ (dirty ごと消えるのは削除確認が覆う)。
        if (ed.current === relPath) {
          ed.current = "";
          ed.text = "";
          ed.savedText = "";
        }
        void this.refreshEditorIssues();
        void this.refreshEditorVocab();
      } catch (e) {
        this.logToast = String(e);
      }
    },

    /**
     * メディアの投入 (spec 28 追補、2026-08-28)。**画像は WebView で WebP へ変換**してから
     * 送る (spec 27 のローカル取り込みと同じ規律 — Rust に image crate も libwebp も足さない。
     * 長辺 1536・q0.88 で 4000px の写真が配布可能なサイズに落ちる)。音声は**変換しない**
     * (ブラウザに transcode の口が無い) ので原本のまま — 作法は Ogg 推奨。
     * 行き先は backend が中身で振り分けるので、ここでは種別を申告しない。
     */
    async putEditorMedia(files: File[]) {
      if (!this.editor.on || !files.length) return;
      let added = 0;
      let last = "";
      for (const file of files) {
        try {
          const isImage = file.type.startsWith("image/") || /\.(png|jpe?g|webp|gif|bmp)$/i.test(file.name);
          // SVG はラスタ化すると意味が変わる (拡大に強いのが取り柄) のでそのまま通す。
          const isSvg = file.type === "image/svg+xml" || /\.svg$/i.test(file.name);
          const bytes =
            isImage && !isSvg
              ? await fileToWebp(file)
              : new Uint8Array(await file.arrayBuffer());
          const res = await invoke<{ rel_path: string; media: { rel_path: string; category: string }[] }>(
            "put_editor_media",
            bytes,
            { headers: { "x-name": encodeURIComponent(file.name) } },
          );
          this.editor.media = res.media.map((f) => ({ relPath: f.rel_path, category: f.category }));
          last = res.rel_path;
          added++;
        } catch (e) {
          // 1 枚ずつ報告する (まとめて握り潰すと、どれが弾かれたか分からない)。
          this.logToast = `${file.name}: ${String(e)}`;
        }
      }
      if (added === 1) this.logToast = t("editor.mediaAdded", { file: last });
      else if (added > 1) this.logToast = t("editor.mediaAddedMany", { n: String(added) });
      if (added) void this.refreshEditorVocab(); // 補完に新しいアセット名を載せる
    },

    /** クロップの書き戻し (spec 28 追補)。**同じ ID を上書き** — 参照している YAML を
     *  書き直さずに済ませるため。不可逆なので確認を挟む。 */
    async replaceEditorMedia(relPath: string, bytes: Uint8Array) {
      if (!this.editor.on) return false;
      try {
        const res = await invoke<{ media: { rel_path: string; category: string }[] }>(
          "replace_editor_media",
          bytes,
          { headers: { "x-rel": relPath } },
        );
        this.editor.media = res.media.map((f) => ({ relPath: f.rel_path, category: f.category }));
        // 同じ名前で中身が替わった = URL キャッシュが古い絵を出す (failures #86 と同型)。
        this.editor.mediaRev++;
        this.logToast = t("editor.mediaCropped", { file: relPath });
        return true;
      } catch (e) {
        this.logToast = String(e);
        return false;
      }
    },

    /** エディタの文字サイズ段を変える (即座に永続 — 表示設定と同じ流儀)。 */
    setEditorFontStep(step: number) {
      this.editorFontStep = Math.min(2, Math.max(0, Math.round(step)));
      localStorage.setItem(EDITOR_FONT_KEY, String(this.editorFontStep));
    },

    /** 補完語彙の更新 (spec 28 Phase C)。失敗は沈黙 (補完が出ないだけ — 編集は止めない)。 */
    async refreshEditorVocab() {
      if (!this.editor.on) return;
      try {
        this.editor.vocab = await invoke<EditorVocabulary>("editor_vocabulary");
      } catch {
        /* 沈黙 */
      }
    },

    /** 編集モードを出る。未保存があれば確認 (force は確認済みの経路 = パッケージ切替/終了)。
     *  戻り値 false = ユーザーがキャンセルして留まった。 */
    async exitEditor(force = false): Promise<boolean> {
      if (!this.editor.on) return true;
      if (!force && this.editorDirty) {
        if (!(await this.askConfirm(t("editor.discardConfirm"), t("editor.discardOk")))) return false;
      }
      try {
        await invoke("close_editor");
      } catch {
        // 登録解除の失敗は無視してよい (次の open_editor が上書く)。
      }
      this.editor = freshEditorState();
      return true;
    },

    /** ファイルを開く。未保存があれば確認 (OK = 破棄して切替。保存したければ先に Ctrl+S)。 */
    async openEditorFile(relPath: string) {
      if (!this.editor.on || relPath === this.editor.current) return;
      if (this.editorDirty) {
        if (!(await this.askConfirm(t("editor.discardConfirm"), t("editor.discardOk")))) return;
      }
      try {
        const raw = await invoke<string>("read_editor_file", { relPath });
        // 元の改行を覚えて LF へ正規化 (CodeMirror の内部表現に合わせる — EditorState 参照)。
        this.editor.eol = raw.includes("\r\n") ? "\r\n" : "\n";
        const text = raw.replace(/\r\n/g, "\n");
        this.editor.current = relPath;
        this.editor.text = text;
        this.editor.savedText = text;
      } catch (e) {
        this.logToast = String(e);
      }
    },

    /** 保存 (Ctrl+S / 保存ボタン)。書庫由来なら初回にフォーク確認 (spec 28 B)。 */
    async saveEditorFile() {
      const ed = this.editor;
      if (!ed.on || !ed.current || ed.saving) return;
      const fork = ed.fromSite;
      if (fork && !(await this.askConfirm(t("editor.forkConfirm"), t("editor.forkOk")))) return;
      ed.saving = true;
      try {
        // 元の改行へ戻して書く (エディタ内部は LF — ディスクの改行コードを黙って変えない)。
        const out = ed.eol === "\r\n" ? ed.text.replace(/\n/g, "\r\n") : ed.text;
        const res = await invoke<{ forked: boolean; fork_warning: string | null }>("save_package_file", {
          relPath: ed.current,
          text: out,
          fork,
        });
        ed.savedText = ed.text;
        if (res.forked) ed.fromSite = false; // メタが消えた = 以後は手動配置と同じ
        if (res.fork_warning) {
          this.logToast = res.fork_warning; // 保存は成功・メタ削除だけ失敗 (次の保存で再試行)
        } else if (this.started && this.activePackagePath === ed.root) {
          // プレイ中のパッケージを編集した — 黙って効かないのは不信の元 (spec 28)。
          this.logToast = t("editor.savedWhilePlaying");
        } else {
          this.logToast = t("editor.saved", { file: ed.current });
        }
        // 保存のたびに層 2 と語彙を更新 (ファイル横断の破れはここでしか出ない /
        // 保存で宣言した id が次の打鍵から補完に出る)。
        void this.refreshEditorIssues();
        void this.refreshEditorVocab();
      } catch (e) {
        this.logToast = String(e);
      } finally {
        ed.saving = false;
      }
    },

    // 開発者モードの現在値を backend (プロセス env) から取り直す (起動時)。
    async refreshDevMode() {
      try {
        this.devMode = await invoke<boolean>("get_dev_mode");
      } catch {
        /* Tauri 外では既定 false のまま */
      }
    },
    // 開発者モードを切り替える (env 即時反映 + app_data/.env 永続化)。次の play_turn から効く。
    async setDevMode(enabled: boolean) {
      await invoke("set_dev_mode", { enabled });
      this.devMode = enabled;
    },
    // 使用中の AI モデル名を backend から取り直す (起動時 + AIモデル設定の保存後)。
    // TitleBar のバッジと OS ウィンドウタイトル (タスクバー/Alt+Tab) の両方に反映する。
    async refreshLlmModel() {
      try {
        const cfg = await invoke<{ model: string }>("get_llm_config");
        this.llmModel = cfg.model ?? "";
      } catch {
        return; // Tauri 外 (ブラウザ) や backend 未接続では静かに諦める
      }
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        await getCurrentWindow().setTitle(
          this.llmModel
            ? t("store.windowTitleModel", { model: this.llmModel })
            : t("store.windowTitle"),
        );
      } catch {
        /* ウィンドウ API が無い環境ではバッジ表示のみ */
      }
    },

    // 配布サイトに新しいアプリがあるか確認する (起動時)。現在版 = ビルド時に埋めた git タグ
    // (__APP_VERSION__)。判定は backend (fetch_app_update) の純関数に委ね、結果だけ受け取る。
    // 自動更新はしない — 通知だけ (クリックでサイトを既定ブラウザで開く)。
    async checkAppUpdate() {
      try {
        const status = await invoke<{ update_available: boolean; latest_version: string }>(
          "fetch_app_update",
          { siteUrl: this.siteUrl, currentVersion: __APP_VERSION__ || "" },
        );
        this.updateAvailable = status.update_available;
        this.latestVersion = status.latest_version;
      } catch {
        // オフライン / 配布サイト未設定 / Tauri 外は静かに諦める (更新通知は非必須)。
        this.updateAvailable = false;
      }
    },

    // 「最新版があります」クリック: 配布サイトを既定ブラウザで開く (アプリ更新は手動)。
    async openUpdateSite() {
      try {
        await invoke("open_external_url", { url: this.siteUrl });
      } catch (e) {
        this.logToast = t("store.openSiteFailed", { error: String(e) });
      }
    },

    // 書庫のパッケージ詳細ページを既定ブラウザで開く (説明の全文・レビューはサイト側で読む)。
    // 開くのは常にユーザー登録の siteUrl 起点 — id はパス成分として encode し origin を変えられない。
    async openSitePackagePage(id: string) {
      try {
        await invoke("open_external_url", {
          url: `${this.siteUrl}/packages/${encodeURIComponent(id)}`,
        });
      } catch (e) {
        this.logToast = t("store.openSiteFailed", { error: String(e) });
      }
    },

    // 背景の明るさを設定 (即時反映 + localStorage 永続化)。グラフィック設定タブから呼ぶ。
    setBgBrightness(v: number) {
      this.bgBrightness = Math.max(0, Math.min(100, Math.round(v)));
      localStorage.setItem(BG_BRIGHTNESS_KEY, String(this.bgBrightness));
    },

    // 会話ペインの配色を設定 (即時反映 + localStorage 永続化)。
    setPaneTheme(v: PaneTheme) {
      this.paneTheme = v;
      localStorage.setItem(PANE_THEME_KEY, v);
    },

    // 右ペインの幅を設定 (ドラッグ中に即時反映 + localStorage 永続化)。範囲でクランプ。
    setPanelWidth(px: number) {
      this.panelWidth = Math.max(PANEL_WIDTH_MIN, Math.min(PANEL_WIDTH_MAX, Math.round(px)));
      localStorage.setItem(PANEL_WIDTH_KEY, String(this.panelWidth));
    },

    // 本文フォントを設定 (即時反映 + localStorage 永続化)。表示設定タブから呼ぶ。
    setMsgFont(id: string) {
      this.msgFont = MESSAGE_FONTS.some((f) => f.id === id) ? id : "default";
      localStorage.setItem(MSG_FONT_KEY, this.msgFont);
    },
    // 本文の文字サイズを設定 (0 = UI に合わせる = キーごと消す)。表示設定タブから呼ぶ。
    setMsgSize(px: number) {
      this.msgSize = Number.isFinite(px) && px >= 12 && px <= 32 ? px : 0;
      if (this.msgSize > 0) localStorage.setItem(MSG_SIZE_KEY, String(this.msgSize));
      else localStorage.removeItem(MSG_SIZE_KEY);
    },
    // 本文の文字色を設定 (空 = テーマ既定へ戻す)。
    setMsgColor(hex: string) {
      this.msgColor = hex;
      if (hex) localStorage.setItem(MSG_COLOR_KEY, hex);
      else localStorage.removeItem(MSG_COLOR_KEY);
    },
    // 作者文の色を設定 (空 = 既定へ戻す)。
    setAuthoredColor(hex: string) {
      this.authoredColor = hex;
      if (hex) localStorage.setItem(AUTHORED_COLOR_KEY, hex);
      else localStorage.removeItem(AUTHORED_COLOR_KEY);
    },
    // 本文の影の濃さを設定 (0 = なし)。
    setMsgShadow(v: number) {
      this.msgShadow = Math.max(0, Math.min(100, Math.round(v)));
      localStorage.setItem(MSG_SHADOW_KEY, String(this.msgShadow));
    },
    // ビート/想起の黒背景の濃さを設定 (0 = なし)。表示設定タブから呼ぶ。
    setBeatBgOpacity(v: number) {
      this.beatBgOpacity = Math.max(0, Math.min(100, Math.round(v)));
      localStorage.setItem(BEAT_BG_KEY, String(this.beatBgOpacity));
    },

    // 音量を設定 (即時反映 + localStorage 永続化)。サウンド設定タブから呼ぶ。
    setAudioVolume(v: number) {
      this.audioVolume = Math.max(0, Math.min(100, Math.round(v)));
      localStorage.setItem(AUDIO_VOLUME_KEY, String(this.audioVolume));
    },
    // ミュート切替 (即時反映 + localStorage 永続化)。
    setAudioMuted(b: boolean) {
      this.audioMuted = b;
      localStorage.setItem(AUDIO_MUTED_KEY, String(b));
    },
    // SE を one-shot 再生する (発火ビート由来)。ミュート/音量 0 なら鳴らさない。
    // BGM はループ要素 (App.vue の <audio>) が担うので、ここは効果音だけ。
    playSe(url: string | null) {
      const gain = this.audioGain;
      if (!url || gain <= 0) return;
      try {
        const a = new Audio(url);
        a.volume = gain;
        void a.play().catch(() => {
          /* 自動再生制約・デコード失敗は握りつぶす (没入の付帯機能ゆえ致命でない) */
        });
      } catch {
        /* Audio 生成失敗も無視 */
      }
    },

    // localStorage のパス一覧から各 package.yaml の manifest を読み、一覧 view を更新する。
    async refreshPackages() {
      try {
        this.packages = await invoke<PackageEntry[]>("list_packages", {
          paths: this.packagePaths,
        });
        // 選択中パスが一覧から消えていたら表示の一番上 (新しい順の先頭 = 末尾要素) へ寄せる。
        if (!this.packagePaths.includes(this.packagePath) && this.packagePaths.length) {
          this.packagePath = this.packagePaths[this.packagePaths.length - 1];
        }
      } catch (e) {
        this.error = String(e);
      }
    },

    // パッケージフォルダのパスを一覧に追加する (localStorage に永続化)。
    // 追加できたら「親フォルダ」を覚え、次回の参照ダイアログの初期位置にする。
    addPackage(path: string) {
      const p = path.trim();
      if (!p || this.packagePaths.includes(p)) return;
      this.packagePaths.push(p);
      savePaths(this.packagePaths);
      const parent = parentDir(p);
      if (parent) {
        this.lastPackageParent = parent;
        localStorage.setItem(LAST_PKG_PARENT_KEY, parent);
      }
      this.refreshPackages();
    },

    // OS ネイティブのフォルダ選択ダイアログでパッケージフォルダを選び、そのまま一覧へ追加する
    // (パッケージ一覧の「参照」ボタン)。初期ディレクトリは前回追加の親フォルダ。
    // 選択がキャンセルされたら何もしない。無効な (package.yaml の無い) フォルダを選んでも
    // 追加はされ、一覧に「読込失敗」で並ぶ (手入力パスと同じ扱い)。
    async browseAndAddPackage() {
      const picked = await this.pickFolder();
      if (picked) this.addPackage(picked);
    },
    /** フォルダ選択ダイアログ (前回追加した場所から開く)。キャンセルは null、失敗はエラー表示。 */
    async pickFolder(start?: string): Promise<string | null> {
      try {
        return await invoke<string | null>("pick_package_folder", { start: start ?? this.lastPackageParent });
      } catch (e) {
        this.error = t("store.folderPickFailed", { error: String(e) });
        return null;
      }
    },
    /**
     * 新しいパッケージの骨格を作る (2026-09-04 ユーザー要望)。置き場と名前だけで
     * `{parent}/{name}/` に package.yaml と最小の entry を書き、一覧へ登録して選択する
     * (従来は手でフォルダと package.yaml を作ってローカル読み込みしないと編集に入れなかった
     * = その手順をここへ畳む)。編集モードへの入場は呼び出し側 (ダイアログを閉じてから)。
     * 成功なら true。
     */
    async createLocalPackage(parent: string, name: string): Promise<boolean> {
      try {
        const path = await invoke<string>("create_local_package", { parent, name });
        this.addPackage(path);
        this.packagePath = path;
        this.logToast = t("store.packageCreated", { path });
        return true;
      } catch (e) {
        this.logToast = t("store.packageCreateFailed", { error: String(e) });
        return false;
      }
    },

    // パスを一覧から外す。
    async removePackage(path: string) {
      // セーブ (autosave + 手動スロット) は app_data/saves のファイルなので、一覧からパスを
      // 消すだけでは孤児として残り続ける。セーブがあるパッケージなら削除するか確認する
      // (キャンセル = セーブは残す = パスを再追加すれば「続きから」もスロットも復活する)。
      const entry = this.packages.find((p) => p.path === path);
      if (entry?.autosave_turn != null || entry?.has_slots) {
        const title = entry.title || path;
        const msg =
          entry.autosave_turn != null
            ? t("store.deleteSaveConfirm", { title, turn: entry.autosave_turn })
            : t("store.deleteSlotsConfirm", { title });
        if (await this.askConfirm(msg, t("store.deleteConfirmOk"))) {
          try {
            await invoke("delete_autosave", { packagePath: path });
          } catch (e) {
            this.logToast = t("store.deleteSaveFailed", { error: String(e) });
          }
        }
      }
      this.packagePaths = this.packagePaths.filter((p) => p !== path);
      savePaths(this.packagePaths);
      if (this.packagePath === path) {
        // 表示の一番上 (新しい順の先頭 = 末尾要素) へ寄せる。
        this.packagePath = this.packagePaths[this.packagePaths.length - 1] ?? "";
      }
      this.refreshPackages();
    },

    // 書庫サイトの URL を設定する (localStorage 永続。空なら既定 = 公式へ戻す)。
    setSiteUrl(url: string) {
      const u = url.trim();
      this.siteUrl = u || DEFAULT_SITE_URL;
      localStorage.setItem(SITE_URL_KEY, this.siteUrl);
      // URL が変わったら前のサイトの一覧は無効。
      this.remote = null;
      this.remoteError = null;
    },

    // 書庫の一覧を取得する (無認証の公開 API。backend が HTTP を担い CORS を回避)。
    async fetchSitePackages(opts?: { page?: number; q?: string; category?: string; sort?: string }) {
      this.remoteLoading = true;
      this.remoteError = null;
      try {
        this.remote = await invoke<RemoteList>("fetch_site_packages", {
          siteUrl: this.siteUrl,
          page: opts?.page ?? 1,
          q: opts?.q ?? null,
          category: opts?.category ?? null,
          sort: opts?.sort ?? null,
        });
      } catch (e) {
        this.remote = null;
        this.remoteError = String(e);
      } finally {
        this.remoteLoading = false;
      }
    },

    // 書庫からパッケージを取得する: zip DL → クライアント側検証 (zip slip 遮断) → 展開 →
    // packagePaths へ登録。展開先は backend が app data dir に据える。成功なら登録先パスを返す。
    async installSitePackage(id: string): Promise<InstalledPackage | null> {
      if (this.installingId) return null; // 直列化 (多重 DL しない)
      this.installingId = id;
      this.remoteError = null;
      try {
        // spec 17 A-1: サーバ申告の sha256 を expected として渡す (DL 破損の一致検証 +
        // 出所メタの基準)。一覧に無ければ null (古い書庫 = 検証なしで従来どおり)。
        const expected = this.remote?.items.find((p) => p.id === id)?.sha256 ?? null;
        const installed = await invoke<InstalledPackage>("install_site_package", {
          siteUrl: this.siteUrl,
          id,
          sha256: expected,
        });
        this.addPackage(installed.path);
        return installed;
      } catch (e) {
        this.remoteError = String(e);
        return null;
      } finally {
        this.installingId = null;
      }
    },

    // --- パッケージ更新 (spec 17 Phase C) ---

    // 登録済みパッケージの更新有無を書庫へ照会する (ローカルタブを開くたび自動)。
    // 失敗は沈黙 (rev2 B-8): 例外でも既存のバッジ状態を消さない — 検知は best-effort で、
    // オフラインや一時的な 5xx が「更新なし」に見えてしまう方が有害。
    async checkPackageUpdates() {
      try {
        this.packageUpdates = await invoke<PackageUpdate[]>("check_package_updates", {
          siteUrl: this.siteUrl,
          paths: this.packagePaths,
        });
      } catch {
        /* 沈黙 (前回の判定を保つ) */
      }
    },

    // このパスに更新が来ているか (一覧行のバッジ判定)。
    updateFor(path: string): PackageUpdate | undefined {
      return this.packageUpdates.find((u) => u.path === path);
    },

    // この書庫 id を現在のサイトから取得済みか (サイトタブの「取得済み」判定)。
    // 出所メタの site_url まで見るので、別サイトの同 id とは混ざらない。同じ配布物を
    // `_2` で二重に据えるのを止めるのが眼目 (更新はローカルタブの役割)。
    installedFromSite(id: string): PackageEntry | undefined {
      const site = canonicalSite(this.siteUrl);
      return this.packages.find(
        (p) => p.source_id === id && canonicalSite(p.source_site ?? "") === site,
      );
    },

    // 書庫の最新版で上書き更新する。ローカル編集が在れば先に確認する (失われる変更の告知)。
    // 成功したらメタ・一覧・バッジを取り直し、版の遷移をトーストで報せる。
    async updatePackage(path: string) {
      if (this.updatingPath) return; // 直列化 (backend の排他と対)
      if (path === this.activePackagePath) {
        this.logToast = t("store.updateWhilePlaying");
        return;
      }
      try {
        const edited = await invoke<boolean>("package_is_locally_edited", { path });
        if (edited && !(await this.askConfirm(t("store.updateEditedConfirm"), t("store.updateConfirmOk")))) {
          return;
        }
        this.updatingPath = path;
        const r = await invoke<UpdateResult>("update_site_package", {
          siteUrl: this.siteUrl,
          path,
          force: edited,
        });
        const unknown = t("store.versionUnknown");
        this.logToast = t("store.packageUpdated", {
          title: r.title,
          from: r.from_version ?? unknown,
          to: r.to_version ?? unknown,
        });
        await this.refreshPackages();
        await this.checkPackageUpdates();
      } catch (e) {
        this.logToast = t("store.updateFailed", { error: String(e) });
      } finally {
        this.updatingPath = null;
      }
    },

    // --- 会話ログのテキスト保存 (ユーザーFB 2026-07-09) ---

    // ログ保存先フォルダを設定する (空 = 既定 app_data_dir/logs へ戻す)。
    setLogDir(path: string) {
      this.logDir = path.trim();
      if (this.logDir) localStorage.setItem(LOG_DIR_KEY, this.logDir);
      else localStorage.removeItem(LOG_DIR_KEY);
    },

    // 会話ログをプレーンテキストへ整形する (ConversationLog の見た目に沿う)。
    formatLog(): string {
      const lines: string[] = [];
      for (const e of this.log) {
        switch (e.kind) {
          case "opening":
            lines.push(e.text);
            break;
          case "player":
            lines.push(`> ${t("log.you")}: ${e.text}`);
            break;
          case "narration":
            lines.push(e.text);
            break;
          case "authored":
            lines.push(e.text);
            break;
          case "beat":
            if (e.narration.trim()) lines.push(`✦ ${e.narration}`);
            for (const r of e.recalled) lines.push(`  ┊ ${r}`);
            break;
          case "rolls":
            for (const r of e.rolls)
              lines.push(
                `🎲 1d${r.sides} = ${r.result} (DC ${r.dc}) → ${r.success ? t("log.success") : t("log.fail")}`,
              );
            break;
          case "checks":
            for (const c of e.checks) {
              // percentile (degree あり) はロールアンダー書式 (spec 16)。
              if (c.degree) {
                lines.push(
                  `🎯 ${t("log.checkLabel", { entity: c.entity, stat: c.stat })}: d100=${c.roll} ${c.success ? "≤" : ">"} ${c.dc} → ${degreeLabel(c.degree)}`,
                );
                if (c.narration) lines.push(c.narration);
                continue;
              }
              const mod = c.modifier >= 0 ? `+${c.modifier}` : `${c.modifier}`;
              lines.push(
                `🎯 ${t("log.checkLabel", { entity: c.entity, stat: c.stat })}: ${c.count > 1 ? c.count : 1}d${c.sides}(${c.roll})${c.times > 1 ? '×' + c.times : ''}${mod} = ${c.total} (DC ${c.dc}) → ${c.success ? t("log.success") : t("log.fail")}`,
              );
              if (c.narration) lines.push(c.narration);
            }
            break;
          case "statrolls":
            for (const sr of e.stat_rolls) lines.push(`🎲 ${statRollLine(sr)}`);
            break;
          case "reject":
            lines.push(t("log.rejectHeader", { attempts: e.attempts }));
            for (const r of e.reasons) lines.push(`  - ${r}`);
            break;
          case "selfrepair":
            // ログ保存は畳まず全文 (診断情報を残す)。
            lines.push(t("log.selfrepairDone", { attempts: e.attempts }));
            if (e.reasons.length) {
              lines.push(t("log.rejectedAttempts"));
              e.reasons.forEach((rs, i) =>
                lines.push(`  ${t("log.selfrepairAttempt", { n: i + 1, reasons: rs.join(" / ") })}`),
              );
            }
            break;
          case "system":
            lines.push(`── ${e.text} ──`);
            break;
        }
        lines.push(""); // エントリ間に空行
      }
      return lines.join("\n");
    },

    // 現在のログを「日時_パッケージ名.txt」で保存する。backend がフォルダを解決・書き込む。
    async saveLog(): Promise<void> {
      if (!this.started || !this.log.length) {
        this.logToast = t("store.noLogToSave");
        return;
      }
      const now = new Date();
      const p = (n: number) => String(n).padStart(2, "0");
      const stamp =
        `${now.getFullYear()}${p(now.getMonth() + 1)}${p(now.getDate())}` +
        `_${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}`;
      // パッケージ名をファイル名に使える形へ (パス特殊文字・空白を除去、長すぎ切り詰め)。
      const safeTitle =
        (this.title || "kataribe")
          .replace(/[\\/:*?"<>|]/g, "")
          .replace(/\s+/g, "_")
          .slice(0, 40) || "kataribe";
      const fileName = `${stamp}_${safeTitle}.txt`;
      const header = `# ${this.title || t("store.brandFallback")}\n# ${t("store.logHeaderDate")}: ${now.toLocaleString()}\n\n`;
      try {
        const path = await invoke<string>("save_log_file", {
          folder: this.logDir,
          fileName,
          content: header + this.formatLog(),
        });
        this.logToast = t("store.logSaved", { path });
      } catch (e) {
        this.logToast = t("store.saveFailed", { error: String(e) });
      }
    },

    // パッケージのフォルダを OS のファイルマネージャで開く (一覧の「フォルダ」ボタン)。
    // 中身を直接いじる (改変する・自作の下敷きにする) ための導線。
    async openPackageFolder(path: string) {
      try {
        await invoke("open_package_folder", { path });
      } catch (e) {
        this.logToast = t("store.openFolderFailed", { error: String(e) });
      }
    },

    // ログフォルダを OS のファイルマネージャで開く (設定ダイアログのボタン)。
    async openLogFolder() {
      try {
        await invoke("open_log_folder", { folder: this.logDir });
      } catch (e) {
        this.logToast = t("store.openFolderFailed", { error: String(e) });
      }
    },

    // --- 既成事実 (spec 20) のユーザー専権編集。成功後は backend が即時 autosave 済み ---
    async factsAdd(text: string) {
      if (!text.trim()) return;
      try {
        const res = await transport.request<FactsOpView>("facts_add", { text });
        this.facts = res.facts;
        // 満杯で押し出された行はトーストで可視化する (silent な退場を作らない)。
        if (res.evicted) this.logToast = t("state.factsEvicted", { text: res.evicted });
      } catch (e) {
        this.logToast = String(e);
      }
    },
    async factsEdit(id: number, text: string) {
      if (!text.trim()) return;
      try {
        const res = await transport.request<FactsOpView>("facts_edit", { id, text });
        this.facts = res.facts;
      } catch (e) {
        this.logToast = String(e);
      }
    },
    async factsDelete(id: number) {
      try {
        const res = await transport.request<FactsOpView>("facts_delete", { id });
        this.facts = res.facts;
      } catch (e) {
        this.logToast = String(e);
      }
    },

    // new_game / resume_game 共通の view 反映。resume なら再開マーカーと前回までの語りをログに出す。
    async applyGameView(view: GameView, path: string) {
      // アセット ID はパッケージ内でのみ一意 — 盤面が替わるここでキャッシュを捨て、
      // この view の分を先に解決しておく (以後の assetUrl は同期で引ける)。
      assetUrlCache.clear();
      await prefetchAssets(collectViewAssets(view));
      this.started = true;
      this.packagePath = path;
      // このゲームが「プレイ中の真実」。以後コンボリストを別へ切り替えても動かない
      // (セーブはこのパスに対して有効。packagePath がこれと食い違えばセーブは無効化)。
      this.activePackagePath = path;
      // 次回起動時のコンボリスト初期選択のために「前回プレイ」を覚える
      // (new_game / resume / スロットロードの全経路がここを通る)。
      saveLastPlayed(path);
      this.title = view.title;
      this.state = view.state;
      this.background = assetUrl("images", view.background);
      this.bgm = assetUrl("audios", view.bgm);
      this.presentCharacters = view.present_characters.map((c) => ({ ...c, iconId: c.icon, icon: assetUrl("images", c.icon) }));
      this.map = view.map ?? { nodes: [], edges: [] };
      this.log = [{ kind: "opening", text: view.description }];
      this.cacheWarned = false; // 新しいセッション = 新しいクライアント (計測もゼロから)
      // 開帳の保留 (spec 18) は前のプレイの揮発状態 — 新規/再開/ロードで必ず捨てる。
      this.pendingTail = [];
      this.pendingSe = [];
      this.pendingVisual = null;
      this.pendingSpeech = null;
      // spec 24/27: 挿絵まわりの揮発物を捨てる (backend も世代を進める)。
      this.dropVolatileImage();
      this.imageBusy = false;
      this.refStock = null; // 参照ストックはパッケージ別 — 開いたときに読み直す。
      // 決断待ち (B)・対決 (C) はセーブを跨いで生きる — 再開時に復元する (新規は null)。
      this.decision = view.decision ?? null;
      this.deciding = false;
      this.contest = view.contest ?? null;
      this.fighting = false;
      // あらすじ (spec 10): 新規開始は空、再開はセーブから全量復元。
      this.synopsis = view.synopsis ?? [];
      this.recentLog = view.recent_log ?? [];
      // 既成事実 (spec 20): 新規開始は空、再開はセーブから復元。権限は盤面の宣言に従う。
      this.facts = view.facts ?? [];
      this.factsPolicy = view.facts_policy ?? "locked";
      // 盤面が変わったら読み上げは必ず止める (前のゲームの語りが喋り続けない)。
      tts.stop();
      this.compacting = false;
      // scenario の lint (作者向け・非 fatal)。死んだ flag_hint 等を開幕で報せる。
      for (const w of view.warnings ?? []) {
        this.log.push({ kind: "system", text: `⚠ ${w}` });
      }
      if (view.resumed) {
        this.log.push({ kind: "system", text: t("store.resumeMarker", { turn: view.resumed.turn }) });
        if (view.resumed.last_narration) {
          this.log.push({ kind: "narration", text: view.resumed.last_narration });
        }
        for (const w of view.resumed.warnings) {
          this.log.push({ kind: "system", text: `⚠ ${w}` });
        }
      }
    },

    async newGame(packagePath?: string) {
      const path = packagePath ?? this.packagePath;
      if (!path) return;
      this.loading = true;
      this.error = null;
      try {
        // 言語設定タブの選択 (localStorage) を backend へ。却下理由の localize に効く。
        const lang = localStorage.getItem("kataribe.lang") || null;
        const view = await invoke<GameView>("new_game", { packagePath: path, lang });
        await this.applyGameView(view, path);
      } catch (e) {
        this.error = String(e);
      } finally {
        this.loading = false;
      }
    },

    // オートセーブから再開 (spec 07 Phase C)。正本と語りの継続性は backend が復元する。
    async resumeGame(packagePath?: string) {
      const path = packagePath ?? this.packagePath;
      if (!path) return;
      this.loading = true;
      this.error = null;
      try {
        const lang = localStorage.getItem("kataribe.lang") || null;
        const view = await invoke<GameView>("resume_game", { packagePath: path, lang });
        await this.applyGameView(view, path);
      } catch (e) {
        this.error = String(e);
      } finally {
        this.loading = false;
      }
    },

    // --- 手動セーブスロット (spec 07 Phase D) ---

    // スロット一覧を取得する。forSave=true はプレイ中 session のパッケージ (保存先の真実は
    // backend session が握る)、false はヘッダーで選択中のパッケージ (「続きから」と同じ意味論)。
    async listSlots(forSave: boolean): Promise<SlotView[]> {
      return await invoke<SlotView[]>("list_save_slots", {
        packagePath: forSave ? null : this.packagePath,
      });
    },

    // 現在のプレイ状態をスロットへ保存する (上書き確認はダイアログ側)。成功なら更新後の SlotView。
    async saveToSlot(slot: number): Promise<SlotView | null> {
      try {
        const v = await invoke<SlotView>("save_slot", { slot });
        this.logToast = t("store.slotSaved", { slot });
        // スロットが立った可能性があるので一覧の has_slots を取り直す (削除確認の材料)。
        this.refreshPackages();
        return v;
      } catch (e) {
        this.logToast = t("store.slotSaveFailed", { error: String(e) });
        return null;
      }
    },

    // スロットからロードして再開する。backend が GameSession を丸ごと差し替える =
    // プレイ中でも前のプレイは忘れられ、GM は次ターンからロードされた記憶だけを読み直す。
    async loadSlot(slot: number): Promise<boolean> {
      if (!this.packagePath) return false;
      this.loading = true;
      this.error = null;
      try {
        const lang = localStorage.getItem("kataribe.lang") || null;
        const view = await invoke<GameView>("load_slot", {
          packagePath: this.packagePath,
          slot,
          lang,
        });
        await this.applyGameView(view, this.packagePath);
        return true;
      } catch (e) {
        this.error = String(e);
        return false;
      } finally {
        this.loading = false;
      }
    },

    // --- ダイスの開帳 (spec 18 Phase A) ---

    setDiceReveal(on: boolean) {
      this.diceReveal = on;
      localStorage.setItem(DICE_REVEAL_KEY, String(on));
      // オフにした瞬間、開帳待ちが残っていれば全部開く (入力ロックの脱出口を兼ねる)。
      if (!on) this.revealAll();
    },

    // 次の 1 個を開帳する (1 クリック 1 判定)。開けるのは常に最初の未開帳 entry のみ。
    // 開いた判定の SE はこの瞬間に鳴らす (結末文と同期)。全部開いたら保留行を flush。
    revealNext(entryIndex: number) {
      if (entryIndex !== this.revealTargetIndex) return; // 直列規律 (先の行を先に開く)
      const e = this.log[entryIndex];
      if (e.kind === "rolls" && e.revealed < e.rolls.length) {
        e.revealed++;
      } else if (e.kind === "checks" && e.revealed < e.checks.length) {
        const c = e.checks[e.revealed];
        e.revealed++;
        this.playSe(assetUrl("audios", c.sound)); // 結末 SE は開帳と同時 (先に鳴ったら開帳の意味がない)
      } else if (e.kind === "statrolls" && e.revealed < e.stat_rolls.length) {
        e.revealed++;
      } else {
        return;
      }
      // spec 23 Phase B: 開帳の正はホスト session の RevealState — 単騎でも同じ経路を通す。
      // 演出はローカルで即時 (スクランブルの同期性を保つ)、カウンタは transport 越しに進める。
      // ローカル適用済みを計上 = reveal_order のエコーが二重に開かない (applyRevealOrder)。
      this.multi.revealApplied++;
      void transport
        .request<{ revealed: number; total: number }>("reveal_next")
        .then((rv) => tableHooks.onLocalReveal?.(rv))
        .catch(() => {});
      if (!this.hasUnrevealedDice) this.flushPendingDice();
    },

    // 全部開く (演出オフ切替や保険の脱出口)。SE は最後の 1 回だけ鳴らす (連打音を避ける)。
    revealAll() {
      for (const e of this.log) {
        if (e.kind === "rolls") e.revealed = e.rolls.length;
        else if (e.kind === "checks") e.revealed = e.checks.length;
        else if (e.kind === "statrolls") e.revealed = e.stat_rolls.length;
      }
      // spec 23 Phase B: session 側カウンタも全開帳へ (ゲーム未開始・保留無しでも無害)。
      void transport
        .request<{ revealed: number; total: number }>("reveal_all")
        .then((rv) => tableHooks.onLocalReveal?.(rv))
        .catch(() => {});
      this.flushPendingDice();
    },

    // --- 決断つき判定 (spec 18 Phase B) ---

    // 決断 (受け入れ / 押す / 買う) を backend で確定し、結果をログへ差し込む。
    // LLM は呼ばれない (トークン消費ゼロのプレイヤー op)。
    async resolveDecision(choice: "accept" | "push" | "buy", degree?: string) {
      if (!this.decision || this.deciding) return;
      this.deciding = true;
      try {
        const r = await transport.request<DecisionResultView>("resolve_dice_decision", {
          choice,
          degree: degree ?? null,
        });
        this.multi.revealApplied = 0; // 新しい伏せ束 (プッシュ 1 or 0)
        await prefetchAssets([
          ...collectTurnAssets(r.beats, [r.check]),
          ...collectViewAssets({ map: r.map }),
        ]);
        // 凍結されていた判定行 (pending) を最終結果で差し替える。
        const entryIdx = this.log.findIndex(
          (e) => e.kind === "checks" && e.checks.some((c) => c.pending),
        );
        const isPush = choice === "push" && this.diceReveal;
        // 演出オフのプッシュ: backend は新出目 1 枚を伏せ直すが UI は即開示 → 追認 (spec 23 Phase B)。
        if (choice === "push" && !this.diceReveal) void transport.request("reveal_all").catch(() => {});
        if (entryIdx >= 0) {
          const entry = this.log[entryIdx];
          if (entry.kind === "checks") {
            const itemIdx = entry.checks.findIndex((c) => c.pending);
            entry.checks[itemIdx] = r.check;
            if (isPush) {
              // 振り直し = 新しい出目 → もう一度伏せて開かせる (緊張の山場を二度作る)。
              entry.revealed = itemIdx;
            } else {
              // 受け入れ/買いは出目が変わらない → その場で確定表示 + 結末 SE。
              this.playSe(assetUrl("audios", r.check.sound));
            }
          }
        }
        // 帰結 (可変量ダイス/ビート/goal バナー): push の再開帳中は保留し、開帳完了で flush
        // (漏洩防止は Phase A と同じ機構を使い回す)。それ以外は直接ログへ。
        const pushTail = (e: LogEntry) => (isPush ? this.pendingTail.push(e) : this.log.push(e));
        if (r.stat_rolls.length) {
          pushTail({ kind: "statrolls", stat_rolls: r.stat_rolls, revealed: r.stat_rolls.length });
        }
        for (const b of r.beats) {
          if (b.narration.trim() || b.recalled.length) {
            pushTail({ kind: "beat", narration: b.narration, recalled: b.recalled, expanded: false });
          }
          if (isPush) this.pendingSe.push(assetUrl("audios", b.sound));
          else this.playSe(assetUrl("audios", b.sound));
        }
        // 決断の帰結で goal に達しうる (押して失敗 → HP0 の死など)。
        if (r.goal_reached) {
          if (r.goal_narration) pushTail({ kind: "authored", text: r.goal_narration });
          const goalLabel = r.goal_title ?? r.goal_id;
          const label = goalLabel
            ? t("store.clearedNamed", { goal: goalLabel })
            : t("store.clearedGeneric");
          pushTail({ kind: "system", text: label });
        }
        this.state = r.state;
        if (r.map) this.map = r.map;
        // 次の決断 (1 ターン複数凍結時) または null。
        this.decision = r.decision;
      } catch (e) {
        this.logToast = String(e);
      } finally {
        this.deciding = false;
      }
    },

    // 対決を 1 ラウンド進める (spec 18 Phase C)。LLM は呼ばれない (トークンゼロ)。
    // player の振りは伏せカードで開き、相手の振り・帰結文・ビートは開帳後に流れる。
    async playContestRound() {
      if (!this.contest || this.fighting) return;
      this.fighting = true;
      try {
        const r = await transport.request<ContestRoundView>("play_contest_round", {});
        this.multi.revealApplied = 0; // 新しい伏せ束 (player の 1 枚)
        await prefetchAssets([
          ...collectTurnAssets(r.beats, [r.player, r.opponent]),
          ...collectViewAssets({ map: r.map }),
        ]);
        const reveal = this.diceReveal;
        // 演出オフ: backend は player の振り 1 枚を伏せ直すが UI は即開示 → 追認 (spec 23 Phase B)。
        if (!reveal) void transport.request("reveal_all").catch(() => {});
        // player の振り: 伏せカード (開帳)。演出オフなら即開示。
        this.log.push({ kind: "checks", checks: [r.player], revealed: reveal ? 0 : 1 });
        // 相手の振り + ラウンド帰結文/SE は開帳後に (漏洩防止は Phase A の機構を使い回す)。
        const tail = (e: LogEntry) => (reveal ? this.pendingTail.push(e) : this.log.push(e));
        tail({ kind: "checks", checks: [r.opponent], revealed: 1 });
        if (!reveal) this.playSe(assetUrl("audios", r.opponent.sound));
        else this.pendingSe.push(assetUrl("audios", r.opponent.sound));
        if (r.stat_rolls.length) {
          tail({ kind: "statrolls", stat_rolls: r.stat_rolls, revealed: r.stat_rolls.length });
        }
        for (const b of r.beats) {
          if (b.narration.trim() || b.recalled.length) {
            tail({ kind: "beat", narration: b.narration, recalled: b.recalled, expanded: false });
          }
          if (reveal) this.pendingSe.push(assetUrl("audios", b.sound));
          else this.playSe(assetUrl("audios", b.sound));
        }
        if (r.ended) {
          tail({ kind: "system", text: r.ended.digest });
        }
        if (r.goal_reached) {
          if (r.goal_narration) tail({ kind: "authored", text: r.goal_narration });
          const goalLabel = r.goal_title ?? r.goal_id;
          tail({
            kind: "system",
            text: goalLabel
              ? t("store.clearedNamed", { goal: goalLabel })
              : t("store.clearedGeneric"),
          });
        }
        this.state = r.state;
        if (r.map) this.map = r.map;
        this.contest = r.contest; // 決着後は null → パネルが畳まれ入力が開く
      } catch (e) {
        this.logToast = String(e);
      } finally {
        this.fighting = false;
      }
    },

    // 開帳完了: 保留していた後続行 (ビート/goal バナー/エピローグ) と SE・CG を解き放つ。
    flushPendingDice() {
      for (const entry of this.pendingTail) this.log.push(entry);
      this.pendingTail = [];
      for (const se of this.pendingSe) this.playSe(se);
      this.pendingSe = [];
      if (this.pendingVisual) {
        this.background = this.pendingVisual.background;
        if (this.pendingVisual.bgm !== this.bgm) this.bgm = this.pendingVisual.bgm;
        this.pendingVisual = null;
      }
      if (this.pendingSpeech) {
        const text = this.pendingSpeech;
        this.pendingSpeech = null;
        if (this.ttsFeature && this.ttsEnabled) void tts.speak(text, { queue: true });
      }
    },

    async playTurn(action: string) {
      // 多人数の卓が動いている間、入力は「提出」になる (締切で束ねて 1 ターン = 決定 4)。
      if (this.multi.role !== "solo" && this.multi.started) {
        await this.submitPartyInput(action);
        return;
      }
      const trimmed = action.trim();
      if (!trimmed || this.loading || !this.started) return;
      this.log.push({ kind: "player", text: trimmed });
      this.loading = true;
      this.error = null;
      try {
        const turn = await transport.request<TurnView>("play_turn", { action: trimmed });
        await this.ingestTurn(turn);
      } catch (e) {
        this.error = String(e);
      } finally {
        this.loading = false;
        this.compacting = false; // 圧縮インジケータはターン完了で必ず解除
        this.writingEpilogue = false; // エピローグも同様
      }
    },

    // --- 多人数プレイ (spec 23 Phase C) ---

    // 入力窓へ自分の行動を提出する (再提出は上書き)。締切はホストが握る (決定 4)。
    async submitPartyInput(action: string, pass = false) {
      const text = action.trim();
      if ((!text && !pass) || !this.multi.myPeerId) return;
      try {
        const st = await transport.request<{ submitted: string[]; waiting: string[] }>(
          "submit_turn_input",
          { peerId: this.multi.myPeerId, action: text, pass },
        );
        // 自分の提出だけログに出す (他人の文面は narration が映す)。再提出は上書き表示しない。
        if (!this.multi.inputStatus?.submitted.includes(this.multi.myPeerId)) {
          this.log.push({ kind: "player", text: pass ? t("table.passLogged") : text });
        }
        this.multi.inputStatus = st;
        tableHooks.onLocalInputStatus?.(st);
      } catch (e) {
        this.error = String(e);
      }
    },

    // reveal_order の追従 (ホスト/他ゲスト発の開帳を自分の画面でも開く)。
    // 自分発のエコーは revealApplied が既に進んでいるので no-op (二重開帳しない)。
    applyRevealOrder(rv: { revealed: number; total: number }) {
      while (this.multi.revealApplied < rv.revealed) {
        const i = this.revealTargetIndex;
        if (i < 0) break;
        const e = this.log[i];
        if (e.kind === "rolls" && e.revealed < e.rolls.length) {
          e.revealed++;
        } else if (e.kind === "checks" && e.revealed < e.checks.length) {
          const c = e.checks[e.revealed];
          e.revealed++;
          this.playSe(assetUrl("audios", c.sound));
        } else if (e.kind === "statrolls" && e.revealed < e.stat_rolls.length) {
          e.revealed++;
        } else {
          break;
        }
        this.multi.revealApplied++;
      }
      if (!this.hasUnrevealedDice) this.flushPendingDice();
    },

    // 受け取った TurnView を会話ログ・状態パネル・演出へ描き込む共通部 (spec 23 Phase C:
    // 単騎の play_turn / 多人数の party_turn — ホストの締切実行もゲストの push 受信も
    // 同じ実体を通る。ゲストは state を宛先別に差し替えてから渡す)。
    async ingestTurn(turn: TurnView) {
        // 新しいダイス束 = 開帳追従位置をリセット (spec 23 reveal_order)。
        this.multi.revealApplied = 0;
        this.multi.timerRemaining = null;
        // このターンに載っているアセット ID を先に解決する (以後は同期で引ける)。
        await prefetchAssets([
          ...collectViewAssets(turn),
          ...collectTurnAssets(turn.beats, turn.checks),
        ]);
        // 開帳演出が有効で、このターンにダイスが在るか (spec 18 Phase A)。
        const hasDice =
          turn.accepted &&
          (turn.rolls.length > 0 || turn.checks.length > 0 || turn.stat_rolls.length > 0);
        const revealing = this.diceReveal && hasDice;
        // ダイスより後ろの行は開帳まで保留する (結果の漏洩防止)。revealing でなければ直挿し。
        const pushLog = (e: LogEntry) => {
          if (revealing) this.pendingTail.push(e);
          else this.log.push(e);
        };
        if (turn.accepted) {
          // 決断待ち (spec 18 Phase B)。開帳がすべて済んだ後にパネルが出る (表示条件は getter)。
          this.decision = turn.decision ?? null;
          // 対決 (spec 18 Phase C)。attempt_contest が開いたら ⚔ パネル。
          this.contest = turn.contest ?? null;
          if (turn.narration) {
            this.log.push({ kind: "narration", text: turn.narration });
            // 読み上げは narration だけ (判定結果やビートは読まない = ダイスの結果を
            // 開帳前に音声で漏らさない)。await しない = 語りの表示を音声待ちにしない。
            if (this.ttsFeature && this.ttsEnabled) void tts.speak(turn.narration);
          }
          // ダイス系 3 行は伏せて積む (revealed=0)。演出オフなら全開 (= 従来動作)。
          // 演出オフのときは session 側の開帳カウンタ (spec 23 Phase B) も即・全開へ寄せる
          // (backend はターン確定時に必ず伏せ直すので、UI が開いて見せた事実を追認させる)。
          if (hasDice && !revealing) void transport.request("reveal_all").catch(() => {});
          if (turn.rolls.length) {
            this.log.push({ kind: "rolls", rolls: turn.rolls, revealed: revealing ? 0 : turn.rolls.length });
          }
          if (turn.checks.length) {
            this.log.push({ kind: "checks", checks: turn.checks, revealed: revealing ? 0 : turn.checks.length });
            // 結末効果音: 演出中は各判定の開帳時に鳴らす (revealNext)。オフなら従来どおり即時。
            if (!revealing) for (const c of turn.checks) this.playSe(assetUrl("audios", c.sound));
          }
          // 可変量ダイス (spec 16): 「SAN -4 (1d6=4)」の監査行。
          if (turn.stat_rolls.length) {
            this.log.push({
              kind: "statrolls",
              stat_rolls: turn.stat_rolls,
              revealed: revealing ? 0 : turn.stat_rolls.length,
            });
          }
          for (const b of turn.beats) {
            // narration も recalled も無い「効果のみ」の発火はログに出さない (裸の ✦ を防ぐ)。
            // CG は turn.beats から、SE は下で別途処理するのでログに積まなくても失われない。
            if (b.narration.trim() || b.recalled.length) {
              pushLog({ kind: "beat", narration: b.narration, recalled: b.recalled, expanded: false });
            }
            // 発火 SE (受理ターンのみ)。ビートは判定の帰結でありうる = 開帳前に鳴ると漏洩。
            if (revealing) this.pendingSe.push(assetUrl("audios", b.sound));
            else this.playSe(assetUrl("audios", b.sound));
          }
          if (turn.attempts > 1) {
            // 自己修復は既定で畳む (⚠ アイコンのみ) — メタ情報の没入低下を避ける。
            // クリックで「N 回目で筋を通した」+ 却下理由を展開 (author 診断)。
            pushLog({ kind: "selfrepair", attempts: turn.attempts, reasons: turn.retries, expanded: false });
          }
          // goal 到達: 単発/終端なら goal_reached、campaign 継続なら transition で signal。
          if (turn.goal_reached || turn.transition) {
            // 結末ナレーション (authored) があれば語りとして出す (遷移元モジュールの結末)。
            if (turn.goal_narration) {
              pushLog({ kind: "authored", text: turn.goal_narration });
            }
            // 表示は authored title を優先し、無ければ id (機械用セレクタ) へフォールバック。
            const goalLabel = turn.goal_title ?? turn.goal_id;
            if (turn.transition) {
              // campaign: この章の結末 → 次モジュールへ。入力は締めず続行。
              const end = goalLabel
                ? t("store.chapterEndNamed", { goal: goalLabel })
                : t("store.chapterEndGeneric");
              pushLog({
                kind: "system",
                text: t("store.transitionTo", { end, module: turn.transition.module_title }),
              });
              // 遷移先モジュールの開幕描写。
              pushLog({ kind: "opening", text: turn.transition.description });
              // spec 24/27: 挿絵は章を跨がない (backend も scene_seq を進めて原本を捨てる)。
              this.dropVolatileImage();
            } else {
              // 単発シナリオ/キャンペーン終端 = クリア。
              const label = goalLabel
                ? t("store.clearedNamed", { goal: goalLabel })
                : t("store.clearedGeneric");
              pushLog({ kind: "system", text: label });
            }
          }
          // エピローグ (spec 11)。表示順 = 結末文 → バナー → エピローグで幕
          // (バナーが余韻をぶった切らない)。narration と同じ本文スタイルで積む
          // = 会話ログのテキスト保存にも自然に含まれる。
          if (turn.epilogue) {
            pushLog({ kind: "system", text: t("store.epilogueMarker") });
            pushLog({ kind: "narration", text: turn.epilogue });
            // エピローグは **LLM が書く本文** (epilogue_prompt は作者の生成指示であって
            // 本文ではない) なので読み上げ対象。ただし結末を語るため、ダイスが伏せられた
            // ままだと帰結を音声で漏らす → 開帳待ちなら flush まで保留する。
            // **queue** = 同じターンの語りを途中で切らず、その後ろに続けて読む。
            if (revealing) this.pendingSpeech = turn.epilogue;
            else if (this.ttsFeature && this.ttsEnabled) void tts.speak(turn.epilogue, { queue: true });
          }
        } else {
          this.log.push({ kind: "reject", reasons: turn.reasons, attempts: turn.attempts });
        }
        // キャッシュ健全性の警告 (#44/#45 — キャッシュの静かな漏出は usage が一次ソース)。
        // 連続 miss が閾値を越えた瞬間に 1 回だけ出す。ヒット復帰で再武装するエッジトリガー。
        // 初回リクエストは書き込みゆえ miss が正常 → total_requests>=2 で除外。
        const cs = turn.cache;
        if (cs.last_cache_read > 0) {
          this.cacheWarned = false;
        } else if (!this.cacheWarned && cs.total_requests >= 2 && cs.consecutive_misses >= 3) {
          this.cacheWarned = true;
          this.log.push({
            kind: "system",
            text: t("store.cacheWarning", { misses: cs.consecutive_misses }),
          });
        }
        // 既成事実 (spec 20): GM は書かないのでターン中は変わらない (変えるのはユーザー編集)。
        // 権限だけ campaign 遷移で追従する。
        if (turn.facts) this.facts = turn.facts;
        if (turn.facts_policy) this.factsPolicy = turn.facts_policy;
        // あらすじ (spec 10): 追記差分を push (append-only)。章が確定したら「最近の出来事」から
        // その章に呑まれた行 (turn <= upto_turn) を取り除く。会話ログには出さない
        // (物語の外の帳簿イベント — 更新はタブを見れば分かる、ユーザーFB 2026-07-14)。
        for (const line of turn.new_log ?? []) this.recentLog.push(line);
        for (const s of turn.new_synopsis ?? []) {
          this.synopsis.push(s);
          this.recentLog = this.recentLog.filter((l) => l.turn > s.upto_turn);
        }
        this.state = turn.state;
        this.presentCharacters = turn.present_characters.map((c) => ({ ...c, iconId: c.icon, icon: assetUrl("images", c.icon) }));
        // マップ (spec 15) — 移動/遷移で backend が差し替える (却下でも現状スナップショット)。
        if (turn.map) this.map = turn.map;
        // 背景は受理ターンのみ更新する。却下 = 物語が進んでいないので現在の背景 (=直前の CG) を保つ。
        // イベント CG は既定で瞬間 (spec 01 #3): 発火ターンに出て、次の受理ターンで場所背景へ復帰。
        // image_hold: show の CG は backend が turn.background に畳んで返すので、次ターン以降も残る。
        // hide のビートは「消す」指示なので背景候補にしない (image を併記していても出さない)。
        // campaign 遷移は前章の CG を持ち越さず遷移先の場所背景にする。
        if (turn.accepted) {
          const cgBeat = turn.transition
            ? undefined
            : [...turn.beats]
                .reverse()
                .find(
                  (b) =>
                    b.image &&
                    (b.image_mode ?? "background") === "background" &&
                    b.image_hold !== "hide",
                );
          const nextBackground = cgBeat?.image ? assetUrl("images", cgBeat.image) : assetUrl("images", turn.background);
          const nextBgm = assetUrl("audios", turn.bgm);
          if (revealing) {
            // 発火 CG・場面転換は判定の帰結でありうる = 開帳前に見えたら漏洩。flush で適用。
            this.pendingVisual = { background: nextBackground, bgm: nextBgm };
          } else {
            this.background = nextBackground;
            // BGM は場所変化で差し替え。同一 URL なら再代入せずループを切らさない (CG と違い持続)。
            if (nextBgm !== this.bgm) this.bgm = nextBgm;
          }
        }
    },
  },
});
