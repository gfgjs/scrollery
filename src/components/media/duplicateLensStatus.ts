// 主画廊重复镜头状态判定（docs/designs/2026-09-02-主画廊重复项浏览方案.md §9 状态表）。
// 纯函数单源：DuplicateLensStatusBar（工具栏下第二行）与 MediaGrid 的镜头空态共用同一判定,
// 避免「状态条说已分析、空态说未分析」的双源漂移。组件层只负责把 view.key/params 解 i18n。

import type { DedupRunStatus } from '../../types/ipc'
import type { DuplicateLensModeDto } from '../../types/view'

export interface DuplicateLensStatusInput {
  /** 镜头排列模式。null = 镜头未激活（该判定此时不会被渲染,按 groups 语义取文案）。 */
  lensMode: DuplicateLensModeDto | null
  /** 后端运行态快照的 status 字段（dedupStore.status.status）。 */
  runStatus: DedupRunStatus
  /** 镜头布局是否已有内容（layoutSummary.totalItems > 0）——区分「有旧结果」与「无已发布结果」。 */
  hasLayout: boolean
  /** 已发布 separator 数（layoutSummary.separators.length）：groups=重复组数；folders=涉及重复内容的文件夹数。 */
  separatorCount: number
  /** 已发布重复位置数（layoutSummary.totalItems）。 */
  positionCount: number
  /** 分析进度百分比 0-100（itemsDone/itemsTotal，调用方取整）。 */
  percent: number
  /** 最近一次完成的本地时间戳（dedupStore.completedAt；null = 会话内未知，省略时间文案）。 */
  completedAt: number | null
  /** 可对外展示的错误标识（稳定 errorCode；不透传内部原始字符串）。 */
  safeErrorCode: string | null
}

export type DuplicateLensViewKind =
  | 'notAnalyzed'
  | 'analyzing'
  | 'updating'
  | 'analyzed'
  | 'noDuplicates'
  | 'failedWithResults'
  | 'failedNoResults'
  | 'stopped'

/** 判定结果：i18n 键 + 插值参数 + 动作语义。descKey=null 表示该状态没有次文案（不渲染）。 */
export interface DuplicateLensStatusView {
  kind: DuplicateLensViewKind
  titleKey: string
  titleParams: Record<string, number | string> | null
  descKey: string | null
  /** 主动作语义：start=开始分析 restart=重新分析 retry=重试 stop=停止分析。 */
  primaryAction: 'start' | 'restart' | 'retry' | 'stop'
  completedAt: number | null
  safeErrorCode: string | null
}

/**
 * §9 状态表 → 视图判定。判定优先级与方案表逐行对应：
 * - running/stopping 按「布局是否已有内容」分为 analyzing / updating（分析中且无/有旧结果）；
 * - completed 按组数分为 analyzed / noDuplicates；
 * - failed 按有无旧结果分为 failedWithResults / failedNoResults；
 * - cancelled（用户停止）：有旧结果 → stopped（显示上次结果 + 可重新分析），否则归入未分析；
 * - idle：无布局内容 = 从未完成分析；有布局内容（后端重启后状态归零但已发布数据仍在）以
 *   屏幕真实内容为准按 analyzed 处理，避免「画廊有组、状态条却说从未分析」的自相矛盾。
 */
export function resolveDuplicateLensStatusView(
  input: DuplicateLensStatusInput,
): DuplicateLensStatusView {
  const { lensMode, runStatus, hasLayout, separatorCount, positionCount, percent, completedAt, safeErrorCode } =
    input
  // folders 模式的 separator 是文件夹头（后端 folders 镜头只发 duplicateFolder 头,且仅覆盖
  // 含重复/疑似成员的文件夹）,「重复组」文案在该模式是口径错误——改用文件夹计数文案。
  const analyzedTitle: { titleKey: string; titleParams: Record<string, number | string> } =
    lensMode === 'folders'
      ? {
          titleKey: 'duplicatesLens.analyzedFoldersTitle',
          titleParams: { folders: separatorCount, positions: positionCount },
        }
      : {
          titleKey: 'duplicatesLens.analyzedTitle',
          titleParams: { groups: separatorCount, positions: positionCount },
        }

  if (runStatus === 'running' || runStatus === 'stopping') {
    if (hasLayout) {
      return {
        kind: 'updating',
        titleKey: 'duplicatesLens.updatingTitle',
        titleParams: { percent },
        descKey: 'duplicatesLens.updatingDesc',
        primaryAction: 'stop',
        completedAt,
        safeErrorCode,
      }
    }
    return {
      kind: 'analyzing',
      titleKey: 'duplicatesLens.analyzingTitle',
      titleParams: { percent },
      descKey: 'duplicatesLens.analyzingDesc',
      primaryAction: 'stop',
      completedAt,
      safeErrorCode,
    }
  }

  if (runStatus === 'completed') {
    if (separatorCount > 0) {
      return {
        kind: 'analyzed',
        ...analyzedTitle,
        descKey: completedAt !== null ? 'duplicatesLens.completedAt' : null,
        primaryAction: 'restart',
        completedAt,
        safeErrorCode,
      }
    }
    return {
      kind: 'noDuplicates',
      titleKey: 'duplicatesLens.noDuplicatesTitle',
      titleParams: null,
      descKey: 'duplicatesLens.noDuplicatesDesc',
      primaryAction: 'restart',
      completedAt,
      safeErrorCode,
    }
  }

  if (runStatus === 'failed') {
    if (hasLayout) {
      return {
        kind: 'failedWithResults',
        titleKey: 'duplicatesLens.failedWithResultsTitle',
        titleParams: null,
        descKey: safeErrorCode ? 'duplicatesLens.failedDetail' : null,
        primaryAction: 'retry',
        completedAt,
        safeErrorCode,
      }
    }
    return {
      kind: 'failedNoResults',
      titleKey: 'duplicatesLens.failedNoResultsTitle',
      titleParams: null,
      descKey: safeErrorCode ? 'duplicatesLens.failedDetail' : null,
      primaryAction: 'retry',
      completedAt,
      safeErrorCode,
    }
  }

  if (runStatus === 'cancelled' && hasLayout) {
    return {
      kind: 'stopped',
      titleKey: 'duplicatesLens.stoppedTitle',
      titleParams: null,
      descKey: 'duplicatesLens.stoppedDesc',
      primaryAction: 'restart',
      completedAt,
      safeErrorCode,
    }
  }

  // idle / cancelled-无结果：从未完成分析。
  if (hasLayout) {
    // 状态与内容不一致（后端状态归零而已发布数据仍在）：以屏幕内容为准。
    return {
      kind: 'analyzed',
      ...analyzedTitle,
      descKey: null,
      primaryAction: 'restart',
      completedAt,
      safeErrorCode,
    }
  }
  return {
    kind: 'notAnalyzed',
    titleKey: 'duplicatesLens.notAnalyzedTitle',
    titleParams: null,
    descKey: 'duplicatesLens.notAnalyzedDesc',
    primaryAction: 'start',
    completedAt,
    safeErrorCode,
  }
}
