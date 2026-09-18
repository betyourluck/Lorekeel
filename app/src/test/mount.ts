/**
 * マウントテストの共通準備と後始末 (spec 31)。`*.mount.test.ts` だけが使う。
 *
 * 規律 (spec 31「共通の準備と後始末」):
 * - **IPC の偽装は import より前に 1 回だけ張る** (`installOnce`、`mount.setup.ts` から呼ぶ)。
 *   `transport.ts` は**モジュールの最上段で** transport を作り `listen` を呼ぶ (アプリの生涯を通して
 *   生きる購読)。部品を import した瞬間にこれが走るので、テストの中で張ったのでは間に合わない
 *   (Phase A 初回で `transformCallback` of undefined が未処理 rejection として 6 件出た)。
 *   同じ理由で `clearMocks()` は使わない — Tauri の内部ごと消すと、import 時に登録された購読が
 *   2 本目以降のテストで壊れる。
 * - **テストごとに差し替えるのは偽装の表だけ** (`prepare`)。部品は setup / onMounted の中で
 *   command を投げるので、表は mount より前に置く (`mountWith` が順序を固定する)。
 * - **表に無い command は例外で落とす**。mockIPC の invoke は async なので throw は呼び出し側で
 *   reject になり、`void` の投げっぱなしなら vitest が未処理の rejection として落とす。
 *   **それだけでは足りない (Phase B で判明)**: 部品のローダーの多くは失敗を try/catch で握り潰す
 *   (Tauri の外でも画面を出すため) ので、偽装し忘れた command は**黙って通る**。ゆえに表に無い
 *   呼び出しを記録し、**後始末で 1 件でもあればそのテストを落とす**。意図して表に無い command を
 *   呼ぶテスト (偽装そのものの性質を見るもの) だけが `allowUnmocked()` で外す。
 * - **イベントの内部 command は既定で通る**。`shouldMockEvents: true` のとき `plugin:event|*` は
 *   mockIPC 自身がコールバックより前に処理する。ウィンドウ系 `plugin:window|*` はコールバックへ
 *   届くので、使うテストが表に書く。**既定の allowlist は置かない**。
 * - **store は本物の pinia**。テストごとに作り直し `setActivePinia` する。
 * - **メディア API はスタブ** (素の挙動は `harness.mount.test.ts` が記録する)。
 * - 後始末は `mount.setup.ts` の afterEach が**無条件**に行う。
 */
import { mockConvertFileSrc, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
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

let table: IpcTable = {};
let installed = false;
/** 表に無かった command (後始末で検める)。 */
const unmockedCalls: string[] = [];
let unmockedAllowed = false;

/** このテストでは表に無い command を呼んでも後始末で落とさない (偽装の性質を見るテスト用)。 */
export function allowUnmocked(): void {
  unmockedAllowed = true;
}
const mounted: VueWrapper[] = [];
let mediaDevicesStubbed = false;

/** IPC・ウィンドウ・asset:// の偽装を張る。テストファイルの import より前に 1 回だけ。 */
export function installOnce(): void {
  if (installed) return;
  installed = true;
  mockIPC(
    (cmd, args) => {
      const a = args as Record<string, unknown> | undefined;
      ipcCalls.push({ cmd, args: a });
      const handler = table[cmd];
      if (!handler) {
        unmockedCalls.push(cmd);
        throw new Error(unmockedMessage(cmd));
      }
      return handler(a);
    },
    { shouldMockEvents: true },
  );
  // getCurrentWindow() が引くメタデータ。これ自体は command を投げない。
  mockWindows("main");
  mockConvertFileSrc("windows");
}

/**
 * このテストの偽装の表を置き、メディアをスタブし、pinia を作って返す。
 * 部品以外 (store だけ等) を試すテストはこれを直接呼ぶ。
 */
export function prepare(ipc: IpcTable = {}): Pinia {
  installOnce();
  table = ipc;
  ipcCalls.length = 0;
  unmockedCalls.length = 0;
  unmockedAllowed = false;
  vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue(undefined);
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  if (!("mediaDevices" in navigator)) {
    Object.defineProperty(navigator, "mediaDevices", {
      configurable: true,
      value: {
        enumerateDevices: async () => [],
        getUserMedia: async () => {
          throw new Error("no microphone in tests");
        },
      },
    });
    mediaDevicesStubbed = true;
  }
  const pinia = createPinia();
  setActivePinia(pinia);
  return pinia;
}

/** 表を置いてから部品を立てる。後始末で unmount されるよう記録する。 */
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
  // 2. 偽装の表を空に戻す (空の表ではどの command も例外で落ちる)。表に無い呼び出しは
  //    ここで拾っておき、残りの後始末を全部済ませてから落とす (途中で投げると次のテストが汚れる)
  const missed = unmockedAllowed ? [] : [...new Set(unmockedCalls)];
  table = {};
  ipcCalls.length = 0;
  unmockedCalls.length = 0;
  unmockedAllowed = false;
  // 3. localStorage
  localStorage.clear();
  // 4. 偽のタイマーとスタブ (使わなかったテストでも無条件に戻す)
  vi.useRealTimers();
  vi.restoreAllMocks();
  if (mediaDevicesStubbed) {
    delete (navigator as unknown as Record<string, unknown>).mediaDevices;
    mediaDevicesStubbed = false;
  }
  if (missed.length) {
    throw new Error(
      `偽装の表に無い command が呼ばれた (ローダーが握り潰していても見逃さない): ${missed.join(", ")}`,
    );
  }
}
