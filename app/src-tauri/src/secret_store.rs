//! API キーの保存先 (spec 34)。
//!
//! # なぜ `.env` でも localStorage でもないのか
//!
//! 2026-09-30 の実読で、鍵は **平文で 3 箇所**に居た — `app_data/.env`・WebView の localStorage
//! (`kataribe.aiModelProfiles[].apiKey`)・その写しの `app_data/settings.json`。守りは OS の
//! ユーザー権限だけだった。winget のレビューが保存と保護の説明を求め、答えを正直に書くと
//! 「平文」になるので、先に保存先を OS の資格情報ストアへ移す (Fuseforks `secret.rs` の写経)。
//!
//! # 範囲
//!
//! **保存時の平文をやめる。実行時のメモリは従来どおり** — 鍵は起動時にストアから読んで
//! プロセス env へ載せる (harness / llm_client の `from_env` は無改修)。値を UI に返さない
//! Fuseforks の形は採らない (登録モデルの突き合わせと切り替えを作り替えることになる)。
//!
//! # 書き込みの出口は 1 つ
//!
//! `.env` を書く production の経路は全部 [`persist_env`] を通す。鍵かどうかの判定は
//! [`SECRET_ENV_KEYS`] の一箇所だけ — 後から足した書き込みが鍵を `.env` に落とす経路を
//! 構造で塞ぐ (鍵を書かない呼び出しも通すのはそのため)。

use std::collections::BTreeMap;
use std::path::Path;

/// 資格情報ストアのサービス名 (= bundle identifier)。
pub const SERVICE_NAME: &str = "jp.lorekeel.app";

/// 秘密として扱う env 名。**ここに無い名前は `.env` に平文で書かれる**ので、鍵を増やしたら
/// 必ずここに足す。`JEV_ACCOUNT_ID` は秘密ではない (アカウントの識別子)。
pub const SECRET_ENV_KEYS: &[&str] = &[
    "LLM_API_KEY",
    "SUMMARY_LLM_API_KEY",
    "EDITOR_LLM_API_KEY",
    "IMAGE_API_KEY_OPENAI",
    "IMAGE_API_KEY_GEMINI",
    "JEV_API_TOKEN",
];

pub fn is_secret_env_key(key: &str) -> bool {
    SECRET_ENV_KEYS.contains(&key)
}

/// 秘密の保管先。**取得系は値を返すが、それ以外の経路 (エラー文・ログ) へ値を出さない。**
pub trait SecretStore: Send + Sync {
    /// 未登録は `Ok(None)`。
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    /// 既存の値は置き換える。
    fn set(&self, key: &str, secret: &str) -> Result<(), String>;
    /// 未登録でも成功 (冪等)。
    fn delete(&self, key: &str) -> Result<(), String>;
}

/// OS の資格情報ストア (Windows: 資格情報マネージャー / macOS: キーチェーン /
/// Linux: freedesktop Secret Service)。
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new() -> Self {
        Self { service: SERVICE_NAME.to_owned() }
    }

    /// テストで実ストアの本番エントリを汚さないためのサービス名指定。
    #[cfg(test)]
    pub fn with_service(service: &str) -> Self {
        Self { service: service.to_owned() }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(&self.service, key).map_err(|e| format!("資格情報ストアのエントリを解決できません: {e}"))
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match self.entry(key)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("資格情報ストアから読めません: {e}")),
        }
    }

    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        self.entry(key)?
            .set_password(secret)
            .map_err(|e| format!("資格情報ストアに保存できません: {e}"))
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("資格情報ストアから削除できません: {e}")),
        }
    }
}

/// 書いてから読み戻して一致を確かめる。**消す側の判断はこれが Ok のときだけ**
/// (改名の移行と同じ「コピーであって移動ではない、消すのは確認の後」)。
fn set_verified(store: &dyn SecretStore, key: &str, secret: &str) -> Result<(), String> {
    store.set(key, secret)?;
    match store.get(key)? {
        Some(v) if v == secret => Ok(()),
        _ => Err("資格情報ストアに書いた値を読み戻せません".to_string()),
    }
}

// -----------------------------------------------------------------------------
// .env ファイル
// -----------------------------------------------------------------------------

/// `.env` を書き換える。`set` は既存行の値を差し替え (無ければ末尾に追記)、`remove` は行ごと消す。
/// コメント行・他キー・順序は保つ。
pub fn rewrite_env(path: &Path, set: &[(String, String)], remove: &[&str]) -> std::io::Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for line in existing.lines() {
        let t = line.trim_start();
        if !t.starts_with('#') {
            if let Some(eq) = t.find('=') {
                let key = t[..eq].trim_end();
                if remove.contains(&key) {
                    continue;
                }
                if let Some((k, v)) = set.iter().find(|(k, _)| k.as_str() == key) {
                    out.push(format!("{k}={v}"));
                    seen.push(k.clone());
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    for (k, v) in set {
        if !seen.contains(k) {
            out.push(format!("{k}={v}"));
        }
    }
    std::fs::write(path, out.join("\n") + "\n")
}

/// `.env` の値を読む (引用符などの解釈は dotenvy に任せる)。無い・壊れた file は空。
fn read_env_values(path: &Path) -> BTreeMap<String, String> {
    let Ok(iter) = dotenvy::from_path_iter(path) else {
        return BTreeMap::new();
    };
    iter.filter_map(Result::ok).collect()
}

/// `.env` の書き込みの唯一の出口。返り値は警告 (ストアが使えず `.env` へ退避した鍵)。
///
/// - 鍵・値あり → ストアへ。成功したら `.env` のその行を消す
/// - 鍵・値が空 → ストアから消し、`.env` に `KEY=` の空行を書く。dev では main.rs が repo の
///   `.env` を先に読むので、空行が無いと「消したのに repo の鍵が生き返る」(failures #46 の
///   「GUI の保存値がユーザーの最後の明示意思」を保つ。空行は秘密ではない)
/// - 鍵以外 → そのまま `.env`
/// - ストアが失敗した鍵 → `.env` へ平文で退避 (保存そのものは落とさない。Secret Service の
///   無い Linux を想定 — 設定できないアプリより弱い保存で動くほうを採る)
pub fn persist_env(
    store: &dyn SecretStore,
    path: &Path,
    updates: &[(String, String)],
) -> std::io::Result<Vec<String>> {
    let mut set: Vec<(String, String)> = Vec::new();
    let mut remove: Vec<&str> = Vec::new();
    let mut warnings = Vec::new();
    for (k, v) in updates {
        if !is_secret_env_key(k) {
            set.push((k.clone(), v.clone()));
            continue;
        }
        if v.trim().is_empty() {
            if let Err(e) = store.delete(k) {
                warnings.push(format!("{k}: {e}"));
            }
            set.push((k.clone(), String::new()));
            continue;
        }
        match set_verified(store, k, v) {
            Ok(()) => remove.push(k.as_str()),
            Err(e) => {
                warnings.push(format!("{k} を資格情報ストアに保存できないため .env に保存しました ({e})"));
                set.push((k.clone(), v.clone()));
            }
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    rewrite_env(path, &set, &remove)?;
    Ok(warnings)
}

/// 起動時の移行: `.env` に値のある鍵行があればストアへ移す。**読み戻して一致したものだけ**
/// `.env` から消し、失敗した行は残す (次回起動で再試行)。返り値は警告。
pub fn migrate_env_secrets(store: &dyn SecretStore, path: &Path) -> Vec<String> {
    if !path.exists() {
        return Vec::new();
    }
    let values = read_env_values(path);
    let mut moved: Vec<&str> = Vec::new();
    let mut warnings = Vec::new();
    for key in SECRET_ENV_KEYS {
        let Some(v) = values.get(*key).filter(|v| !v.trim().is_empty()) else {
            continue;
        };
        match set_verified(store, key, v) {
            Ok(()) => moved.push(key),
            Err(e) => warnings.push(format!("{key} を資格情報ストアへ移せませんでした ({e})")),
        }
    }
    if !moved.is_empty() {
        if let Err(e) = rewrite_env(path, &[], &moved) {
            warnings.push(format!(".env から移行済みの鍵を消せませんでした ({e})"));
        }
    }
    warnings
}

/// ストアにある鍵だけプロセス env へ載せる。無い鍵は触らない (dev の repo `.env` の値は
/// 従来どおり生きる)。`keys` を引数にしているのは、テストが本番の env 名に触れないため。
pub fn load_secrets_into_env(store: &dyn SecretStore, keys: &[&str]) -> Vec<String> {
    let mut warnings = Vec::new();
    for key in keys {
        match store.get(key) {
            Ok(Some(v)) => std::env::set_var(key, v),
            Ok(None) => {}
            Err(e) => warnings.push(format!("{key}: {e}")),
        }
    }
    warnings
}

/// `.env` に値のある鍵行 (= ストアが使えず退避している鍵)。**真実は file から読む**
/// (状態を別に持つと file とずれる)。
pub fn fallback_keys(path: &Path) -> Vec<String> {
    let values = read_env_values(path);
    SECRET_ENV_KEYS
        .iter()
        .filter(|k| values.get(**k).is_some_and(|v| !v.trim().is_empty()))
        .map(|k| k.to_string())
        .collect()
}

// -----------------------------------------------------------------------------
// 登録モデルの鍵
// -----------------------------------------------------------------------------

/// 登録モデルの id は frontend の `newProfileId` (UUID / `p_<ms>_<rand>`) が作る。
/// ストアのエントリ名に入るので、想定外の文字は受けない。
pub fn valid_profile_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn profile_entry(id: &str) -> Result<String, String> {
    if valid_profile_id(id) {
        Ok(format!("profile:{id}"))
    } else {
        Err("登録モデルの id が不正です".to_string())
    }
}

/// 登録モデルの鍵を取り寄せる。ストアに無い id は結果に含めない。
pub fn get_profile_secrets(store: &dyn SecretStore, ids: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for id in ids {
        if let Some(v) = store.get(&profile_entry(id)?)? {
            out.insert(id.clone(), v);
        }
    }
    Ok(out)
}

/// 登録モデルの鍵を保存する (空は削除)。1 件でも失敗したら Err — 呼び出し側は
/// localStorage への退避に切り替える。
pub fn set_profile_secrets(store: &dyn SecretStore, entries: &[(String, String)]) -> Result<(), String> {
    for (id, key) in entries {
        let name = profile_entry(id)?;
        if key.trim().is_empty() {
            store.delete(&name)?;
        } else {
            set_verified(store, &name, key)?;
        }
    }
    Ok(())
}

pub fn delete_profile_secrets(store: &dyn SecretStore, ids: &[String]) -> Result<(), String> {
    for id in ids {
        store.delete(&profile_entry(id)?)?;
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// テスト
// -----------------------------------------------------------------------------

#[cfg(test)]
pub mod testing {
    use super::SecretStore;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// プロセス内に閉じた実装。`failing` なら全操作が失敗する (Secret Service の無い Linux の再現)。
    #[derive(Default)]
    pub struct InMemoryStore {
        pub entries: Mutex<HashMap<String, String>>,
        pub failing: bool,
        /// `set` は成功を返すが何も保存しない (書けたように見えて読み戻せない = 読み戻しの確認が守る場面)。
        pub drops_writes: bool,
    }

    impl InMemoryStore {
        pub fn failing() -> Self {
            Self { failing: true, ..Default::default() }
        }
        pub fn dropping() -> Self {
            Self { drops_writes: true, ..Default::default() }
        }
    }

    impl SecretStore for InMemoryStore {
        fn get(&self, key: &str) -> Result<Option<String>, String> {
            if self.failing {
                return Err("unavailable".into());
            }
            Ok(self.entries.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, secret: &str) -> Result<(), String> {
            if self.failing {
                return Err("unavailable".into());
            }
            if !self.drops_writes {
                self.entries.lock().unwrap().insert(key.into(), secret.into());
            }
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), String> {
            if self.failing {
                return Err("unavailable".into());
            }
            self.entries.lock().unwrap().remove(key);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::InMemoryStore;
    use super::*;

    fn tmp_env(name: &str, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lorekeel_secret_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(".env");
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn persist_env_routes_secrets_to_the_store_and_keeps_them_out_of_the_file() {
        let store = InMemoryStore::default();
        store.entries.lock().unwrap().insert("SUMMARY_LLM_API_KEY".into(), "old".into());
        let p = tmp_env("persist", "# comment\nLLM_API_KEY=plain\nLLM_MODEL=m0\n");
        let w = persist_env(
            &store,
            &p,
            &[
                ("LLM_API_KEY".into(), "sk-new".into()),
                ("LLM_MODEL".into(), "m1".into()),
                ("SUMMARY_LLM_API_KEY".into(), "".into()),
            ],
        )
        .unwrap();
        assert!(w.is_empty(), "{w:?}");
        let text = std::fs::read_to_string(&p).unwrap();
        // 鍵は file に残らない (旧い平文行も消える)。
        assert!(!text.contains("sk-new") && !text.contains("plain"), "{text}");
        assert!(!text.lines().any(|l| l.starts_with("LLM_API_KEY")), "{text}");
        // 鍵以外とコメントは保つ。
        assert!(text.contains("# comment") && text.contains("LLM_MODEL=m1"), "{text}");
        // 空の鍵はストアから消え、`KEY=` の空行が書かれる (dev の repo .env を隠す)。
        assert!(text.lines().any(|l| l == "SUMMARY_LLM_API_KEY="), "{text}");
        let e = store.entries.lock().unwrap();
        assert_eq!(e.get("LLM_API_KEY").map(String::as_str), Some("sk-new"));
        assert!(!e.contains_key("SUMMARY_LLM_API_KEY"));
    }

    #[test]
    fn persist_env_falls_back_to_the_file_when_the_store_fails() {
        let store = InMemoryStore::failing();
        let p = tmp_env("fallback", "");
        let w = persist_env(&store, &p, &[("IMAGE_API_KEY_OPENAI".into(), "sk-img".into())]).unwrap();
        assert_eq!(w.len(), 1, "退避したことを警告で言う: {w:?}");
        assert!(!w[0].contains("sk-img"), "警告に鍵の値を載せない: {w:?}");
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("IMAGE_API_KEY_OPENAI=sk-img"), "保存そのものは落とさない: {text}");
        assert_eq!(fallback_keys(&p), vec!["IMAGE_API_KEY_OPENAI".to_string()]);
    }

    #[test]
    fn startup_migration_moves_verified_secrets_and_leaves_the_rest() {
        // 移せる環境: 値のある鍵行はストアへ移り .env から消える。空の鍵行・鍵以外は残る。
        let store = InMemoryStore::default();
        let p = tmp_env(
            "migrate",
            "LLM_BASE_URL=https://x\nLLM_API_KEY=sk-a\nJEV_API_TOKEN=\"tok\"\nEDITOR_LLM_API_KEY=\n",
        );
        let w = migrate_env_secrets(&store, &p);
        assert!(w.is_empty(), "{w:?}");
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("sk-a") && !text.contains("tok"), "{text}");
        assert!(text.contains("LLM_BASE_URL=https://x") && text.contains("EDITOR_LLM_API_KEY="), "{text}");
        assert_eq!(store.entries.lock().unwrap().get("JEV_API_TOKEN").map(String::as_str), Some("tok"),
            "引用符は dotenvy が外す");
        assert!(fallback_keys(&p).is_empty());

        // 移せない環境: 行を消さない (消すのは読み戻しの確認の後だけ)。
        let bad = InMemoryStore::failing();
        let p2 = tmp_env("migrate_fail", "LLM_API_KEY=sk-b\n");
        let w2 = migrate_env_secrets(&bad, &p2);
        assert_eq!(w2.len(), 1);
        assert!(std::fs::read_to_string(&p2).unwrap().contains("LLM_API_KEY=sk-b"));

        // 書けたように見えて読み戻せない環境: これも消さない (鍵を失う唯一の経路を塞ぐ)。
        let lossy = InMemoryStore::dropping();
        let p3 = tmp_env("migrate_lossy", "LLM_API_KEY=sk-c
");
        assert_eq!(migrate_env_secrets(&lossy, &p3).len(), 1);
        assert!(std::fs::read_to_string(&p3).unwrap().contains("LLM_API_KEY=sk-c"));
        // 保存経路も同じ: 読み戻せなければ .env へ退避する。
        let p4 = tmp_env("persist_lossy", "");
        assert_eq!(persist_env(&lossy, &p4, &[("LLM_API_KEY".into(), "sk-d".into())]).unwrap().len(), 1);
        assert!(std::fs::read_to_string(&p4).unwrap().contains("LLM_API_KEY=sk-d"));
        assert!(set_profile_secrets(&lossy, &[("pid".into(), "k".into())]).is_err());
    }

    #[test]
    fn stored_secrets_are_loaded_into_the_process_env() {
        // 本番の env 名に触れないよう、テスト専用の名前で経路を固定する。
        let store = InMemoryStore::default();
        store.entries.lock().unwrap().insert("LOREKEEL_TEST_SECRET_A".into(), "va".into());
        std::env::set_var("LOREKEEL_TEST_SECRET_B", "from-repo-env");
        let w = load_secrets_into_env(&store, &["LOREKEEL_TEST_SECRET_A", "LOREKEEL_TEST_SECRET_B"]);
        assert!(w.is_empty());
        assert_eq!(std::env::var("LOREKEEL_TEST_SECRET_A").unwrap(), "va");
        // ストアに無い鍵は触らない (dev の repo .env の値が生きる)。
        assert_eq!(std::env::var("LOREKEEL_TEST_SECRET_B").unwrap(), "from-repo-env");
    }

    #[test]
    fn profile_secrets_round_trip_and_reject_odd_ids() {
        let store = InMemoryStore::default();
        let id = "3f2c1a9e-0b7d-4c1e-9a55-1234567890ab".to_string();
        let fb = "p_1727000000000_123456789".to_string();
        set_profile_secrets(&store, &[(id.clone(), "sk-1".into()), (fb.clone(), "sk-2".into())]).unwrap();
        let got = get_profile_secrets(&store, &[id.clone(), fb.clone(), "missing".into()]).unwrap();
        assert_eq!(got.len(), 2, "ストアに無い id は含めない");
        assert_eq!(got[&id], "sk-1");
        // 空は削除。
        set_profile_secrets(&store, &[(id.clone(), "  ".into())]).unwrap();
        assert!(!get_profile_secrets(&store, std::slice::from_ref(&id)).unwrap().contains_key(&id));
        delete_profile_secrets(&store, std::slice::from_ref(&fb)).unwrap();
        assert!(store.entries.lock().unwrap().is_empty());
        // エントリ名に入るので想定外の文字は受けない。
        assert!(set_profile_secrets(&store, &[("../x".into(), "k".into())]).is_err());
        assert!(get_profile_secrets(&store, &["a b".into()]).is_err());
        assert!(!valid_profile_id("") && !valid_profile_id(&"x".repeat(65)));
    }

    /// 実 keyring (Windows 資格情報マネージャー等) での往復。本番のサービス名を汚さないよう
    /// テスト用のサービス名を使う。`cargo test -- --ignored keyring_round_trip`
    #[test]
    #[ignore]
    fn keyring_round_trip_on_the_real_os_store() {
        let store = KeyringStore::with_service("jp.lorekeel.app.test");
        store.set("LOREKEEL_TEST", "value-1").unwrap();
        assert_eq!(store.get("LOREKEEL_TEST").unwrap().as_deref(), Some("value-1"));
        store.delete("LOREKEEL_TEST").unwrap();
        assert_eq!(store.get("LOREKEEL_TEST").unwrap(), None);
        store.delete("LOREKEEL_TEST").unwrap(); // 冪等
    }
}
