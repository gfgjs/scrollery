// useDerivationAutoStart —— 派生流水线的「自动启动」触发器（§3.2/§3.3/§3.6）。
//
// 背景（这是「视频封面提取未正常工作」的根因修复）：
//   后端派生流水线（视频封面/关键帧、音频封面、epub 封面）已完整实现并在 Tauri 注册，
//   但此前前端没有任何触发入口，且后端无自动启动 → 流水线从未运行 → 视频封面永远不出现。
//   （对比 pdf/svg 文档缩略图走 DocThumbRenderer 自驱动那条路，故唯独它们正常。）
//
// 这里模仿 DocThumbRenderer 的自驱动思路，在合适时机「踢一脚」后端流水线。区别在于：
// 派生的全部解码/编码都在后端完成，前端只需调用 start_derivation 即可，无需自己渲染。
//   - 启动时跑一次：补全既有库的封面，无需重新扫描。
//   - 监听 db:media_enriched（扫描/导入补全、封面落地）防抖再踢：覆盖新增的视频/音频。
//
// 安全性：
//   - 后端流水线幂等（backfill = INSERT OR IGNORE）、已完成项跳过、对扫描/缩略图/用户交互让步，
//     真无待处理时为廉价空跑（仅一次索引化查询）。
//   - 踢之前先查 derivation_status，已在运行则跳过 —— 避免 start_derivation 内部的
//     cancel+restart 抖动（会把在途任务恢复为待处理、白白重来）。

import { onMounted, onBeforeUnmount } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { useTauriListen } from './useTauriListen'
import { IPC, EVENTS } from '../constants/ipc'

interface DerivationStatus {
  pending: number
  processing: number
  done: number
  error: number
  isRunning: boolean
  active: boolean
}

export function useDerivationAutoStart() {
  // 进程内重入保护：避免 kick() 自身被并发调用（与后端的 is_running 守卫互补）。
  let kicking = false

  async function kick() {
    if (kicking) return
    kicking = true
    try {
      // 已在运行则不重复启动：start_derivation 会先 cancel 现有运行再重开，
      // 重复触发只会把在途任务反复恢复为待处理，徒增开销。
      const status = await invokeIpc<DerivationStatus>(IPC.DERIVATION_STATUS).catch(() => null)
      if (status?.isRunning) return
      // 不带 kind 过滤 → 处理全部派生（视频封面/关键帧、音频封面、epub 封面）。
      // 后端自行 backfill + 续传 + 让步；封面落地后会发 db:media_enriched 让画廊刷新。
      await invokeIpc(IPC.START_DERIVATION).catch(() => {})
    } finally {
      kicking = false
    }
  }

  // 防抖：扫描/补全/封面落地会高频触发 db:media_enriched，避免每条都重入。
  // 注意：流水线自身在封面落地时也会发该事件 —— 防抖后的那次 kick 多半会落到「无待处理」空跑
  // 即返回（空跑不再发事件），故不会形成死循环。
  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  function kickDebounced() {
    if (debounceTimer) clearTimeout(debounceTimer)
    debounceTimer = setTimeout(() => {
      void kick()
    }, 1500)
  }

  // MEDIA_ENRICHED 监听解绑交由 useTauriListen(P1-11);onMounted 只留启动延迟 kick。
  useTauriListen(EVENTS.MEDIA_ENRICHED, kickDebounced)

  let kickTimer: ReturnType<typeof setTimeout> | null = null
  onMounted(() => {
    // 启动时延迟一拍再踢：让首屏扫描/缩略图先抢占（流水线本就会让步，这里只是少打一次空转）。
    kickTimer = setTimeout(() => {
      kickTimer = null
      void kick()
    }, 3000)
  })

  onBeforeUnmount(() => {
    // 补齐启动定时器清理:原实现漏清,卸载后仍会 kick 已死组件(同 P1-11 类生命周期缺口)。
    if (kickTimer) clearTimeout(kickTimer)
    if (debounceTimer) clearTimeout(debounceTimer)
  })
}
