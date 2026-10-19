// 読み上げ (TTS)。**提示層だけの機能** — 正本 (gm_core) も prompt も語りも一切変えない。
// 読むのは受理ターンの narration だけで、TTS の ON/OFF で物語の書かれ方は変わらない
// (変わると chronicle/synopsis に残る記録まで再生設定で食い違う)。
//
// エンジンは `@aituber-onair/voice` に委ねる (MIT・ランタイム依存ゼロ)。エンジン差は
// ライブラリが吸収するので、Kataribe 側は**共通の 3 パラメータ (速度・高さ・音量) を
// エンジン別のキーへ写す**だけを持つ。
//
// **値の import は動的だけ** (`import type` は型のみで実体を持たない) — 静的に一つでも
// 値を取ると、TTS を使わないセッションでもライブラリが起動時に読み込まれる。
import type { VoiceEngineAdapter } from "@aituber-onair/voice";
import { invoke } from "@tauri-apps/api/core";

/** 対応エンジン。ブラウザ内蔵は導入ゼロ、ローカル 2 種は別アプリの常駐が要る。 */
export type TtsEngine = "webSpeech" | "voicevox" | "aivisSpeech" | "openaiCompatible";

/** 読み上げる範囲 (spec 35 Phase D)。`dialogue` は台詞の段だけを読む (地の文を読まない)。 */
export type TtsScope = "all" | "dialogue";

export interface TtsSettings {
  engine: TtsEngine;
  /** ローカルエンジンのサーバー URL。空なら既定値。 */
  serverUrl: string;
  /** 話者 ID (webSpeech は音声名)。空なら自動選択。 */
  speaker: string;
  /** 速度。1.0 = 標準 (どのエンジンも倍率なので共通)。 */
  rate: number;
  /** 高さ。0 = 標準 (エンジン別の実レンジへ写す)。 */
  pitch: number;
  /** 音量。1.0 = 標準・最大。 */
  volume: number;
  /** OpenAI 互換エンドポイントのモデル名 (**必須** — 空だとライブラリが例外を投げる)。 */
  model: string;
  /** 読み上げる範囲 (spec 35)。既定 `all` = 従来どおり地の文も台詞も読む。 */
  scope: TtsScope;
  /**
   * OpenAI 互換 (backend 経由、spec 36) の待ち時間 (秒)。**0 = 既定** (backend の 120 秒) — 既定値を
   * frontend に焼かない (あらすじの待ち時間と同じ流儀)。`toRouteConfig` が 0 を null に写す。
   */
  timeoutSecs: number;
}

/** ローカルエンジンの既定エンドポイント (ライブラリの定数と同じ値)。 */
export const DEFAULT_SERVER_URL: Record<TtsEngine, string> = {
  webSpeech: "",
  voicevox: "http://localhost:50021",
  aivisSpeech: "http://localhost:10101",
  // OpenAI 互換は**エンドポイント全体**を渡す (base URL ではない)。既定は
  // Irodori-TTS-Server の既定ポート。他の OpenAI 互換 TTS サーバーにも同じ口で刺さる。
  openaiCompatible: "http://localhost:8088/v1/audio/speech",
};

/**
 * 話者一覧をサーバーから引けるか。**OpenAI 互換は非対応** (ライブラリが例外を投げる) —
 * voice の語彙はサーバー実装ごとに違い、共通の列挙 API が無いため。UI は自由入力にする。
 */
export function supportsVoiceList(engine: TtsEngine): boolean {
  return engine !== "openaiCompatible";
}

/**
 * 高さ・音量を持つか。**OpenAI 互換は速度 (`speed`) しか持たない** — API の仕様であって
 * 実装の手抜きではないので、UI 側でスライダを無効化して誤解を作らない。
 */
export function supportsPitchAndVolume(engine: TtsEngine): boolean {
  return engine !== "openaiCompatible";
}

/** ブラウザ内蔵以外は外部サーバーが要る (UI が導入の注意を出す判断に使う)。 */
export function needsServer(engine: TtsEngine): boolean {
  return engine !== "webSpeech";
}

export const DEFAULT_SETTINGS: TtsSettings = {
  engine: "webSpeech",
  serverUrl: "",
  speaker: "",
  rate: 1.0,
  pitch: 0,
  volume: 1.0,
  model: "",
  scope: "all",
  timeoutSecs: 0,
};

const LS_FEATURE = "kataribe.tts.feature";
const LS_ENABLED = "kataribe.tts.enabled";
const LS_SETTINGS = "kataribe.tts.settings";
const LS_VOICES = "kataribe.tts.voices";
/**
 * キャラ別の声 (spec 35 Phase D)。**`LS_VOICES` (声一覧のキャッシュ) とは別のキー** —
 * 同じキーに書くと loadVoiceList / saveVoiceList と壊し合う。
 * 形は `{ [エンジン + サーバー URL]: { [パッケージ]: { [登場人物の id]: 話者 ID } } }`
 * (話者 ID はエンジンごとに違うので、エンジンを替えれば別の割り当てになる)。
 */
const LS_CHARACTER_VOICES = "kataribe.tts.characterVoices";

/**
 * キャラ別の声の「なし」(2026-10-15 ユーザー要望)。この登場人物の台詞は読まない。
 * 空の話者 ID (= 語り手と同じ) とは別の値 — 空は割り当てを消す意味で既に使っている。
 * 話者 ID と衝突しない綴りにする (VOICEVOX は数字、AivisSpeech は数字、webSpeech は音声名)。
 */
export const VOICE_NONE = "__none__";

/**
 * 合成と再生の継ぎ目 (spec 36 D1)。鎖 (`chain`) / 世代 / スキップはこの外にあり、実装が 2 つある:
 * - `LibrarySynth` = `@aituber-onair/voice` の `VoiceEngineAdapter` の包み (webSpeech / VOICEVOX / AivisSpeech)
 * - `BackendSynth` = backend command `synthesize_speech` → Blob URL → `HTMLAudioElement` (OpenAI 互換)
 *
 * OpenAI 互換だけを backend に回す理由 (failures #114): サーバーに CORS が無いと WebView からは
 * 届かず、届いてもライブラリの fetch は 30 秒固定で CPU の合成に収まらない。
 */
interface Synth {
  /** 1 文を合成して再生し終えるまで待つ。中断されたら reject。 */
  speak(sentence: string, voice: string): Promise<void>;
  /** 再生中の音を止め、進行中の speak を reject させる。 */
  stop(): void;
  isPlaying(): boolean;
  /**
   * 再生中に止めたら捨てて作り直すか (#72: ライブラリのアダプタは再生中に stop() すると
   * `isProcessingQueue` が true のまま残り二度と喋らない)。backend 側は持つ状態が再生中の
   * Audio 1 つだけなので捨てる理由が無い。**挙動の分岐はこの 1 bit に閉じる**。
   */
  readonly discardOnStop: boolean;
}

let synth: Synth | null = null;
let booting: Promise<Synth | null> | null = null;
/** 合成器を組んだ時の設定。変わったら組み直す。 */
let builtWith = "";
/** 文分割 (長文の打ち切り対策)。アダプタと同じ動的 import から受け取る。 */
let splitSentence: ((text: string) => string[]) | null = null;
/** 選んだ日本語音声名 (webSpeech)。アダプタを組み直すたびに探し直さないため。 */
let pickedJaVoice = "";
/** 語り手の声 (設定の話者、未指定なら自動選択)。合成器を組むときに決まる。 */
let narratorVoice = "";
/** 今読み上げている世代。stop() / 次のターンで進め、古い世代の残りを捨てる。 */
let generation = 0;
/**
 * 読み上げの直列鎖。**1 文ずつ順に合成・再生する**ための連結 Promise。
 *
 * まとめてアダプタへ投入すると、ネットワーク型エンジン (VOICEVOX/AivisSpeech) では
 * 全文の合成リクエストが**一斉に並列発射**される。ローカル GPU に十数本を同時に投げると
 * 後続がライブラリ既定の 30 秒タイムアウトに掛かり、こちらは失敗を握り潰すので
 * **その文が黙って飛ぶ** (読み上げが途中で切れたように聞こえる)。
 *
 * かといって 1 文ずつ await して積むと、その隙間に後発の `queue: true` が割り込んで
 * 順序が崩れる。**自前の鎖に繋ぐ**ことで、直列と順序保存を同時に満たす。
 */
let chain: Promise<void> = Promise.resolve();
/**
 * 段 (セリフ) の通し番号。スキップを「次のセリフへ」にするため、鎖に積む文に
 * どの段の文かを持たせる (2026-10-15 ユーザー要望 — 従来はターン全体を捨てていた)。
 */
let partSeq = 0;
/** 最後に読み始めた段の番号。スキップはこの段の残りの文を捨てる。 */
let lastStartedPart = 0;
/** この番号以下の段は読まない (スキップ済み)。 */
let skipThrough = 0;

/** この環境で読み上げできるか (WebView2/Chromium は speechSynthesis を持つ)。 */
export function isSupported(): boolean {
  return typeof globalThis.speechSynthesis !== "undefined";
}

/**
 * 読み上げ機能を使うか (設定サウンドタブのチェックボックス、localStorage 永続)。
 *
 * **既定 OFF** — 旧 `use_tts` (作者宣言ゲート) の撤去 (2026-08-31) に伴い、可否は
 * プレイヤー側のこの一本になった (imageGen の `enabled` と同型 = 会話ペインの操作列の
 * 表示条件 + speak の前提条件)。既定 ON にすると全盤面で勝手に喋り出す。
 */
export function loadFeature(): boolean {
  return localStorage.getItem(LS_FEATURE) === "1";
}

export function saveFeature(on: boolean): void {
  localStorage.setItem(LS_FEATURE, on ? "1" : "0");
}

/**
 * クイックトグルの ON/OFF (会話ペイン右下のホバー操作、localStorage 永続)。
 *
 * **未設定は ON** — 設定の機能スイッチ (`loadFeature`、既定 OFF) を入れた瞬間に、
 * もう一段のトグルを探して押さなくても鳴り始めるように。機能スイッチの後ろに居るので、
 * 既定 ON でも勝手に喋り出す事故は起きない。
 */
export function loadEnabled(): boolean {
  return localStorage.getItem(LS_ENABLED) !== "0";
}

export function saveEnabled(on: boolean): void {
  localStorage.setItem(LS_ENABLED, on ? "1" : "0");
}

export function loadSettings(): TtsSettings {
  try {
    const raw = localStorage.getItem(LS_SETTINGS);
    if (!raw) return { ...DEFAULT_SETTINGS };
    // 欠けたキーは既定で埋める (設定項目が増えても古い保存を壊さない)。
    return { ...DEFAULT_SETTINGS, ...(JSON.parse(raw) as Partial<TtsSettings>) };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

export function saveSettings(s: TtsSettings): void {
  localStorage.setItem(LS_SETTINGS, JSON.stringify(s));
  // 次の読み上げで新しい設定のアダプタが組まれる。
  invalidate();
}

/** 設定が変わったので合成器を捨てる (次回 speak で組み直す)。 */
export function invalidate(): void {
  stop();
  synth = null;
  booting = null;
  builtWith = "";
}

/** 実効サーバー URL (未入力なら既定)。 */
export function effectiveUrl(s: TtsSettings): string {
  return s.serverUrl.trim() || DEFAULT_SERVER_URL[s.engine];
}

/**
 * 共通 3 パラメータをエンジン別のキーへ写す。**レンジの意味が違う所だけ変換する**:
 * 速度はどれも倍率なのでそのまま、高さは webSpeech が 0〜2 (1=標準)・ローカル 2 種が
 * ±0.15 (0=標準) なので、Kataribe の −1〜+1 (0=標準) から写す。
 */
function engineOptions(s: TtsSettings): Record<string, unknown> {
  switch (s.engine) {
    case "voicevox":
      return {
        engineType: "voicevox",
        voicevoxApiUrl: effectiveUrl(s),
        voicevoxSpeedScale: s.rate,
        voicevoxPitchScale: s.pitch * 0.15,
        voicevoxVolumeScale: s.volume,
      };
    case "aivisSpeech":
      return {
        engineType: "aivisSpeech",
        aivisSpeechApiUrl: effectiveUrl(s),
        aivisSpeechSpeedScale: s.rate,
        aivisSpeechPitchScale: s.pitch * 0.15,
        aivisSpeechVolumeScale: s.volume,
      };
    case "openaiCompatible":
      // 実サーバーで確かめた (2026-10-19、Irodori-TTS-Server 8088、failures #114): 送る形は正しく
      // curl では 200 で WAV が返るが、**WebView からは届かない** — サーバーに CORS が無く preflight
      // が 405 (fetch は net::ERR_FAILED)。届いてもライブラリの fetch は 30 秒固定で、CPU の合成
      // (3.6 秒の音声に 45 秒) は毎回切れる。速度以外のパラメータは API に無い。
      // `voice` は空だと送らない = Irodori は `IRODORI_DEFAULT_VOICE` 未設定なら 400 なので
      // 話者 ID に `none` (Voice Design) か voices/ のファイル名を入れる。
      return {
        engineType: "openaiCompatible",
        openAiCompatibleApiUrl: effectiveUrl(s),
        openAiCompatibleModel: s.model.trim(),
        openAiCompatibleSpeed: s.rate,
      };
    default:
      return {
        engineType: "webSpeech",
        webSpeechRate: s.rate,
        webSpeechPitch: 1 + s.pitch,
        webSpeechVolume: s.volume,
        webSpeechLanguage: "ja-JP",
      };
  }
}

/**
 * 日本語の音声を一つ選ぶ (webSpeech で話者未指定のとき)。speechSynthesis の既定は
 * OS ロケール次第で英語音声になりうるので、ja を明示的に拾う。
 */
async function pickJapaneseVoice(): Promise<string> {
  // 一度見つけたら覚える。中断のたびにアダプタを組み直す (stop() の注記) ので、
  // ここで毎回 voiceschanged を最大 1 秒待つと読み上げの出だしが遅れる。
  if (pickedJaVoice) return pickedJaVoice;
  const synth = globalThis.speechSynthesis;
  if (!synth) return "";
  let voices = synth.getVoices();
  if (voices.length === 0) {
    voices = await new Promise<SpeechSynthesisVoice[]>((resolve) => {
      const done = () => resolve(synth.getVoices());
      const timer = setTimeout(done, 1000);
      synth.addEventListener("voiceschanged", () => {
        clearTimeout(timer);
        done();
      }, { once: true });
    });
  }
  // 見つからなかった時は覚えない (後から voices が揃う環境で拾い直せるように)。
  const found = voices.find((v) => /^ja/i.test(v.lang))?.name ?? "";
  if (found) pickedJaVoice = found;
  return found;
}

/** アダプタを遅延生成する (TTS を使うまでエンジン群のチャンクを読み込まない)。 */
/** ライブラリのアダプタの包み。声は `updateOptions({speaker})` で、変わるときだけ呼ぶ。 */
class LibrarySynth implements Synth {
  readonly discardOnStop = true;
  private currentVoice: string;
  constructor(private readonly adapter: VoiceEngineAdapter, initialVoice: string) {
    this.currentVoice = initialVoice;
  }
  async speak(sentence: string, voice: string): Promise<void> {
    // 鎖は 1 文ずつ合成と再生を待つので、ここで切り替えても前の文に混ざらない
    // (ライブラリは合成のたびに options.speaker を読む)。
    if (voice !== this.currentVoice) {
      this.adapter.updateOptions({ speaker: voice });
      this.currentVoice = voice;
    }
    await this.adapter.speakText(sentence);
  }
  stop(): void {
    this.adapter.stop();
  }
  isPlaying(): boolean {
    return this.adapter.isPlaying();
  }
}

/** backend command `synthesize_speech` の送り側の形 (契約 `TtsBackendRoute.config`)。 */
export interface TtsRouteConfig {
  endpoint: string;
  model: string;
  voice: string;
  speed: number;
  timeout_secs: number | null;
}

/** backend の失敗 4 種 (契約 `TtsBackendRoute.errors`)。UI がこれを区別して文面にする。 */
export type RouteFailure =
  | { kind: "http"; status: number; body_head: string }
  | { kind: "timeout"; secs: number }
  | { kind: "network"; detail: string }
  | { kind: "shape"; detail: string };

/** backend 経路の失敗。`failure.kind` で区別できる (404 も timeout も「失敗」の一語に潰さない)。 */
export class RouteError extends Error {
  constructor(readonly failure: RouteFailure) {
    super(describeFailure(failure));
    this.name = "RouteError";
  }
  /** invoke の reject 値 (serde tag `kind` の object) から。形が違えば network 扱い。 */
  static from(e: unknown): RouteError {
    if (e instanceof RouteError) return e;
    const f = e as Partial<RouteFailure> | null;
    if (f && typeof f === "object" && typeof f.kind === "string") return new RouteError(f as RouteFailure);
    return new RouteError({ kind: "network", detail: String(e) });
  }
}

function describeFailure(f: RouteFailure): string {
  switch (f.kind) {
    case "http":
      return `HTTP ${f.status}: ${f.body_head}`;
    case "timeout":
      return `timeout after ${f.secs}s`;
    case "network":
      return `network: ${f.detail}`;
    default:
      return `shape: ${f.detail}`;
  }
}

/**
 * 失敗を人の読む文に (UI 用)。backend の 4 種は i18n の文面へ、それ以外は String(e)。
 * tts.ts は i18n を import しない (store と部品が持つ `t` を遅延 import すると循環になる) ので、
 * 文面の鍵と引数だけ返す純関数 `routeMessage` を `describeError` が引く。
 */
export function routeMessage(f: RouteFailure): { key: string; args: Record<string, string | number> } {
  switch (f.kind) {
    case "http":
      return { key: "settings.voice.routeHttp", args: { status: f.status, body: f.body_head } };
    case "timeout":
      return { key: "settings.voice.routeTimeout", args: { secs: f.secs, warmup: WARMUP_TIMEOUT_SECS } };
    case "network":
      return { key: "settings.voice.routeNetwork", args: { detail: f.detail } };
    default:
      return { key: "settings.voice.routeShape", args: { detail: f.detail } };
  }
}

/** UI の文面。`translate` は i18n の `t` (呼び出し側が渡す)。 */
export function describeError(e: unknown, translate?: (key: string, args?: Record<string, string | number>) => string): string {
  if (e instanceof RouteError) {
    if (!translate) return e.message;
    const { key, args } = routeMessage(e.failure);
    return translate(key, args);
  }
  return String(e);
}

/** 中断 (stop / skip / 次のターン)。失敗ではないので通知しない。 */
class Interrupted extends Error {
  constructor() {
    super("interrupted");
    this.name = "Interrupted";
  }
}

/**
 * 設定 → backend の config (純関数、契約 `TtsBackendRoute.config`)。
 * - `voice` は**必ず入れる**: 空なら `none` (Irodori の参照なし。省くと `IRODORI_DEFAULT_VOICE`
 *   未設定で 400 = #114)。他サーバーで `none` が通らなければ http の本文がそのまま画面に出る
 * - `timeout_secs`: 設定の 0 は null (= backend の既定 120)。`minTimeoutSecs` は試聴のウォームアップ
 *   (初回のモデル読み込みは実測 439 秒 — 120 では必ず切れる) で max(設定, 600) にするため
 */
export function toRouteConfig(s: TtsSettings, voice: string | null, minTimeoutSecs = 0): TtsRouteConfig {
  const set = Number.isFinite(s.timeoutSecs) && s.timeoutSecs > 0 ? Math.floor(s.timeoutSecs) : 0;
  const eff = Math.max(set, minTimeoutSecs);
  return {
    endpoint: effectiveUrl(s),
    model: s.model.trim(),
    voice: (voice ?? s.speaker).trim() || "none",
    speed: s.rate,
    timeout_secs: eff > 0 ? eff : null,
  };
}

type BusyListener = (busy: boolean) => void;
type FailureListener = (err: RouteError) => void;
const busyListeners = new Set<BusyListener>();
const failureListeners = new Set<FailureListener>();
let busyCount = 0;

/** backend で合成中か (読み上げの操作列が「合成中」を出す = サーバーの直列待ちを無音にしない)。 */
export function onSynthesizing(cb: BusyListener): () => void {
  busyListeners.add(cb);
  return () => busyListeners.delete(cb);
}

/** 本番の読み上げで backend 経路が失敗したとき (試聴は投げるのでここには来ない)。 */
export function onRouteFailure(cb: FailureListener): () => void {
  failureListeners.add(cb);
  return () => failureListeners.delete(cb);
}

function setBusy(delta: 1 | -1): void {
  const before = busyCount > 0;
  busyCount = Math.max(0, busyCount + delta);
  const after = busyCount > 0;
  if (before !== after) for (const cb of busyListeners) cb(after);
}

function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/**
 * backend 経由の合成器 (spec 36)。1 文 = 1 invoke。再生は Blob URL + `HTMLAudioElement`
 * (CSP の `media-src` に `data:` は無く `blob:` はある)。
 *
 * `stop()` は `pause()` して**自分で reject する** — ライブラリの stop が play の Promise を
 * settle しない罠 (#72) を自分では踏まない。進行中の invoke は止められない (Tauri の command に
 * cancel は無い) ので、解決後に `stopped` を見て**再生しない** (URL も作らない)。
 */
class BackendSynth implements Synth {
  readonly discardOnStop = false;
  private stopped = false;
  private current: { audio: HTMLAudioElement; url: string; reject: (e: Error) => void; cleanup: () => void } | null = null;
  constructor(
    private readonly settings: TtsSettings,
    private readonly minTimeoutSecs = 0,
  ) {}
  async speak(sentence: string, voice: string): Promise<void> {
    this.stopped = false;
    const config = toRouteConfig(this.settings, voice, this.minTimeoutSecs);
    setBusy(1);
    let view: { mime: string; data_b64: string };
    try {
      view = await invoke<{ mime: string; data_b64: string }>("synthesize_speech", { config, text: sentence });
    } catch (e) {
      throw RouteError.from(e);
    } finally {
      setBusy(-1);
    }
    if (this.stopped) throw new Interrupted();
    const url = URL.createObjectURL(new Blob([base64ToBytes(view.data_b64)], { type: view.mime }));
    const audio = new Audio(url);
    const vol = this.settings.volume;
    audio.volume = Number.isFinite(vol) ? Math.min(1, Math.max(0, vol)) : 1;
    await new Promise<void>((resolve, reject) => {
      const cleanup = () => {
        audio.removeEventListener("ended", done);
        audio.removeEventListener("error", fail);
        URL.revokeObjectURL(url);
        if (this.current?.audio === audio) this.current = null;
      };
      const done = () => {
        cleanup();
        resolve();
      };
      const fail = () => {
        cleanup();
        reject(new RouteError({ kind: "shape", detail: "audio playback failed" }));
      };
      audio.addEventListener("ended", done);
      audio.addEventListener("error", fail);
      this.current = { audio, url, reject, cleanup };
      audio.play().catch(fail);
    });
  }
  stop(): void {
    this.stopped = true;
    const cur = this.current;
    if (!cur) return;
    cur.audio.pause();
    cur.cleanup();
    cur.reject(new Interrupted());
  }
  isPlaying(): boolean {
    return this.current !== null;
  }
}

async function ensureSynth(minTimeoutSecs = 0): Promise<Synth | null> {
  const s = loadSettings();
  const key = JSON.stringify(s) + "|" + minTimeoutSecs;
  if (synth && builtWith === key) return synth;
  if (booting && builtWith === key) return booting;
  builtWith = key;
  booting = (async () => {
    if (s.engine === "webSpeech" && !isSupported()) return null;
    const mod = await import("@aituber-onair/voice");
    splitSentence = mod.splitSentence;
    if (s.engine === "openaiCompatible") {
      // backend 経由 (spec 36)。語り手の声は設定の話者そのもの (空なら toRouteConfig が none にする)。
      narratorVoice = s.speaker.trim();
      synth = new BackendSynth(s, minTimeoutSecs);
      return synth;
    }
    const speaker =
      s.speaker.trim() || (s.engine === "webSpeech" ? await pickJapaneseVoice() : "");
    narratorVoice = speaker;
    const adapter = new mod.VoiceEngineAdapter({
      ...engineOptions(s),
      speaker,
    } as never);
    synth = new LibrarySynth(adapter, speaker);
    return synth;
  })();
  return booting;
}

export interface VoiceOption {
  id: string;
  label: string;
}

/**
 * 取得済みの話者一覧のキャッシュ。**話者 ID だけ保存しても選択肢が無ければ select は
 * 空白に見える**ので、一覧ごと持ち越す (設定を開き直すたびにサーバーへ問い合わせない
 * 利点もある)。エンジンとサーバー URL を鍵にし、どちらか変わったら捨てる
 * (別エンジンの ID は通用しない)。
 */
export function loadVoiceList(s: TtsSettings): VoiceOption[] {
  try {
    const raw = localStorage.getItem(LS_VOICES);
    if (!raw) return [];
    const c = JSON.parse(raw) as { engine?: string; url?: string; voices?: VoiceOption[] };
    if (c.engine !== s.engine || c.url !== effectiveUrl(s)) return [];
    return c.voices ?? [];
  } catch {
    return [];
  }
}

export function saveVoiceList(s: TtsSettings, voices: VoiceOption[]): void {
  localStorage.setItem(
    LS_VOICES,
    JSON.stringify({ engine: s.engine, url: effectiveUrl(s), voices }),
  );
}

export function clearVoiceList(): void {
  localStorage.removeItem(LS_VOICES);
}

/** キャラ別の声の全体。`{ [エンジン + URL]: { [パッケージ]: { [id]: 話者 ID } } }`。 */
export type CharacterVoiceMap = Record<string, Record<string, Record<string, string>>>;

/** 話者 ID はエンジンとサーバーで意味が変わるので、その組を鍵にする。 */
export function voiceEngineKey(s: TtsSettings): string {
  return `${s.engine}|${effectiveUrl(s)}`;
}

function loadCharacterVoiceMap(): CharacterVoiceMap {
  try {
    const raw = localStorage.getItem(LS_CHARACTER_VOICES);
    return raw ? (JSON.parse(raw) as CharacterVoiceMap) : {};
  } catch {
    return {};
  }
}

/** いまのエンジンとパッケージでの割り当て (登場人物の id → 話者 ID)。 */
export function loadCharacterVoices(s: TtsSettings, pkg: string): Record<string, string> {
  return { ...(loadCharacterVoiceMap()[voiceEngineKey(s)]?.[pkg] ?? {}) };
}

/**
 * 割り当てを 1 件書き換える (純関数)。空の話者 ID は「語り手と同じ」= 割り当てを消す。
 * 空になったパッケージ・エンジンの箱も畳む (保存を太らせない)。
 */
export function withCharacterVoice(
  map: CharacterVoiceMap,
  engineKey: string,
  pkg: string,
  entity: string,
  voiceId: string,
): CharacterVoiceMap {
  const next: CharacterVoiceMap = JSON.parse(JSON.stringify(map));
  const byPkg = (next[engineKey] ??= {});
  const byEntity = (byPkg[pkg] ??= {});
  if (voiceId.trim()) byEntity[entity] = voiceId.trim();
  else delete byEntity[entity];
  if (Object.keys(byEntity).length === 0) delete byPkg[pkg];
  if (Object.keys(byPkg).length === 0) delete next[engineKey];
  return next;
}

export function saveCharacterVoice(s: TtsSettings, pkg: string, entity: string, voiceId: string): void {
  const next = withCharacterVoice(loadCharacterVoiceMap(), voiceEngineKey(s), pkg, entity, voiceId);
  localStorage.setItem(LS_CHARACTER_VOICES, JSON.stringify(next));
}

/** 読み上げる段 (TurnView.segments の必要な欄だけ)。地の文は `name` が空。 */
export interface SpokenSegment {
  speaker: string | null;
  name: string;
  text: string;
}

/** 読み上げる 1 片。`voice` が null なら語り手の声。 */
export interface SpeechPart {
  text: string;
  voice: string | null;
}

/**
 * 段から「何をどの声で読むか」を決める (純関数)。
 *
 * - 地の文: 語り手の声。範囲が `dialogue` なら読まない
 * - 登場人物の台詞: 割り当てがあればその声、無ければ語り手の声
 * - モブの台詞 (`speaker` null・`name` あり): **地の文として扱う** (範囲が `dialogue` なら読まない)。
 *   store は先に `foldMobLines` で `名前「…」` の地の文へ畳んでから渡すが、ここでも同じ扱いにする
 * - 声を「なし」(`VOICE_NONE`) にした人物の台詞: 読まない
 */
/** 主人公の話者 id (gm_core `PLAYER`)。主人公の台詞は吹き出しで出すが読み上げない (2026-10-16)。 */
export const PLAYER_SPEAKER = "player";

export function planSegments(
  segments: SpokenSegment[],
  scope: TtsScope,
  voices: Record<string, string>,
): SpeechPart[] {
  const out: SpeechPart[] = [];
  for (const seg of segments) {
    const text = seg.text.trim();
    if (!text) continue;
    // 主人公の台詞は読まない (プレイヤー自身の言葉に声を付けない)。
    if (seg.speaker === PLAYER_SPEAKER) continue;
    // 台詞は登場人物のものだけ。モブ (speaker なし) は地の文 (2026-10-15 ユーザー要望)。
    const isLine = seg.name.trim() !== "" && seg.speaker !== null;
    if (!isLine && scope === "dialogue") continue;
    const voice = isLine && seg.speaker ? voices[seg.speaker] ?? null : null;
    // 声を「なし」にした人物の台詞は読まない (地の文と他の人物は読む)。
    if (voice === VOICE_NONE) continue;
    out.push({ text, voice });
  }
  return out;
}

/** 設定画面の話者一覧。ローカルエンジンはサーバーへ問い合わせる (失敗は例外)。 */
export async function listVoices(
  s: TtsSettings,
): Promise<VoiceOption[]> {
  const mod = await import("@aituber-onair/voice");
  const voices = await mod.getVoiceEngineVoiceList(s.engine, {
    apiUrl: effectiveUrl(s) || undefined,
    language: "ja-JP",
  });
  return voices.map((v) => ({ id: v.id, label: v.label }));
}

/**
 * 読み上げる。既定は**前の読み上げを打ち切って置き換える** — ターンが進んだのに前の語りが
 * 喋り続けるのは「矛盾しない GM」の音声版の破れになる。
 *
 * `queue: true` は**前に続けて足す**。同じターンの中で後から出る文 (エピローグ) 用で、
 * 語りを途中で切らずに繋ぐ。
 *
 * 長文は文単位に割る (Chrome 系は 1 発話が長いと途中で切れる既知の癖があり、ライブラリの
 * `splitSentence` がそのまま使える)。**自前の直列鎖 (`chain`) に繋ぐ**のが要点 —
 * まとめて投入すると合成リクエストが並列発射されてタイムアウトで文が飛び、1 文ずつ
 * await して積むと隙間に後発が割り込む。鎖なら直列と順序保存を同時に満たす。
 */
export async function speak(text: string, opts?: { queue?: boolean }): Promise<void> {
  // 平らな文字列は地の文として読む (エピローグ・段の無い語り)。範囲が台詞だけなら読まない。
  if (loadSettings().scope === "dialogue") return;
  await speakParts([{ text, voice: null }], opts);
}

/**
 * 段ごとに声を変えて読む (spec 35 Phase D)。`pkg` はキャラ別の声を引くパッケージの鍵。
 * 打ち切り・続けて足すの扱いは `speak` と同じ。
 */
export async function speakSegments(
  segments: SpokenSegment[],
  pkg: string,
  opts?: { queue?: boolean },
): Promise<void> {
  const s = loadSettings();
  await speakParts(planSegments(segments, s.scope, loadCharacterVoices(s, pkg)), opts);
}

/**
 * セリフ 1 つを読み直す (会話ログの吹き出しの読み上げボタン)。前の読み上げは打ち切る。
 *
 * **範囲 (台詞だけ / すべて) と「なし」は見ない** — 押したのはその一言を聞くためなので、
 * 設定で読まないことにしていても読む。声を「なし」にした人物は語り手の声で読む
 * (読まない設定に声の割り当ては無い)。
 */
export async function replaySegment(seg: SpokenSegment, pkg: string): Promise<void> {
  const text = seg.text.trim();
  if (!text || seg.speaker === PLAYER_SPEAKER) return;
  const assigned = seg.speaker ? loadCharacterVoices(loadSettings(), pkg)[seg.speaker] ?? null : null;
  await speakParts([{ text, voice: assigned === VOICE_NONE ? null : assigned }]);
}

async function speakParts(parts: SpeechPart[], opts?: { queue?: boolean }): Promise<void> {
  if (parts.every((p) => !p.text.trim())) return;
  if (!opts?.queue) {
    // 割り込み = 前の読み上げを捨てる。世代を進めるのもアダプタの始末も stop() が担う
    // ので、**アダプタを取る前に**呼ぶ (後だと今から使うアダプタを捨ててしまう)。
    stop();
    chain = Promise.resolve(); // 前の鎖は世代チェックで空回りするので繋ぎ直す
  }
  // 続けて足す時は現在の世代に相乗りする (互いを無効化しない)。
  const mine = generation;
  if (!(await ensureSynth())) return;
  // 合成器の生成を待つ間に stop() / 次のターンが来ていたら、もう投入しない。
  if (mine !== generation) return;
  for (const part of parts) {
    const body = part.text.trim();
    if (!body) continue;
    const seq = ++partSeq;
    const sentences = splitSentence ? splitSentence(body) : [body];
    for (const sentence of sentences) {
      chain = chain.then(async () => {
        // 鎖に積んだ後で打ち切られたら (stop / 次のターン) 残りは読まない。
        // スキップされた段の残りの文も読まない (次の段から続ける)。
        if (mine !== generation || seq <= skipThrough) return;
        // 合成器は文ごとに取り直す — スキップで再生中のアダプタを捨てた (stop() の注記)
        // 後も、同じ鎖の続きを新しい合成器で読むため。
        const sy = await ensureSynth();
        if (!sy || mine !== generation || seq <= skipThrough) return;
        lastStartedPart = seq;
        // 声は鎖の中で切り替える (合成器が「変わるときだけ」を担う)。
        const want = part.voice ?? narratorVoice;
        // stop() は待機中の Promise を reject する = 中断の正常系。読み上げ失敗もプレイを
        // 止める理由にならないので鎖は止めないが、backend 経路の失敗 (404 / timeout / 接続) は
        // **沈黙にしない** — 購読者 (store のトースト) へ渡す (spec 36 D6)。
        await sy.speak(sentence, want).catch((e: unknown) => {
          if (e instanceof RouteError) for (const cb of failureListeners) cb(e);
        });
      });
    }
  }
  await chain;
}

/**
 * 設定画面の試聴。**失敗を投げる** — speak() は握り潰すが、こちらは
 * 「サーバーが起動していない」等をユーザーに見せるのが目的 (設定の接地)。
 */
export async function test(sample: string, voice?: string): Promise<void> {
  invalidate();
  // OpenAI 互換 (backend 経由) の試聴は**ウォームアップを兼ねる** (spec 36 D6): 初回はサーバーが
  // モデルを読み込むので (実測 439 秒)、設定の待ち時間と 600 秒の大きい方で待つ。
  const sy = await ensureSynth(WARMUP_TIMEOUT_SECS);
  if (!sy) throw new Error("speechSynthesis unavailable");
  // キャラ別の声の試聴 (spec 35)。語り手と違う声なら切り替えてから読む。
  await sy.speak(sample, voice ?? narratorVoice);
  // 試聴用の長い待ち時間を本番に持ち越さない。
  invalidate();
}

/** 試聴の待ち時間の下限 (秒)。初回のモデル読み込み (実測 439 秒) を収める。 */
export const WARMUP_TIMEOUT_SECS = 600;

/**
 * 読み上げを即座に止める (スキップ / OFF / 新しいターン / ゲーム切り替え)。
 *
 * **再生中に止めたアダプタは二度と喋らない** (ライブラリの罠・実測で確認):
 * `BrowserAudioPlayer.stop()` は `audioElement.pause()` と `currentTime = 0` をするだけで
 * `play()` の Promise を settle しない (pause は `ended` も `error` も発火しない)。すると
 * `VoiceEngineAdapter.processQueue` が `await playPreparedSpeech` で永久に止まり
 * **`isProcessingQueue` が true のまま残る** → 以後の `speakText` は
 * `if (this.isProcessingQueue) return;` で即座に捨てられる (キューに積まれたまま鳴らない)。
 * こちらは失敗を握り潰す設計なので、症状は例外でなく**無音**として現れる。
 *
 * ゆえに**再生中に止めたときだけ**アダプタを捨てて次の speak で組み直す。合成待ちや
 * 停止済みならライブラリのキューは正常に巻き戻るので、そのまま使い回す。
 * ブラウザ内蔵 (webSpeech) は self-playing で `speechSynthesis.cancel()` が発話を
 * settle させるため本来この罠を踏まないが、判定を分けても得がないので一律で扱う。
 */
export function stop(): void {
  generation++;
  interrupt();
}

/**
 * いま読んでいるセリフ (段) を飛ばして、次のセリフから続ける (2026-10-15 ユーザー要望)。
 * 従来のスキップはターンの読み上げ全体を捨てていた。
 *
 * 世代は進めない — 鎖は生きたまま、飛ばした段の残りの文だけが空回りする。再生中の文は
 * アダプタの stop() が reject するので鎖は止まらない (ライブラリは activeRequest も
 * 待機中の request も reject する = 鎖の await が必ず settle する)。
 */
export function skip(): void {
  skipThrough = Math.max(skipThrough, lastStartedPart);
  interrupt();
}

/** 再生中の音を止める (世代は触らない)。stop() と skip() の共通部。 */
function interrupt(): void {
  const dying = synth;
  const wasPlaying = dying?.isPlaying() ?? false; // stop() が畳む前に見る
  dying?.stop();
  // 合成器の生成前に停止が来た場合の保険。
  globalThis.speechSynthesis?.cancel();
  if (wasPlaying && dying?.discardOnStop) {
    synth = null;
    booting = null;
    builtWith = "";
  }
}
