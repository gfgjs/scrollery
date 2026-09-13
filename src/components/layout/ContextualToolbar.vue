<template>
  <!-- 上下文工具栏(L4,顶栏重构)。按当前视图从注册表渲染 navigation 组命令(标题栏主图标按钮),
       by command.when 谓词过滤:网格显网格命令、图/视查看器显 viewer-image 命令(P5-2 点亮)。
       未 populate viewerStore 的内容页(/doc·/audio)整体抑制,待 P5 登记其命令册。
       按钮用 UiIconButton 原语;全组接 roving tabindex——只占一个 Tab 停靠位,方向键组内移焦。
       溢出收纳(useToolbarOverflow)待命令增多接入。 -->
  <div
    v-if="commands.length"
    ref="containerRef"
    class="ctx-toolbar"
    data-window-drag-surface
    @keydown="onKeydown"
    @focusin="onFocusin"
  >
    <UiIconButton
      v-for="(cmd, i) in commands"
      :key="cmd.id"
      :label="resolveCommandTitle(cmd)"
      :title="tooltipFor(cmd)"
      :active="activeOf(cmd)"
      :disabled="!enabledOf(cmd)"
      :tabindex="tabindexFor(i)"
      data-toolbar-item
      @click="runCmd(cmd)"
    >
      <component :is="cmd.icon" :size="18" />
    </UiIconButton>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute } from 'vue-router'
import {
  commandRegistry,
  buildCommandContext,
  resolveCommandTitle,
  formatKeybinding,
  isCommandEnabled,
  isCommandActive,
  type Command,
} from '../../commands'
import { useViewerStore } from '../../stores/viewerStore'
import { useRovingTabindex } from '../../composables/useRovingTabindex'
import UiIconButton from '../ui/UiIconButton.vue'

const route = useRoute()
const viewer = useViewerStore()

// 未 populate viewerStore 的内容路由(/doc·/audio):其专属命令待 P5 登记后由 viewerStore 驱动,
// 之前整体抑制(不误显网格命令)。/view 图/视已 populate,由 activeViewer 的 when 谓词驱动;
// P5 令 doc/audio 也 populate 后本前缀兜底可退役,纯靠 when 谓词。
const CONTENT_ROUTE_PREFIXES = ['/doc/', '/audio/']
// 设置页拥有自己的工作区工具栏；不带入撤销/重做等图库命令，避免标题栏与设置操作混在一起。
const settingsRoute = computed(() => route.path.startsWith('/settings'))
const suppressToolbar = computed(
  () =>
    settingsRoute.value ||
    (!viewer.hasActiveViewer && CONTENT_ROUTE_PREFIXES.some((p) => route.path.startsWith(p))),
)

// ctx 快照:在 computed 内构造→其读取的 selection/viewerStore 变化时响应式重算。
const ctx = computed(() => buildCommandContext())

// navigation 组命令(标题栏主按钮):by command.when 谓词按上下文过滤——网格命令 when=view'grid'、
// 图/视命令 when=kind image/video,故网格与查看器各显其命令,同组共存零冲突(P5-2)。
// 未 populate 的内容页整体抑制。
const commands = computed<Command[]>(() =>
  suppressToolbar.value ? [] : commandRegistry.query(ctx.value, { group: 'navigation' }),
)

// 谓词在模板求值:谓词内读的响应式源(historyStore.canUndo、useWindowMode.isFullscreen 等)
// 在渲染期被追踪→态变即重渲染。源是 store 还是模块级 computed 都一样追踪,不影响本机制。
function enabledOf(cmd: Command): boolean {
  return isCommandEnabled(cmd, ctx.value)
}
function activeOf(cmd: Command): boolean {
  return isCommandActive(cmd, ctx.value)
}
function runCmd(cmd: Command): void {
  commandRegistry.run(cmd.id, ctx.value)
}

// tooltip = 标题 +(有快捷键则附格式化键位)。键位同源(P5-6):与 keydown 分发共用 command.keybinding。
function tooltipFor(cmd: Command): string {
  const title = resolveCommandTitle(cmd)
  return cmd.keybinding ? `${title} (${formatKeybinding(cmd.keybinding)})` : title
}

// roving tabindex(S3):全组只占一个 Tab 停靠位,ArrowLeft/Right/Home/End 组内移焦。停靠位随命令集/
// 禁用态变化校正——rovingKey 编码 id+enabled,任一变化即触发 refresh,使 disabled 项不被当作 Tab 落点。
// 须在 commands/enabledOf 定义之后装配(remeasureKey 建 watch 时会即时求值 rovingKey → 读 commands)。
const containerRef = ref<HTMLElement | null>(null)
const rovingKey = computed(() =>
  commands.value.map((c) => `${c.id}:${enabledOf(c) ? 1 : 0}`).join('|'),
)
const { tabindexFor, onKeydown, onFocusin } = useRovingTabindex({
  containerRef,
  orientation: 'horizontal',
  remeasureKey: rovingKey,
})
</script>

<style scoped>
.ctx-toolbar {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  height: 100%;
  /* mac 红绿灯避让由 WindowChrome 的 --titlebar-controls-inset padding 处理,此处不重复 */
}
</style>
