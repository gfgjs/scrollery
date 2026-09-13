<template>
  <!-- 鼠标移出整组 → 取消 hover 预览，回落到 modelValue。 -->
  <div
    class="star-rating"
    :class="{ 'star-rating--readonly': readonly, 'star-rating--rating': tone === 'rating' }"
    @mouseleave="hoverValue = 0"
  >
    <button
      v-for="n in max"
      :key="n"
      type="button"
      class="star-rating__star"
      :class="{ filled: n <= displayValue }"
      :disabled="readonly"

      @mouseenter="onHover(n)"
      @click="onClick(n)"
    >
      <Star :size="size" :fill="n <= displayValue ? 'currentColor' : 'none'" :stroke-width="1.5" />
    </button>
  </div>
</template>

<script setup lang="ts">
// 可复用星级控件：既用于评分录入（点星打分、点当前值清零），也用于"≥N 星"筛选。
// 单一职责、纯展示+交互，不内嵌任何 IPC/store——副作用由父层经 v-model / change 处理。
import { ref, computed } from 'vue'
import { Star } from '@lucide/vue'

const props = withDefaults(
  defineProps<{
    /** 当前评分（0 = 未评分 / 不筛选）。 */
    modelValue: number
    /** 星级上限。 */
    max?: number
    /** 图标尺寸（px）。 */
    size?: number
    /** 只读：仅展示，不可交互（如缩略图角标）。 */
    readonly?: boolean
    /** 允许"点当前值清零"（评分清空 / 筛选取消"≥N"）。默认开。 */
    allowClear?: boolean
    /** 评分语义色只在媒体深底使用;普通工具栏/筛选控件沿用主题 accent。 */
    tone?: 'accent' | 'rating'
  }>(),
  { max: 5, size: 18, readonly: false, allowClear: true, tone: 'accent' },
)

const emit = defineEmits<{
  'update:modelValue': [value: number]
  /** 用户显式改动（与 v-model 同值，便于父层做副作用如批量评分）。 */
  change: [value: number]
}>()

// hover 预览值（0 = 无 hover）；展示时优先 hover，否则取 modelValue。
const hoverValue = ref(0)
const displayValue = computed(() => hoverValue.value || props.modelValue)

function onHover(n: number) {
  if (props.readonly) return
  hoverValue.value = n
}

function onClick(n: number) {
  if (props.readonly) return
  // 点当前值 → 清零（toggle-off）；否则设为 n。
  const next = props.allowClear && n === props.modelValue ? 0 : n
  emit('update:modelValue', next)
  emit('change', next)
}
</script>

<style scoped>
.star-rating {
  display: inline-flex;
  gap: 2px;
}
.star-rating__star {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  background: transparent;
  border: none;
  /* 空星取边框灰、填充星取琥珀色，与详情页既有评分视觉一致。 */
  color: var(--color-border);
  cursor: pointer;
  transition: color var(--transition-fast);
}
.star-rating__star.filled {
  color: var(--color-accent-text);
}
.star-rating:not(.star-rating--readonly) .star-rating__star:hover {
  color: var(--color-accent-hover);
}
.star-rating--rating .star-rating__star.filled {
  color: var(--color-rating-amber);
}
.star-rating--rating:not(.star-rating--readonly) .star-rating__star:hover {
  color: var(--color-rating-amber);
}
.star-rating--readonly .star-rating__star {
  cursor: default;
}
</style>
