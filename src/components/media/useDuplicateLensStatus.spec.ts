import { beforeEach, describe, expect, it, vi } from 'vitest'
import { reactive } from 'vue'
import type { LayoutSummary } from '../../types/layout'

const dedup = reactive({
  status: {
    status: 'completed',
    itemsTotal: 0,
    itemsDone: 0,
    errors: [] as Array<{ code: string }>,
  },
  completedAt: null as number | null,
  start: async () => {},
  stop: async () => {},
})
const lens = reactive({
  mode: 'groups' as 'groups' | 'folders' | null,
  showUniqueItems: false,
})
const media = reactive({
  layoutSemanticKey: 'normal' as string | null,
  layoutSummary: null as LayoutSummary | null,
})

vi.mock('vue-i18n', async (importOriginal) => ({
  ...(await importOriginal<typeof import('vue-i18n')>()),
  useI18n: () => ({ t: (key: string) => key }),
}))
vi.mock('../../stores/dedupStore', () => ({ useDedupStore: () => dedup }))
vi.mock('../../stores/duplicateLensStore', () => ({ useDuplicateLensStore: () => lens }))
vi.mock('../../stores/mediaStore', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../stores/mediaStore')>()),
  useMediaStore: () => media,
}))

import { useDuplicateLensStatus } from './useDuplicateLensStatus'
import { buildLayoutContentKey } from '../../stores/mediaStore'

function summary(): LayoutSummary {
  return {
    totalRows: 3,
    totalHeight: 600,
    layoutVersion: 9,
    totalItems: 6,
    separators: [
      { groupId: 'a', label: 'a', count: 3, y: 0, epochDay: null },
      { groupId: 'b', label: 'b', count: 3, y: 200, epochDay: null },
    ],
    monthBuckets: [],
  }
}

describe('useDuplicateLensStatus: 布局语义匹配', () => {
  beforeEach(() => {
    dedup.status.status = 'completed'
    lens.mode = 'groups'
    media.layoutSemanticKey = 'normal'
    media.layoutSummary = summary()
  })

  it('忽略普通画廊或其他镜头的摘要，只统计当前镜头布局', () => {
    const { view } = useDuplicateLensStatus()
    expect(view.value.kind).toBe('noDuplicates')

    media.layoutSemanticKey = buildLayoutContentKey({
      duplicateLens: { mode: 'groups', showUniqueItems: false, orderingVersion: 1 },
    })
    expect(view.value.kind).toBe('analyzed')
    expect(view.value.titleParams).toEqual({ groups: 2, positions: 6 })

    lens.mode = 'folders'
    expect(view.value.kind).toBe('noDuplicates')
  })
})
