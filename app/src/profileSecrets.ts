/**
 * 登録モデルの API キーを OS の資格情報ストアと往復させる (spec 34)。
 *
 * 純関数の側は `aiProfiles.ts` (テストはそちら)。ここは invoke を持つので Tauri の外では失敗し、
 * そのときは**鍵を捨てない側**へ倒れる (取り寄せに失敗 = 手元の値のまま / 保存に失敗 = 鍵ごと退避)。
 */
import { invoke } from "@tauri-apps/api/core";
import {
  loadAiProfiles,
  mergeProfileSecrets,
  profilesWithStoredKeys,
  saveAiProfiles,
  type AiModelProfile,
} from "./aiProfiles";

/** 設定ダイアログを開いたとき: ストアから鍵を取り寄せてメモリ上の登録へ重ねる。 */
export async function hydrateProfiles(list: AiModelProfile[]): Promise<AiModelProfile[]> {
  if (list.length === 0) return list;
  try {
    const secrets = await invoke<Record<string, string>>("get_profile_secrets", { ids: list.map((p) => p.id) });
    return mergeProfileSecrets(list, secrets);
  } catch {
    return list;
  }
}

/** 登録の保存。鍵はストアへ、localStorage には鍵を抜いて書く。ストアが使えなければ
 *  **鍵ごと localStorage へ退避**して `"fallback"` を返す (呼び出し側が状態行で告げる)。 */
export async function persistProfiles(list: AiModelProfile[]): Promise<"store" | "fallback"> {
  try {
    await invoke("set_profile_secrets", { entries: list.map((p) => ({ id: p.id, key: p.apiKey.trim() })) });
    saveAiProfiles(list);
    return "store";
  } catch {
    saveAiProfiles(list, { keepKeys: true });
    return "fallback";
  }
}

/** 登録を消したとき: その鍵もストアから消す (失敗しても登録の削除は止めない)。 */
export async function deleteProfileSecret(id: string): Promise<void> {
  try {
    await invoke("delete_profile_secrets", { ids: [id] });
  } catch {
    /* ストアが使えない = もともとそこに鍵が無い */
  }
}

/** 起動時の移行: localStorage に鍵が残っている登録をストアへ移し、成功したら鍵を抜いて書き直す。
 *  **設定ダイアログを開くまで待たない** — 開かないユーザーの settings.json (設定ミラー) に平文が残り続けるため。
 *  書き直しは設定ミラーの write-through を起こすので、settings.json も鍵の無い内容で上書きされる。 */
export async function migrateLegacyProfileKeys(): Promise<void> {
  const list = loadAiProfiles();
  const keyed = profilesWithStoredKeys(list);
  if (keyed.length === 0) return;
  try {
    await invoke("set_profile_secrets", { entries: keyed.map((p) => ({ id: p.id, key: p.apiKey.trim() })) });
  } catch {
    return; // ストアが使えない環境 = 退避のまま (次回起動で再試行)
  }
  saveAiProfiles(list);
}
