// GalleryViewControls 镜头态 contract 测试(2026-09-02 方案 §8.1 browse-only)。
//
// 覆盖:镜头激活时「导出当前视图」整项不渲染——导出走 buildCurrentViewDescriptor 的普通画廊
// 语义,与镜头画面不是同一集合,静默导出会漂移(F-005 gate 清单的批量命令阻断点);同时镜头
// 排列分段控件在位。普通画廊态两者相反。
//
// 环境:SSR renderToString + 真实 i18n + 真实 duplicateLensStore(镜头接线是被测面)。
// uiStore 真实现初始化即碰 window.matchMedia(node 下炸,同 useGalleryQuerySync.spec 的旁路
// 姿态),aiStore/exportStore/useExportEntries 仅给渲染所需最小 fake,避免拖入 IPC 链;
// mock '../../router' 应用路由单例(duplicateLensStore 动作要读 currentRoute)。

import { describe, it, expect, vi } from 'vitest'
import { createSSRApp, h, type Component } from 'vue'
import { renderToString } from '@vue/server-renderer'
import { createI18n } from 'vue-i18n'
import { createPinia } from 'pinia'
import zhCN from '../../i18n/locales/zh-CN'

vi.mock('../../router', () => ({
  default: {
    isReady: () => Promise.resolve(),
    get currentRoute() {
      return { value: { fullPath: '/', path: '/', query: {} } }
    },
    push: () => Promise.resolve(),
    replace: () => Promise.resolve(),
  },
}))

vi.mock('../../stores/uiStore', async () => {
  const { reactive: r } = await import('vue')
  const ui = r({
    groupBy: 'date',
    sortWithinGroup: 'datetime',
    sortOrder: 'desc',
    layoutMode: 'justified',
    gridRowHeight: 256,
    seamlessGroups: false,
    setGroupBy() {},
    setSortWithinGroup() {},
    setLayoutMode() {},
    setGridRowHeight() {},
    setSeamlessGroups() {},
  })
  return { useUiStore: () => ui }
})

vi.mock('../../stores/aiStore', async () => {
  const { reactive: r } = await import('vue')
  const ai = r({ isSemanticMode: false })
  return { useAiStore: () => ai }
})

vi.mock('../../stores/exportStore', () => ({
  useExportStore: () => ({ openExportDialog: () => {} }),
}))

vi.mock('../../composables/useExportEntries', () => ({
  useExportEntries: () => ({
    buildCurrentViewExportPayload: () => ({ selection: {}, source: 'current-view' }),
  }),
}))

import GalleryViewControls from './GalleryViewControls.vue'
import { useDuplicateLensStore } from '../../stores/duplicateLensStore'

const i18n = createI18n({ legacy: false, locale: 'zh-CN', messages: { 'zh-CN': zhCN } })
const t = i18n.global.t

async function render(withLens: boolean): Promise<string> {
  const app = createSSRApp({
    setup() {
      if (withLens) {
        // 组件 setup 只读 lens.mode 渲染分支,动作语义(URL 写侧)由 duplicateLensStore.spec 锁定,
        // 这里直写 store 模拟「URL→store 同步已收敛出镜头态」。
        useDuplicateLensStore().$patch({ mode: 'groups' })
      }
      return () =>
        h(GalleryViewControls as Component, {
          variant: 'inline',
          visibleCount: 99,
          measuring: false,
          baseIndex: 0,
        })
    },
  })
  app.use(i18n)
  app.use(createPinia())
  return renderToString(app)
}

describe('GalleryViewControls:镜头态导出阻断(§8.1 批量命令)', () => {
  it('普通画廊:导出当前视图按钮在位', async () => {
    expect(await render(false)).toContain(`title="${t('toolbar.exportView')}"`)
  })

  it('镜头激活:导出当前视图整项不渲染,镜头排列分段控件在位', async () => {
    const html = await render(true)
    expect(html).not.toContain(`title="${t('toolbar.exportView')}"`)
    expect(html).toContain(t('toolbar.lensModeGroups'))
  })
})
