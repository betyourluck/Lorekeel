import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import "./assets/main.css";
import { applyTheme } from "./theme";
import { initSettingsMirror } from "./settingsMirror";

// テーマ (ライト/ダーク) を mount 前に <html> へ反映する = 描画前に確定させ、
// 保存済みライトモードで開いてもダークが一瞬映るフラッシュを防ぐ。既定ダーク。
// (災害復旧 = 設定ミラーからの復元パスでは localStorage がまだ空なので既定ダークで
//  一瞬立ち上がり、直後の reload で保存済みテーマに揃う — 復旧の道でだけ払うコスト。)
applyTheme();

// 配布 (production) ビルドでは WebView の既定右クリックメニューを抑止する。
// 既定メニューには「最新の情報に更新」「名前を付けて保存」「印刷」などブラウザ由来の項目が
// 乗っており、配布アプリでは触らせない。ただし**テキスト入力欄と選択中テキストの上では
// ネイティブメニューを残す**(コピー/貼り付けの導線)。dev では抑止しない (右クリック→検証 =
// 開発者ツールを使うため)。Vite が dev/production を import.meta.env.DEV で判別する。
if (!import.meta.env.DEV) {
  window.addEventListener("contextmenu", (e) => {
    const t = e.target instanceof HTMLElement ? e.target : null;
    const editable = t?.closest("input, textarea, [contenteditable]");
    const hasSelection = !!window.getSelection()?.toString();
    if (!editable && !hasSelection) e.preventDefault();
  });
  // 右クリックの「更新」を潰しても F5 / Ctrl+R (WebView2 のブラウザアクセラレータ) が
  // 同じ再読み込みを起こすので、同時に塞ぐ (再読み込みはゲームセッションの表示を吹き飛ばす)。
  window.addEventListener("keydown", (e) => {
    if (e.key === "F5" || ((e.ctrlKey || e.metaKey) && (e.key === "r" || e.key === "R"))) {
      e.preventDefault();
    }
  });
}

// 設定ミラー (localStorage → app_data/settings.json の write-through + 新プロファイルへの
// 復元) を張ってから mount する。復元したときは reload が来るので mount しない。
// invoke 1 往復ぶん mount が遅れるが、ローカル IPC なので体感には乗らない。
initSettingsMirror().then((restored) => {
  if (restored) return;
  createApp(App).use(createPinia()).mount("#app");
});
