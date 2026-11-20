import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vitest/config";

/**
 * 测试配置独立于 vite.config.ts：
 * - 算法与数据层测试跑在 node 环境（快、无需 DOM）
 * - 需要 DOM 的测试在文件首行加 `// @vitest-environment jsdom`
 */
export default defineConfig({
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  test: {
    include: ["tests/**/*.test.ts", "tests/**/*.test.tsx"],
    environment: "node",
    restoreMocks: true,
    reporters: ["default"],
  },
});
