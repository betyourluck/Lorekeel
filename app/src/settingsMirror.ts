/**
 * UI 設定ミラー (2026-08-31、改名時の設定消失事故の根治策)。
 *
 * localStorage は WebView プロファイル (bundle identifier 別) に住むため、identifier の
 * 変更やプロファイル破損で設定が丸ごと消える (2026-08-28 の Lorekeel 改名で実機発生)。
 * 本モジュールは `kataribe.*` 全キーを backend の `app_data/settings.json` へ
 * write-through で写し、起動時に localStorage が空 (新プロファイル) なら file から復元する。
 *
 * - **localStorage が正本のまま** — 読み取り経路 (~50 箇所の同期読み) は 1 バイトも変えない。
 * - 書き込みの捕捉は `Storage.prototype` のフック 1 箇所 — 書き込み箇所を列挙しない
 *   (列挙は必ず漏れる = failures #84 の「列挙 1 本化」と同じ向き)。
 * - 復元後は reload 1 回で全モジュールの import 時読みに値が行き渡る (災害復旧の道でだけ
 *   払うコスト。通常起動では reload しない)。
 */
import { invoke } from "@tauri-apps/api/core";

/** localStorage キーの接頭辞 (改名で意図的に据え置いた内部識別子。backend の検証と同じ値)。 */
export const SETTINGS_PREFIX = "kataribe.";

/**
 * 復元→reload の一度きりガード (sessionStorage はタブ内 reload を生き残る)。
 * localStorage への書き込みが黙って失敗する環境で reload が無限ループしないための保険。
 */
const RESTORE_GUARD = "kataribe.mirror.restored";

/** debounce の待ち時間 ms。短いほど「最後の変更を落とす窓」が狭い。 */
const SAVE_DELAY_MS = 800;

/** Storage 互換の最小面 (テストで fake を差すため)。 */
export interface StorageLike {
  readonly length: number;
  key(i: number): string | null;
  getItem(k: string): string | null;
  setItem(k: string, v: string): void;
  removeItem(k: string): void;
}

/** `kataribe.*` キーだけを写したスナップショットを作る。 */
export function collectSnapshot(storage: StorageLike): Record<string, string> {
  const snap: Record<string, string> = {};
  for (let i = 0; i < storage.length; i++) {
    const k = storage.key(i);
    if (!k || !k.startsWith(SETTINGS_PREFIX)) continue;
    const v = storage.getItem(k);
    if (v !== null) snap[k] = v;
  }
  return snap;
}

/**
 * スナップショット JSON を検証つきで読む (backend `settings_store::validate` と同じ規則 —
 * object・全キー接頭辞つき・全値文字列)。壊れは null = 復元経路に乗せない。
 */
export function parseSnapshot(text: string): Record<string, string> | null {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof v !== "object" || v === null || Array.isArray(v)) return null;
  const obj = v as Record<string, unknown>;
  for (const [k, val] of Object.entries(obj)) {
    if (!k.startsWith(SETTINGS_PREFIX) || typeof val !== "string") return null;
  }
  return obj as Record<string, string>;
}

/**
 * 復元すべきか: 手元に `kataribe.*` が 1 つも無く (新プロファイル)、file 側に 1 つ以上
 * あるとき**だけ**。手元に 1 つでもあれば localStorage が生きている = そちらが正。
 */
export function shouldRestore(
  storage: StorageLike,
  snap: Record<string, string> | null,
): boolean {
  if (!snap || Object.keys(snap).length === 0) return false;
  return Object.keys(collectSnapshot(storage)).length === 0;
}

/** スナップショットを storage へ書く (復元)。 */
export function applySnapshot(storage: StorageLike, snap: Record<string, string>): void {
  for (const [k, v] of Object.entries(snap)) storage.setItem(k, v);
}

/**
 * debounce 付きミラー。`onChange(key)` を書き込みフックから呼ぶと、待ち時間後に
 * スナップショット全量を save へ渡す (全量 dump なので removeItem も自然に収束する)。
 * schedule/cancel は注入可 (テストで fake timer を使うため)。
 */
export function createMirror(
  storage: StorageLike,
  save: (json: string) => void,
  delayMs: number = SAVE_DELAY_MS,
  schedule: (fn: () => void, ms: number) => unknown = (fn, ms) => setTimeout(fn, ms),
  cancel: (h: unknown) => void = (h) => clearTimeout(h as ReturnType<typeof setTimeout>),
): { onChange(key: string): void; flush(): void; saveNow(): void } {
  let timer: unknown = null;
  const fire = () => {
    timer = null;
    save(JSON.stringify(collectSnapshot(storage)));
  };
  return {
    onChange(key: string) {
      if (!key.startsWith(SETTINGS_PREFIX)) return;
      if (timer !== null) cancel(timer);
      timer = schedule(fire, delayMs);
    },
    /** 待ち中の保存を今すぐ発火する (beforeunload 用)。待ち無しなら何もしない。 */
    flush() {
      if (timer === null) return;
      cancel(timer);
      fire();
    },
    /** 無条件で今すぐ保存する (起動時の初回同期用)。 */
    saveNow: fire,
  };
}

/**
 * 起動時に呼ぶ (main.ts、mount の前)。復元したら true を返す — 呼び出し側は mount せず
 * reload を待つ。Tauri 外 (素の vite dev) や backend 失敗ではミラー無しで false。
 */
export async function initSettingsMirror(): Promise<boolean> {
  let fileText: string | null;
  try {
    fileText = await invoke<string | null>("load_ui_settings");
  } catch {
    return false; // Tauri IPC が無い環境 — ミラー無しの通常起動。
  }
  // 災害復旧: 新プロファイル (localStorage 空) + file 在り → 復元して reload 1 回。
  try {
    const snap = fileText === null ? null : parseSnapshot(fileText);
    if (snap && shouldRestore(localStorage, snap) && !sessionStorage.getItem(RESTORE_GUARD)) {
      applySnapshot(localStorage, snap);
      sessionStorage.setItem(RESTORE_GUARD, "1");
      location.reload();
      return true;
    }
  } catch {
    /* storage が塞がれた環境では復元しない (通常起動へ) */
  }
  // write-through ミラーを張る。フックは Storage.prototype の 1 箇所 —
  // localStorage インスタンスへの代入は Storage の named setter に化けるので使えない。
  const mirror = createMirror(localStorage, (json) => {
    invoke("save_ui_settings", { json }).catch((e) =>
      console.warn("[settingsMirror] 設定バックアップの保存に失敗:", e),
    );
  });
  const origSet = Storage.prototype.setItem;
  Storage.prototype.setItem = function (this: Storage, k: string, v: string) {
    origSet.call(this, k, v);
    if (this === window.localStorage) mirror.onChange(k);
  };
  const origRemove = Storage.prototype.removeItem;
  Storage.prototype.removeItem = function (this: Storage, k: string) {
    origRemove.call(this, k);
    if (this === window.localStorage) mirror.onChange(k);
  };
  // 起動時に 1 回同期 — 既存ユーザーの file を「最初の設定変更」を待たずに作る。
  mirror.saveNow();
  // 終了間際の変更を落とさない (debounce 待ち中なら今すぐ書く)。
  window.addEventListener("beforeunload", () => mirror.flush());
  return false;
}
