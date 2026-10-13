<template>
  <!-- 从注册表按 command.when 渲染 navigation 命令。未填充 viewerStore 的内容页暂时抑制整组。
       按钮组使用 roving tabindex，只占一个 Tab 停靠位，方向键在组内移焦。 -->
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

// /doc、/audio 尚未填充 viewerStore 时抑制工具栏，避免误显示网格命令。
// 已填充的内容页由 activeViewer 的 when 谓词决定可用命令。
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
// 图/视命令 when=kind image/video，网格与查看器各显示匹配命令。
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
