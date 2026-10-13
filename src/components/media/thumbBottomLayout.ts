/** DOM 与 Canvas 共用的底部行尺寸，悬停前预留完整操作槽以免文字跳位。 */
export interface ThumbBottomLayout {
  actionHeight: number
  statusBottom: number
  durationBottom: number
  ratingBottom: number
  favoriteBottom: number
  infoBottom: number
}

export const THUMB_INFO_LINE_HEIGHT = 12
export const THUMB_INFO_BADGE_HEIGHT = 16
export const THUMB_INFO_FADE_HEIGHT = 24

/** 极密格容纳不了操作与状态行时，沿用 compact 的操作省略，避免角标越过格边。 */
export function thumbActionsFit(
  width: number,
  height: number,
  hasStatus: boolean,
  splitStatus = false,
): boolean {
  return (
    width >= 28 && height >= (width < 112 ? 48 : 24) + (hasStatus ? (splitStatus ? 40 : 20) : 0) + 6
  )
}

/** 计算底部布局；Canvas 可传入复用对象，避免逐格创建临时结果。 */
export function computeThumbBottomLayout(
  width: number,
  browseOnly: boolean,
  compact: boolean,
  hasStatus: boolean,
  result: ThumbBottomLayout = {
    actionHeight: 0,
    statusBottom: 4,
    durationBottom: 4,
    ratingBottom: 8,
    favoriteBottom: 4,
    infoBottom: 6,
  },
  splitStatus = false,
): ThumbBottomLayout {
  const splitActions = width < 112
  result.actionHeight = compact || browseOnly ? 0 : splitActions ? 48 : 24
  result.ratingBottom = splitActions ? 32 : 8
  result.favoriteBottom = 4
  result.statusBottom = result.actionHeight + 4
  result.durationBottom = result.statusBottom + (splitStatus ? 20 : 0)
  result.infoBottom = result.actionHeight + (hasStatus ? (splitStatus ? 40 : 20) : 0) + 6
  return result
}

/** 保留顶部渐隐区与徽章行，信息行只能占用剩余的完整行高。 */
export function thumbInfoLineLimit(height: number, bottom: number, hasBadges: boolean): number {
  return Math.max(
    0,
    Math.floor(
      (height - bottom - THUMB_INFO_FADE_HEIGHT - (hasBadges ? THUMB_INFO_BADGE_HEIGHT : 0)) /
        THUMB_INFO_LINE_HEIGHT,
    ),
  )
}
