/**
 * マウントテストの共通準備と後始末 (spec 31)。`*.mount.test.ts` だけが使う。
 *
 * 規律 (spec 31「共通の準備と後始末」):
 * - **偽装は mount より前**。部品は setup / onMounted の中で command を投げるので、後から偽装しても
 *   間に合わない。`mountWith` が順序を固定する。
 * - **偽装していない command は例外で落とす**。既定の handler は throw し、テストは使う command だけを
 *   表で渡す。`mockIPC` の invoke は async なので throw は呼び出し側で reject になり、`void` で投げっぱなし
 *   の呼び出しなら vitest が未処理の rejection としてテストを落とす = 偽装し忘れが黙って通らない。
 * - **イベントの内部 command は既定で通る**。`shouldMockEvents: true` のとき `plugin:event|*` は
 *   mockIPC 自身がコールバックより前に処理する (mocks.js で確認)。ウィンドウ系 `plugin:window|*` は
 *   コールバックへ届くので、使うテストが表に書く。**既定の allowlist は置かない**。
 * - **store は本物の pinia**。テストごとに作り直し `setActivePinia` する (状態をテスト間で共有しない)。
 * - **メディア API はスタブ**。happy-dom の素の挙動に頼るテストは書かない (素の挙動は
 *   `harness.mount.test.ts` が記録する)。
 * - 後始末は `mount.setup.ts` の afterEach が**無条件**に行う (使わなかったテストでも呼ぶ =
 *   呼び忘れが次のテストへ漏れない)。
 */
import { clearMocks, mockConvertFileSrc, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { mount, type ComponentMountingOptions, type VueWrapper } from "@vue/test-utils";
import { createPinia, setActivePinia, type Pinia } from "pinia";
import { vi } from "vitest";
import type { Component } from "vue";

/** テストが偽装する command の表。値は command の返り値を作る関数 (引数は invoke の args)。 */
export type IpcTable = Record<string, (args: Record<string, unknown> | undefined) => unknown>;

/** 偽装していない command の例外文。テストはこの形で落ちたことを確かめられる。 */
export function unmockedMessage(cmd: string): string {
  return `unmocked command: ${cmd}`;
}

/** 呼ばれた command を記録する (どの command が何回呼ばれたかを見たいテスト用)。 */
export const ipcCalls: { cmd: string; args: Record<string, unknown> | undefined }[] = [];

const mounted: VueWrapper[] = [];
let mediaDevicesStubbed = false;

/**
 * 準備 1〜4 を行い、作った pinia を返す。`mountWith` を使わず部品以外 (store だけ等) を
 * 試すテストはこれを直接呼ぶ。
 */
export function prepare(ipc: IpcTable = {}): Pinia {
  ipcCalls.length = 0;
  // 1. IPC。表に無い command は既定の handler で落ちる。
  mockIPC(
    (cmd, args) => {
      const a = args as Record<string, unknown> | undefined;
      ipcCalls.push({ cmd, args: a });
      const handler = ipc[cmd];
      if (!handler) throw new Error(unmockedMessage(cmd));
      return handler(a);
    },
    { shouldMockEvents: true },
  );
  // getCurrentWindow() が引くメタデータ。これ自体は command を投げない。
  mockWindows("main");
  // 2. asset:// の URL 化。
  mockConvertFileSrc("windows");
  // 3. メディア系のスタブ (restoreAllMocks で戻る)。
  vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue(undefined);
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  if (!("mediaDevices" in navigator)) {
    Object.defineProperty(navigator, "mediaDevices", {
      configurable: true,
      value: { enumerateDevices: async () => [], getUserMedia: async () => { throw new Error("no microphone in tests"); } },
    });
    mediaDevicesStubbed = true;
  }
  // 4. store。
  const pinia = createPinia();
  setActivePinia(pinia);
  return pinia;
}

/** 準備 1〜4 のあとで部品を立てる (準備 5)。後始末で unmount されるよう記録する。 */
export function mountWith<C extends Component>(
  component: C,
  options: ComponentMountingOptions<C> = {} as ComponentMountingOptions<C>,
  ipc: IpcTable = {},
): VueWrapper {
  const pinia = prepare(ipc);
  const global = { ...(options.global ?? {}), plugins: [...(options.global?.plugins ?? []), pinia] };
  const wrapper = mount(component, { ...options, global } as ComponentMountingOptions<C>) as unknown as VueWrapper;
  mounted.push(wrapper);
  return wrapper;
}

/** 後始末。`mount.setup.ts` の afterEach が無条件に呼ぶ。 */
export function teardown(): void {
  // 1. unmount (部品の onBeforeUnmount = タイマー・リスナーの解除を先に走らせる)
  while (mounted.length) mounted.pop()!.unmount();
  // 2. IPC の偽装を外す (次のテストの準備で張り直す)
  clearMocks();
  // 3. localStorage
  localStorage.clear();
  // 4. 偽のタイマーとスタブ (使わなかったテストでも無条件に戻す)
  vi.useRealTimers();
  vi.restoreAllMocks();
  if (mediaDevicesStubbed) {
    delete (navigator as unknown as Record<string, unknown>).mediaDevices;
    mediaDevicesStubbed = false;
  }
  ipcCalls.length = 0;
}
