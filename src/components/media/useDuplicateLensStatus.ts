// 重复镜头 §9 状态判定的响应式封装:dedupStore 运行态 + 镜头布局统计 → DuplicateLensStatusView。
// DuplicateLensStatusBar(工具栏下第二行)与 MediaGrid 的镜头空态共用,保证两处文案/动作同源。
// 独立文件的原因:duplicateLensStatus.ts 保持纯函数(单测零依赖),store 消费面收拢在此。

import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDedupStore } from '../../stores/dedupStore'
import { useDuplicateLensStore } from '../../stores/duplicateLensStore'
import { buildLayoutContentKey, useMediaStore } from '../../stores/mediaStore'
import { resolveDuplicateLensStatusView } from './duplicateLensStatus'

export function useDuplicateLensStatus() {
  const dedup = useDedupStore()
  const lens = useDuplicateLensStore()
  const media = useMediaStore()
  const { t } = useI18n()

  const view = computed(() => {
    const contentKey =
      lens.mode === null
        ? null
        : buildLayoutContentKey({
            duplicateLens: {
              mode: lens.mode,
              showUniqueItems: lens.showUniqueItems,
              orderingVersion: 1,
            },
          })
    const summary = media.layoutSemanticKey === contentKey ? media.layoutSummary : null
    return resolveDuplicateLensStatusView({
      lensMode: lens.mode,
      runStatus: dedup.status.status,
      hasLayout: (summary?.totalItems ?? 0) > 0,
      separatorCount: summary?.separators.length ?? 0,
      positionCount: summary?.totalItems ?? 0,
      // 分析进度百分比(§9「正在分析重复内容 · {percent}%」)。
      percent:
        dedup.status.itemsTotal > 0
          ? Math.round((dedup.status.itemsDone / dedup.status.itemsTotal) * 100)
          : 0,
      completedAt: dedup.completedAt,
      // 稳定错误标识回落快照内错误码;不透传内部原始字符串。
      safeErrorCode: dedup.status.errors[0]?.code ?? null,
    })
  })

  /** 主文案(titleKey 按 params 有无分别解 i18n)。 */
  const title = computed(() =>
    view.value.titleParams ? t(view.value.titleKey, view.value.titleParams) : t(view.value.titleKey),
  )

  /** 次文案:analyzed 带本地完成时刻;failed 带稳定错误码;无次文案的状态返回空串。 */
  const desc = computed(() => {
    if (view.value.descKey === null) return ''
    if (view.value.kind === 'analyzed' && view.value.completedAt !== null) {
      return t(view.value.descKey, {
        time: new Date(view.value.completedAt).toLocaleString(),
      })
    }
    if (view.value.kind === 'failedWithResults' || view.value.kind === 'failedNoResults') {
      return t(view.value.descKey, { code: view.value.safeErrorCode })
    }
    return t(view.value.descKey)
  })

  /** 主动作标签(开始分析/重新分析/重试/停止分析)。 */
  const primaryLabel = computed(() => {
    switch (view.value.primaryAction) {
      case 'start':
        return t('duplicatesLens.start')
      case 'restart':
        return t('duplicatesLens.restart')
      case 'retry':
        return t('duplicatesLens.retry')
      case 'stop':
        return t('duplicatesLens.stop')
      default:
        return ''
    }
  })

  /** 主动作分派:重新分析清旧摘要重跑(reset=true),开始/重试续跑(reset=false),停止即停。 */
  function runPrimaryAction() {
    if (view.value.primaryAction === 'stop') {
      void dedup.stop().catch(() => {})
      return
    }
    void dedup.start(view.value.primaryAction === 'restart').catch(() => {
      // 失败经状态判定呈现(failed* 文案),动作处不再弹错。
    })
  }

  return { view, title, desc, primaryLabel, runPrimaryAction }
}
