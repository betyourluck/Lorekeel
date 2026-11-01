/**
 * パッケージ一覧の配布サイトタブ「ブラウザで開く」(2026-11-01)。入力欄に今ある URL を
 * `open_external_url` へ渡す (「接続」しなくても開ける)。空なら既定の書庫へ倒す。
 *
 * Red の確かめ方: ボタンを外すと 1 本目が落ちる / `openSiteHome` が引数を見ず `siteUrl` を
 * 開くと 1 本目 (未接続の URL) が落ちる。
 */
import { flushPromises } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import { DEFAULT_SITE_URL } from "../stores/game";
import { ipcCalls, mountWith } from "../test/mount";
import PackageDialog from "./PackageDialog.vue";

const EMPTY_LIST = { items: [], total: 0, page: 1, per_page: 20 };

async function openSiteTab() {
  const w = mountWith(PackageDialog, {}, {
    list_packages: () => [],
    fetch_site_packages: () => EMPTY_LIST,
    open_external_url: () => null,
    check_package_updates: () => [],
  });
  await flushPromises();
  const tabBtn = w.findAll("button").find((b) => b.attributes("class")?.includes("rounded") && b.text().match(/配布サイト|Site|Archive/));
  expect(tabBtn, "配布サイトのタブボタン").toBeTruthy();
  await tabBtn!.trigger("click");
  await flushPromises();
  return w;
}

describe("パッケージ一覧 — 配布サイトをブラウザで開く", () => {
  it("入力欄の URL を (接続していなくても) 既定ブラウザで開く", async () => {
    const w = await openSiteTab();
    const input = w.find('input[placeholder="https://lorekeel.outcasts.jp"]');
    await input.setValue("  https://example.test/archive  ");
    await w.find('[data-testid="site-open-browser"]').trigger("click");
    await flushPromises();
    const call = ipcCalls.find((c) => c.cmd === "open_external_url");
    expect(call?.args).toEqual({ url: "https://example.test/archive" });
  });

  it("入力欄が空なら既定の書庫を開く", async () => {
    const w = await openSiteTab();
    await w.find('input[placeholder="https://lorekeel.outcasts.jp"]').setValue("");
    await w.find('[data-testid="site-open-browser"]').trigger("click");
    await flushPromises();
    const call = ipcCalls.find((c) => c.cmd === "open_external_url");
    expect(call?.args).toEqual({ url: DEFAULT_SITE_URL });
  });
});
