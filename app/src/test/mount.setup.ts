// mount project (spec 31) の setupFiles。テストファイルの import より**前**に走る。
// - IPC の偽装をここで張る: transport.ts はモジュールの最上段で listen を呼ぶので、
//   テストの中で張ったのでは import の時点で Tauri の内部が無く落ちる。
// - 後始末は全テストに無条件で掛ける。
import { afterEach } from "vitest";

import { installOnce, teardown } from "./mount";

installOnce();

afterEach(() => {
  teardown();
});
