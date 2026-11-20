/**
 * Node ESM 解析钩子：让 `node` 直接运行 src 下的 TS 模块。
 *
 * 只解决两件事：
 * 1. `@/xxx` → `<root>/src/xxx`
 * 2. 省略扩展名的相对导入 → 依次尝试 .ts / .tsx / index.ts
 *
 * 项目启用了 TypeScript 的 erasableSyntaxOnly，Node 24 的原生类型剥离即可运行，
 * 因此不需要引入 tsx / esbuild / ts-node。
 */
import { existsSync } from "node:fs";
import { dirname, extname, resolve as resolvePath } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const SRC = resolvePath(process.cwd(), "src");
const EXTENSIONS = [".ts", ".tsx", ".mts", ".js"];

function firstExisting(base) {
  if (existsSync(base) && extname(base)) return base;
  for (const ext of EXTENSIONS) {
    if (existsSync(`${base}${ext}`)) return `${base}${ext}`;
  }
  for (const ext of EXTENSIONS) {
    const indexFile = resolvePath(base, `index${ext}`);
    if (existsSync(indexFile)) return indexFile;
  }
  return null;
}

export async function resolve(specifier, context, nextResolve) {
  if (specifier.startsWith("@/")) {
    const found = firstExisting(resolvePath(SRC, specifier.slice(2)));
    if (found) return { url: pathToFileURL(found).href, shortCircuit: true };
  }

  if (specifier.startsWith(".") && context.parentURL?.startsWith("file:")) {
    const parentDir = dirname(fileURLToPath(context.parentURL));
    const found = firstExisting(resolvePath(parentDir, specifier));
    if (found) return { url: pathToFileURL(found).href, shortCircuit: true };
  }

  return nextResolve(specifier, context);
}

/**
 * `import.meta.env` 只在 Vite 下存在（mock/base.ts 用它读延迟配置）。
 * 导出内容只需要静态数据，把这里替换成空对象即可。
 */
export async function load(url, context, nextLoad) {
  const result = await nextLoad(url, context);
  if (!url.startsWith("file:") || !/\.(ts|tsx|mts)$/.test(url)) return result;

  const source = result.source ? String(result.source) : "";
  if (!source.includes("import.meta.env")) return result;

  return {
    ...result,
    source: source.replaceAll("import.meta.env", "(globalThis.__SAGE_ENV__ ?? {})"),
  };
}
