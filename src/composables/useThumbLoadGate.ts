// src/composables/useThumbLoadGate.ts
// 快滚时的缩略图加载闸门：单一模块级信号供 DOM 缩略图和 Canvas 管线共读。
//   MediaThumb / MediaThumbCompact(经 useThumbLoader) + MediaGridCanvas(getImage)。
//
// 由 MediaGrid.onGridScroll 按滚动速度置位(shouldDeferThumbLoad):DOM 卡片在快速飞掠期间
// 抑制**新**缩略图启动；Canvas 可见区仍以有界并发加载，视口外预取收缩为仅前向，
// 避免稍快速滚动时格子入屏才起载造成换图波。
//
// 为何用模块级单例而非 props 穿透:DOM 模式加载信号需穿 MediaGrid→MediaGridRow→卡片→
// useThumbLoader 四层,给数千张 compact 卡传响应式 prop、每次翻转触发全量 re-render 本身
// 就是一次 hitch。单例信号让「真正发起加载的 loader」各自 watch,零 prop 穿透、零卡片
// re-render(镜像本项目 useSelection.setPointerIdResolver 的模块单例惯例)。桌面单窗口
// SPA,无 SSR 跨请求串态之虞。
//
// DOM 闸门只抑制加载 **启动**:已缓存/已解码的图照常绘制;被抑制的加载不丢失,放行时由
// loader 的 watch 补起。Canvas 可见区不读此闸门;预取调度读取它决定预取窗形态(开闸全窗/
// 关闸仅前向),放行后恢复全窗续跑。

import { ref, readonly, type DeepReadonly, type Ref } from 'vue'

/// 是否推迟缩略图加载启动(true = 飞掠中,抑制)。
const deferThumbLoad = ref(false)
/// 只读句柄模块级快照:loader 每张卡调一次 useThumbLoadGate(),复用同一 readonly 代理,
/// 免去每卡新建代理的分配(极密网格数千卡)。
const deferThumbLoadRO = readonly(deferThumbLoad)

/**
 * 置位加载闸门(由 MediaGrid.onGridScroll 调用)。仅在值变化时写,避免无谓触发 watch。
 */
export function setDeferThumbLoad(v: boolean): void {
  if (deferThumbLoad.value !== v) deferThumbLoad.value = v
}

/**
 * 只读响应式句柄——供 loader 内部 `watch` 监听放行时机(闸门 true→false 即补起被抑制的加载)。
 */
export function useThumbLoadGate(): DeepReadonly<Ref<boolean>> {
  return deferThumbLoadRO
}

/**
 * 非响应式即时读——供 canvas getImage 等命令式路径在绘制时判断是否跳过发起新加载。
 */
export function isThumbLoadDeferred(): boolean {
  return deferThumbLoad.value
}
