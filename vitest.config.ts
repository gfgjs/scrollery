import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'

// Part5 T4a 回归基线:选区策略层为纯函数,无需 DOM → 默认 node 环境即可。
// S1 起新增 UI primitive(.vue)的 contract 测试:用 @vue/server-renderer 的 renderToString 做 SSR
// 渲染断言(项目未装 @vue/test-utils),故需 @vitejs/plugin-vue 解析 SFC;仍用 node 环境(SSR 不触 DOM)。
// vue() 只转换 .vue,现有纯 .ts 测试不受影响。
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: 'node',
    // scripts/ 亦纳入:S7 的打包预算门(vite-plugin-bundle-budget)把纯判定层与 Vite 解耦,
    // 好让「门能红」由单测双向钉死而非靠人手工改数字试。CI 已跑 npm test,故无需另加步骤。
    include: ['src/**/*.spec.ts', 'scripts/**/*.spec.mjs'],
  },
})
