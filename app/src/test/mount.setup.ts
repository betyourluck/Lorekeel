// mount project (spec 31) の全テストに無条件で掛かる後始末。`vite.config.ts` の setupFiles から読む。
import { afterEach } from "vitest";

import { teardown } from "./mount";

afterEach(() => {
  teardown();
});
