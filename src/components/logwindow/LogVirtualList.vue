<script setup lang="ts">
// 虚拟滚动日志行列表(日志能力重构 S4,方案 §5/§9.4 S4)。@tanstack/vue-virtual 提供
// anchorTo:'end' + followOnAppend + measureElement 三原语,恰好覆盖「行高不一(msg 换行)+
// 实时追加 + 底部跟随/上滚退出跟随」,免自写滚动锚定状态机(方案 §5 依据)。
//
// live 模式(followBottom=true):新增行到达时若用户仍在底部,视图跟随;用户上滚后
// followOnAppend 内部自动停止跟随(库行为,非本组件手写)。history 模式(followBottom=false):
// 静态浏览,不跟随。
import { computed, ref } from 'vue'
import { useVirtualizer } from '@tanstack/vue-virtual'
import { useI18n } from 'vue-i18n'
import LogRow from './LogRow.vue'
import type { LogEntry } from '../../types/logEntry'

const props = defineProps<{
  entries: LogEntry[]
  followBottom: boolean
  selectedSeq?: number | null
}>()
const emit = defineEmits<{ select: [seq: number] }>()

const { t } = useI18n()

const scrollEl = ref<HTMLElement | null>(null)

const virtualizerOptions = computed(() => ({
  count: props.entries.length,
  getScrollElement: () => scrollEl.value,
  estimateSize: () => 22,
  overscan: 12,
  // 用 _seq(单调递增)而非 index 当 key:数组头部丢最旧行(appendLive 裁剪)后剩余行的 index
  // 全体前移一位,若拿 index 当 key,Vue 会把同一逻辑行判成"新元素"而错误重挂载/丢测量缓存。
  getItemKey: (index: number) => props.entries[index]?._seq ?? index,
  anchorTo: props.followBottom ? ('end' as const) : undefined,
  followOnAppend: props.followBottom,
}))

const virtualizer = useVirtualizer(virtualizerOptions)

const virtualItems = computed(() => virtualizer.value.getVirtualItems())
const totalSize = computed(() => virtualizer.value.getTotalSize())
const showJumpToLatest = computed(
  () => props.followBottom && props.entries.length > 0 && !virtualizer.value.isAtEnd(),
)

function measureRef(el: Element | { $el?: Element } | null) {
  if (!el) return
  const node = el instanceof Element ? el : el.$el
  if (node instanceof Element) virtualizer.value.measureElement(node)
}

function scrollToLatest() {
  virtualizer.value.scrollToEnd({ behavior: 'smooth' })
}
</script>

<template>
  <div class="log-virtual-list">
    <div ref="scrollEl" class="log-virtual-list__scroll">
      <div class="log-virtual-list__spacer" :style="{ height: `${totalSize}px` }">
        <div
          v-for="row in virtualItems"
          :key="row.key as number"
          :data-index="row.index"
          :ref="measureRef"
          class="log-virtual-list__row"
          :style="{ transform: `translateY(${row.start}px)` }"
        >
          <LogRow
            :entry="entries[row.index]"
            :selected="entries[row.index]?._seq === selectedSeq"
            @select="emit('select', $event)"
          />
        </div>
      </div>
      <div v-if="entries.length === 0" class="log-virtual-list__empty">
        {{ t('logWindow.empty') }}
      </div>
    </div>
    <button
      v-if="showJumpToLatest"
      class="log-virtual-list__jump"
      type="button"
      @click="scrollToLatest"
    >
      {{ t('logWindow.jumpToLatest') }}
    </button>
  </div>
</template>

<style scoped>
.log-virtual-list {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
}

.log-virtual-list__scroll {
  height: 100%;
  overflow-y: auto;
  overflow-x: hidden;
}

.log-virtual-list__spacer {
  position: relative;
  width: 100%;
}

.log-virtual-list__row {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
}

.log-virtual-list__empty {
  padding: var(--spacing-2xl);
  text-align: center;
  color: var(--color-text-tertiary);
  font-size: var(--font-size-sm);
}

.log-virtual-list__jump {
  position: absolute;
  bottom: var(--spacing-md);
  left: 50%;
  transform: translateX(-50%);
  min-height: var(--control-size-default);
  padding: 0 var(--spacing-md);
  border-radius: var(--radius-sm);
  border: 1px solid var(--material-recipe-float-border-color);
  background: var(--material-recipe-float-background-color);
  color: var(--color-text-secondary);
  box-shadow: var(--material-recipe-float-box-shadow);
  cursor: pointer;
  font-size: var(--font-size-xs);
}
.log-virtual-list__jump:hover {
  background: var(--color-bg-hover);
}
</style>
