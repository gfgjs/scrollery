import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
// @ts-expect-error 构建期脚本为 .mjs 无类型声明,仅在 Node 侧运行,不进产物
import bundleBudget from './scripts/vite-plugin-bundle-budget.mjs'
// @ts-expect-error 构建期脚本为 .mjs 无类型声明,仅在 Node 侧运行,不进产物
import devCsp from './scripts/vite-plugin-dev-csp.mjs'
import themeDefaultCssPlugin from './scripts/vite-plugin-theme-default-css'

// @ts-expect-error process is a nodejs global
// @ts-expect-error process 是一个 nodejs 全局变量
const host = process.env.TAURI_DEV_HOST

// https://vite.dev/config/
export default defineConfig(async () => ({
  // 默认主题 CSS 与首帧脚本共用运行时主题生成逻辑。
  plugins: [themeDefaultCssPlugin(), vue(), bundleBudget(), devCsp()],

  build: {
    // 由 bundleBudget 校验入口体积及重依赖；懒加载块只报告体积。
    chunkSizeWarningLimit: Number.MAX_SAFE_INTEGER,
  },

  // i18n 消息以 AST 解释执行，避免 new Function，使生产 CSP 无需 unsafe-eval。
  define: {
    __INTLIFY_JIT_COMPILATION__: true,
  },

  // 保留终端中的 Rust 错误。
  clearScreen: false,
  // Tauri devUrl 使用固定端口，占用时直接报错。
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
      // 忽略 Rust 源码与构建产物，避免监视 Windows 上被占用的 DLL 引发 EBUSY。
      ignored: ['**/src-tauri/**', '**/target/**'],
    },
  },
}))
