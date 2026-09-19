<!-- 阅读设置面板内的可折叠分组：标题行点击开合，展开状态按 id 持久化到设置（reader_panel_expanded 内联表）。
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
import { computed } from 'vue'
import { ChevronRight } from '@lucide/vue'
import { readSetting, settingsReady, writeSettings } from '../../stores/settingsPersistence'
import { parseSettingJson } from '../../composables/settingsValues'

const props = defineProps<{
  /** 展开映射里的稳定键（对应后端 reader_panel_expanded 内联表的字段）。 */
  id: string
  /** 分组标题（已 i18n）。 */
  title: string
  /** 首次（映射无该键值时）是否展开，默认展开。 */
  defaultOpen?: boolean
}>()

/** 阅读器面板分组展开映射的设置键（schema 已固化 theme/typography/paging/book 四项默认）。 */
const SETTING_KEY = 'reader_panel_expanded'

/** 读展开映射（规范 JSON 文本 → 表）；缺键或非法文本一律空表（回落 defaultOpen）。 */
function readExpanded(): Record<string, boolean> {
  return parseSettingJson<Record<string, boolean>>(readSetting(SETTING_KEY), {})
}

// 展开态：读中央快照；映射缺该键时用调用方声明的缺省。
const open = computed(() => readExpanded()[props.id] ?? props.defaultOpen ?? true)

function toggle() {
  // 权威快照未到达前读到的是空表，此刻提交会把其他分组的展开态一并抹掉；故未就绪不提交。
  if (!settingsReady.value) return
  const next = readExpanded()
  next[props.id] = !open.value
  // 写盘失败由中央服务统一提示；此处 catch 只为收掉 promise。
  writeSettings({ [SETTING_KEY]: JSON.stringify(next) }).catch(() => {})
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
