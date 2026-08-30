//! UI 設定ミラー (2026-08-31、改名時の設定消失事故の根治策)。
//!
//! WebView の localStorage は bundle identifier 別のプロファイルに住むため、identifier の
//! 変更やプロファイル破損で **ユーザーの設定 (登録モデル・パッケージ一覧・画像生成設定・
//! テーマ等) が丸ごと消える** (2026-08-28 の Lorekeel 改名で実機発生)。本モジュールは
//! `app_data_dir/settings.json` を**背後の耐久コピー**として持つ:
//!
//! - **localStorage が正本のまま** — 読み取り経路は 1 バイトも変えない (提示層の回帰リスク
//!   ゼロ)。frontend が `kataribe.*` 全キーのスナップショットを write-through で送ってくる。
//! - 起動時に localStorage が空 (新プロファイル) かつ file が在れば frontend が復元する。
//! - 書き込みは tmp→rename の原子的置換 (`harness::save_session` と同じ流儀) — 書きかけで
//!   バックアップ自体を壊さない。
//!
//! 検証は「`kataribe.` 接頭辞のキーだけ・値は文字列だけ」— localStorage の写しである以上
//! それ以外の形は壊れたデータであり、復元経路に乗せない (ゴミの復元は無より悪い)。

use std::path::Path;

/// スナップショットの上限バイト数。通常は数 KB (最大級の ComfyUI ワークフロー JSON 込みでも
/// 数百 KB) なので、これを超えるのは壊れたデータ。
pub const MAX_BYTES: usize = 4 * 1024 * 1024;

/// localStorage キーの接頭辞。改名 (2026-08-28) で意図的に据え置いた内部識別子と同じ値 —
/// ここを変えると既存ユーザーの設定が別物になるので触らない。
pub const KEY_PREFIX: &str = "kataribe.";

/// スナップショット JSON の検証。object で、全キーが `kataribe.` 接頭辞、全値が文字列。
pub fn validate(text: &str) -> Result<(), String> {
    if text.len() > MAX_BYTES {
        return Err(format!("設定スナップショットが大きすぎます ({} bytes)", text.len()));
    }
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("JSON として読めません: {e}"))?;
    let obj = v.as_object().ok_or("スナップショットは object であるべきです")?;
    for (k, val) in obj {
        if !k.starts_with(KEY_PREFIX) {
            return Err(format!("未知の接頭辞のキー: {k}"));
        }
        if !val.is_string() {
            return Err(format!("キー {k} の値が文字列ではありません"));
        }
    }
    Ok(())
}

/// 原子的置換で書く (tmp→rename)。親フォルダは初回に作る。
pub fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("設定フォルダの作成に失敗: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("一時ファイルの書き込みに失敗: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("原子的置換に失敗: {e}"))
}

/// 読んで検証を通ったときだけ返す。不在は None (正常)、壊れた file も None (復元に乗せない)
/// だが黙らず stderr へ理由を出す。
pub fn read_valid(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    match validate(&text) {
        Ok(()) => Some(text),
        Err(e) => {
            eprintln!("[settings_store] {} を無視します: {e}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 【検証則】接頭辞つき文字列値の object だけを通す — localStorage の写し以外の形は
    /// 壊れたデータとして復元経路に乗せない。
    #[test]
    fn validate_accepts_only_prefixed_string_map() {
        assert!(validate(r#"{"kataribe.theme":"dark","kataribe.fontScale":"18"}"#).is_ok());
        // 空 object も合法 (設定を何も触っていないユーザー)。
        assert!(validate("{}").is_ok());
        // object でない / 値が文字列でない / 接頭辞が違う、は全て拒否。
        assert!(validate(r#"["kataribe.theme"]"#).is_err());
        assert!(validate(r#"{"kataribe.fontScale":18}"#).is_err());
        assert!(validate(r#"{"evil.key":"x"}"#).is_err());
        assert!(validate("not json").is_err());
    }

    /// 【原子性 + 読み戻し】write_atomic は tmp を残さず、read_valid が同じ内容を返す。
    /// 壊れた file は None (黙って復元しない)。
    #[test]
    fn write_atomic_roundtrips_and_rejects_garbage() {
        let dir = std::env::temp_dir().join(format!("kataribe_settings_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");
        let text = r#"{"kataribe.theme":"light"}"#;
        write_atomic(&path, text).expect("write");
        assert_eq!(read_valid(&path).as_deref(), Some(text));
        // tmp が残っていない (原子的置換の完了)。
        assert!(!path.with_extension("json.tmp").exists());
        // 壊れた file は読み戻さない。
        std::fs::write(&path, "garbage").expect("corrupt");
        assert_eq!(read_valid(&path), None);
        // 不在も None (正常系)。
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read_valid(&path), None);
    }
}
