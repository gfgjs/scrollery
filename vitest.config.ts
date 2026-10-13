import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'

// Vue 插件解析 SFC；纯函数与 SSR 渲染测试均可在 node 环境运行。
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: 'node',
    // 同时覆盖工程脚本中的纯逻辑测试。
    include: ['src/**/*.spec.ts', 'scripts/**/*.spec.mjs'],
  },
})
