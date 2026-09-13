// src/composables/player/useVideoFullscreen.ts
// 视频「全屏对」:F 键让 OS 窗口全屏(useWindowMode)与查看器沉浸态(viewerStore.setImmersive)
// **成对进退**——全屏时同时隐掉 app chrome,退出时同时复原。pairActive 标志供 GD 批的 Esc 链读:
// video 且 pairActive → Esc 先退全屏对(而非直接关查看器)。
//
// 红线(计划 §裁决):不改 useWindowMode 内部——setResizable(false) 顶缘死区护栏是既有根治,
// 本 composable 只调其公开 API(toggleFullscreen/setFullscreen),不碰实现。F11 单独进入的全屏
// 不带 pairActive,故 Esc 不越权代退(边界 6)。

import { onBeforeUnmount, ref } from 'vue'
import { setFullscreen, isFullscreen } from '../useWindowMode'
import { useViewerStore } from '../../stores/viewerStore'

export function useVideoFullscreen() {
  const viewer = useViewerStore()
  /** 本播放器发起的「全屏+沉浸」对是否生效。仅经本 composable 的方法置真,F11 独立全屏不置。 */
  const pairActive = ref(false)

  // 校正:useWindowMode.setFullscreen 内部有 busy 早退守卫(转换进行中的重入调用直接空转返回,
  // 不等待、不排队)——连按 F 时后一次 await 会在前一次仍在途时立即 resolve,却仍会照常把
  // pairActive/immersive 乐观翻转,导致这两个状态与 OS 实际全屏态脱钩(根源即该早退守卫,
  // 非本 composable 可改——红线不碰 useWindowMode 实现)。故每次 await 落定后回读 isFullscreen
  // 校正 pairActive/immersive,以 OS 实际态为准。
  function reconcileWithOs(): void {
    const actual = isFullscreen.value
    if (actual !== pairActive.value) {
      pairActive.value = actual
      viewer.setImmersive(actual)
    }
  }

  async function enterPair(): Promise<void> {
    pairActive.value = true
    viewer.setImmersive(true)
    await setFullscreen(true)
    reconcileWithOs()
  }

  async function exitPair(): Promise<void> {
    pairActive.value = false
    viewer.setImmersive(false)
    await setFullscreen(false)
    reconcileWithOs()
  }

  /** F 键 / 全屏按钮:在全屏对之间切换。 */
  async function togglePair(): Promise<void> {
    if (pairActive.value) await exitPair()
    else await enterPair()
  }

  // 宿主卸载兜底(2026-07-23 裁决 J7):全屏看视频时翻页/路由离开会卸载 VideoPlayer,
  // exitPair 是唯一退出者——不在此兜底则 pairActive/immersive 状态孤儿,OS 窗口残留
  // 全屏且 Esc 链随组件消亡,只能 F11 手动退。
  // 有界重试(复核修):卸载恰逢全屏转换在途时,exitPair 内的 setFullscreen(false) 会被
  // useWindowMode 的 busy 早退守卫空转吞掉,且仍挂起的 enterPair 尾部 reconcile 可能把
  // 状态翻回——单发一次 exitPair 在该窄窗口内会复现孤儿全屏。故先退再验 OS 实际态,
  // 未落定则隔 150ms 重试(≤5 次,覆盖转换时长);退成后旧 reconcile 因 actual==pairActive
  // 均为 false 而不再翻状态。
  onBeforeUnmount(() => {
    if (!pairActive.value) return
    void (async () => {
      for (let i = 0; i < 5; i++) {
        await exitPair()
        if (!isFullscreen.value) return
        await new Promise((resolve) => setTimeout(resolve, 150))
      }
    })()
  })

  return { pairActive, isFullscreen, enterPair, exitPair, togglePair }
}
