import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
// @ts-expect-error 构建期脚本为 .mjs 无类型声明,仅在 Node 侧运行,不进产物
import bundleBudget from './scripts/vite-plugin-bundle-budget.mjs'
// @ts-expect-error 构建期脚本为 .mjs 无类型声明,仅在 Node 侧运行,不进产物
import devCsp from './scripts/vite-plugin-dev-csp.mjs'

// @ts-expect-error process is a nodejs global
// @ts-expect-error process 是一个 nodejs 全局变量
const host = process.env.TAURI_DEV_HOST

// https://vite.dev/config/
// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [vue(), bundleBudget(), devCsp()],

  build: {
    // 关掉 Rollup 的无差别体积告警,由 bundleBudget 插件的两条**可行动**不变量取代
    // (入口块预算 + 重依赖不得进入口)。不是「调高阈值让它闭嘴」而是**换掉机制**:
    // Rollup 那条是告警不是门(实测把 pdfjs 静态 import 进首屏它照喊照过 exit=0),
    // 且喊的两个块全是误报(cpp 是懒加载语法、index 是路由全懒加载后的外壳)。
    // 完整实测与取舍见 scripts/vite-plugin-bundle-budget.mjs 顶部注释。
    chunkSizeWarningLimit: Number.MAX_SAFE_INTEGER,
  },

  // vue-i18n JIT 编译开关（P1-23,CSP 加固前置)。
  // vue-i18n v9 默认把 locale 消息(含 {name} 等插值)在运行时用 `new Function` 编译成可执行
  // 函数——本质等于 eval,故此前 CSP 必须放行 `'unsafe-eval'`。开启 JIT 后,消息改为编译成 AST
  // 并在运行时**解释执行**,不再走 `new Function`,从而允许 CSP 删除 `'unsafe-eval'`(仍保留
  // `'wasm-unsafe-eval'` 供 wasm 解码器)。v9 默认 false,须显式置 true;仅翻转编译策略,不改
  // legacy/install 等其余行为(那些 flag 保持 vue-i18n 内建默认)。
  // define: JIT compilation for vue-i18n so message compiling no longer needs `new Function` (eval),
  // which lets the production CSP drop `'unsafe-eval'`.
  define: {
    __INTLIFY_JIT_COMPILATION__: true,
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  // 专为 Tauri 开发量身定制的 Vite 选项，仅在 `tauri dev` 或 `tauri build` 中应用
  //
  // 1. prevent Vite from obscuring rust errors
  // 1. 防止 Vite 掩盖 rust 错误
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  // 2. Tauri 期望一个固定端口，如果该端口不可用则失败
  server: {
    port: 1420,
    strictPort: true,
    // 桌面 WebView 与 Node 对 localhost 的 IPv4/IPv6 解析顺序可能不同；固定回环 IPv4，
    // 与 tauri.conf.json 的 devUrl 保持同一地址，避免服务实际只监听 ::1 而 WebView 访问 127.0.0.1。
    host: host || '127.0.0.1',
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri` and the Rust build output.
      //    workspace 迁移后 target 到了**仓库根**（非 src-tauri/target），故必须显式忽略 `**/target/**`；
      //    否则 cargo 编译时 Vite 试图 watch 被占用的 `target/debug/deps/*.dll` → Windows EBUSY 崩溃。
      // 3. 告诉 Vite 忽略监视 `src-tauri` 及 Rust 编译产物目录。
      ignored: ['**/src-tauri/**', '**/target/**'],
    },
  },
}))
