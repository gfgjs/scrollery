<!-- 阅读设置面板内的可折叠分组：标题行点击开合，展开状态按 id 持久化到 localStorage。
     刻意不复用设置页的 CollapsibleCard——那套挂在全局 useSettingsCards 协调器上（服务设置页
     的「一键全部折叠」），在此面板内使用会污染其作用域；这里用面板自身的 rs- 视觉语言 +
     局部持久化，高内聚低耦合。折叠动画沿用 grid 1fr↔0fr（DOM 常驻不卸载），与 CollapsibleCard 同法。 -->
<template>
  <section class="rsg" :class="{ 'rsg--collapsed': !open }">
    <button type="button" class="rsg__head" @click="toggle">
      <ChevronRight :size="14" class="rsg__chevron" :class="{ expanded: open }" />
      <span class="rsg__title">{{ title }}</span>
    </button>
    <div class="rsg__body">
      <div class="rsg__body-inner">
        <slot />
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { ChevronRight } from '@lucide/vue'

const props = defineProps<{
  /** 持久化展开状态的稳定键（localStorage）。 */
  id: string
  /** 分组标题（已 i18n）。 */
  title: string
  /** 首次（无持久化值时）是否展开，默认展开。 */
  defaultOpen?: boolean
}>()

const STORAGE_PREFIX = 'reader.panel.group.'

// 读初值：localStorage 优先，缺省回落 defaultOpen（默认展开）。localStorage 在个别环境
// （隐私模式 / 被禁用）会抛异常，全部 try/catch 兜底为默认值，不影响面板可用（防御式）。
function readInitial(): boolean {
  try {
    const v = localStorage.getItem(STORAGE_PREFIX + props.id)
    if (v === '0') return false
    if (v === '1') return true
  } catch {
    /* localStorage 不可用：回落默认 */
  }
  return props.defaultOpen ?? true
}

const open = ref(readInitial())

function toggle() {
  open.value = !open.value
  try {
    localStorage.setItem(STORAGE_PREFIX + props.id, open.value ? '1' : '0')
  } catch {
    /* 持久化失败无妨：本次会话内开合仍生效 */
  }
}
</script>

<style scoped>
/* 分组外壳：仅描边 + 透明底，让内部 stepper（--color-bg-elevated）在面板 surface 上仍有对比。 */
.rsg {
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  background: transparent;
  overflow: hidden;
}
.rsg__head {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  padding: 8px 10px;
  background: transparent;
  border: none;
  color: var(--color-text-primary);
  cursor: pointer;
  font-size: var(--font-size-sm);
  font-weight: 600;
  text-align: left;
}
.rsg__head:hover {
  background: var(--color-bg-hover);
}
.rsg__head:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: -2px;
}
.rsg__chevron {
  flex-shrink: 0;
  transition: transform var(--transition-normal);
}
.rsg__chevron.expanded {
  transform: rotate(90deg);
}
.rsg__title {
  flex: 1;
  min-width: 0;
}
/* 折叠动画：grid 行高 1fr↔0fr，内层裁剪溢出；DOM 常驻不卸载。 */
.rsg__body {
  display: grid;
  grid-template-rows: 1fr;
  transition: grid-template-rows var(--duration-moderate) var(--ease-in-out);
}
.rsg--collapsed .rsg__body {
  grid-template-rows: 0fr;
}
.rsg__body-inner {
  overflow: hidden;
  min-width: 0;
}
</style>
