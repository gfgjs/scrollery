// 图/视查看器专属命令(顶栏重构 P5-2)。ContentViewer 经 defineExpose + viewerStore.populate 暴露
// ViewerApi,本册命令经 ctx.activeViewer.api 调用具体动作;when 谓词按 activeViewer.kind 过滤
// (image/video——ContentViewer 承载二者)。navigation 只保留返回、跨项导航与沉浸；缩放、旋转、
// 信息属内容区连续操作，归入 view 组且以 ContentViewer 底部控制条作为唯一可见主入口。
// keybinding 为元数据(供 P5-6 键位同源:tooltip 与实际监听共用同一常量);当前实际按键仍由
// ContentViewer.onKeydown 处理,P5-6 收敛为注册表分发后此元数据成为唯一来源。

import { markRaw } from 'vue'
import {
  ArrowLeft,
  ChevronLeft,
  ChevronRight,
  ZoomIn,
  ZoomOut,
  Maximize,
  RotateCw,
  Info,
  Maximize2,
  PencilLine,
} from '@lucide/vue'
import type { Command, CommandContext } from '../types'
import i18n from '../../i18n'

const t = (k: string) => i18n.global.t(k)

/** 图/视共用可见判据:仅当活动查看器是图或视(ContentViewer 承载二者)。 */
function isImageOrVideo(ctx: CommandContext): boolean {
  const k = ctx.activeViewer?.kind
  return k === 'image' || k === 'video'
}

/** 图片简单编辑(方案 C):仅图像,不含视频(kind === 'video' 即被排除)。 */
function isImage(ctx: CommandContext): boolean {
  return ctx.activeViewer?.kind === 'image'
}

export const viewerImageCommands: Command[] = [
  {
    id: 'viewer.close',
    title: () => t('detail.back'),
    icon: markRaw(ArrowLeft),
    group: 'navigation',
    order: 0,
    keybinding: 'Escape',
    when: isImageOrVideo,
    run: (ctx) => ctx.activeViewer?.api?.close?.(),
  },
  {
    id: 'viewer.prev',
    title: () => t('detail.prev'),
    icon: markRaw(ChevronLeft),
    group: 'navigation',
    order: 10,
    keybinding: 'ArrowLeft',
    // 仅图像:video 上下文 ←/→ 被 viewer-video.ts 夺走作 seek(video.prevItem 走 shift+←,见
    // task_plan「快捷键」裁决),此处收窄防双绑。
    when: isImage,
    run: (ctx) => ctx.activeViewer?.api?.prev?.(),
  },
  {
    id: 'viewer.next',
    title: () => t('detail.next'),
    icon: markRaw(ChevronRight),
    group: 'navigation',
    order: 20,
    keybinding: 'ArrowRight',
    when: isImage,
    run: (ctx) => ctx.activeViewer?.api?.next?.(),
  },
  {
    id: 'viewer.zoomOut',
    title: () => t('detail.zoomOut'),
    icon: markRaw(ZoomOut),
    group: 'view',
    order: 30,
    keybinding: '-',
    when: isImageOrVideo,
    run: (ctx) => ctx.activeViewer?.api?.zoomOut?.(),
  },
  {
    id: 'viewer.zoomIn',
    title: () => t('detail.zoomIn'),
    icon: markRaw(ZoomIn),
    group: 'view',
    order: 40,
    keybinding: '+',
    when: isImageOrVideo,
    run: (ctx) => ctx.activeViewer?.api?.zoomIn?.(),
  },
  {
    id: 'viewer.cycleZoomMode',
    title: () => t('detail.zoomMode'),
    icon: markRaw(Maximize),
    group: 'view',
    order: 50,
    when: isImageOrVideo,
    run: (ctx) => ctx.activeViewer?.api?.cycleZoomMode?.(),
  },
  {
    id: 'viewer.rotate',
    title: () => t('detail.rotate'),
    icon: markRaw(RotateCw),
    group: 'view',
    order: 55,
    when: isImageOrVideo,
    run: (ctx) => ctx.activeViewer?.api?.rotate?.(),
  },
  {
    id: 'viewer.edit',
    title: () => t('edit.entry'),
    icon: markRaw(PencilLine),
    group: 'view',
    order: 57,
    when: isImage,
    run: (ctx) => ctx.activeViewer?.api?.edit?.(),
  },
  {
    id: 'viewer.toggleInfo',
    title: () => t('detail.info'),
    icon: markRaw(Info),
    group: 'view',
    order: 60,
    keybinding: 'i',
    when: isImageOrVideo,
    run: (ctx) => ctx.activeViewer?.api?.toggleInfo?.(),
  },
  {
    id: 'viewer.toggleImmersive',
    title: () => t('detail.immersive'),
    icon: markRaw(Maximize2),
    group: 'navigation',
    order: 70,
    when: isImageOrVideo,
    isActive: (ctx) => ctx.activeViewer?.immersive ?? false,
    run: (ctx) => ctx.activeViewer?.api?.toggleImmersive?.(),
  },
]
