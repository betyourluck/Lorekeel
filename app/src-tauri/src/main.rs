// リリースで Windows の余分なコンソール窓を出さない (DO NOT REMOVE)。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // .env はアプリ入口で読む (lib は環境を書き換えない = failures #13 の規律)。
    // dotenvy は cwd から親へ遡って .env を探すのでリポジトリ root の .env を拾う。
    dotenvy::dotenv().ok();
    // 改名 (Kataribe → Lorekeel) の設定引き継ぎ。**WebView が起動する前**でなければ
    // ならない (起動後は LevelDB を掴んでいる) ので、Builder より手前のここで呼ぶ。
    lorekeel_desktop_lib::migrate_webview_storage();
    lorekeel_desktop_lib::run()
}
