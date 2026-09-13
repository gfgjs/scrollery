// src/stores/filterStore.ts
// 媒体过滤器状态（驱动 compute_layout 重新运行）

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { MediaType } from '../types/media'

export const useFilterStore = defineStore('filter', () => {
  // 这是图库与文件树共同的媒体分类状态。空数组保持既有 URL/API 语义：不限制四类。
  const mediaTypes = ref<MediaType[]>([]) // 空 = 全部
  /**
   * 细分格式（S 线 D-011）：**规范化小写扩展名**，空 = 该维度不限。
   *
   * 存具体扩展名而非 UI 的 `group`（JPEG={jpg,jpeg} / RAW={cr2,…}）：`file_format` 是文件的
   * 客观属性，`group` 只是显示概念 —— 不落库、不进 API、不进 URL。展开由弹层在 UI 层完成。
   */
  const fileFormats = ref<string[]>([])
  const livePhotoOnly = ref(false)
  const favoritedOnly = ref(false)
  const minRating = ref(0)
  const colorLabel = ref(0) // 0=不按色筛选 / 1-7 色档（T16）
  const dateFrom = ref<number | null>(null)
  const dateTo = ref<number | null>(null)

  const hasActiveFilters = computed(
    () =>
      mediaTypes.value.length > 0 ||
      fileFormats.value.length > 0 ||
      livePhotoOnly.value ||
      favoritedOnly.value ||
      minRating.value > 0 ||
      colorLabel.value > 0 ||
      // 日期范围需 from/to 两者皆备才真正下发谓词（见 toApiFilter），故"激活态"也以两者皆备为准，
      // 避免只填一端时 chip 高亮/出现「清除筛选」却实际不筛选的错觉。
      (dateFrom.value !== null && dateTo.value !== null),
  )

  function setMediaTypes(types: MediaType[]) {
    mediaTypes.value = types
  }

  function toggleMediaType(type: MediaType) {
    const idx = mediaTypes.value.indexOf(type)
    if (idx >= 0) {
      mediaTypes.value = mediaTypes.value.filter((t) => t !== type)
    } else {
      mediaTypes.value = [...mediaTypes.value, type]
    }
  }

  function setFileFormats(formats: string[]) {
    fileFormats.value = formats
  }

  function toggleFileFormat(ext: string) {
    fileFormats.value = fileFormats.value.includes(ext)
      ? fileFormats.value.filter((f) => f !== ext)
      : [...fileFormats.value, ext]
  }

  function clearFilters() {
    mediaTypes.value = []
    fileFormats.value = []
    livePhotoOnly.value = false
    favoritedOnly.value = false
    minRating.value = 0
    colorLabel.value = 0
    dateFrom.value = null
    dateTo.value = null
  }

  function toApiFilter() {
    return {
      mediaTypes: mediaTypes.value.length ? mediaTypes.value : undefined,
      fileFormats: fileFormats.value.length ? fileFormats.value : undefined,
      livePhotoOnly: livePhotoOnly.value || undefined,
      favoritedOnly: favoritedOnly.value || undefined,
      minRating: minRating.value > 0 ? minRating.value : undefined,
      colorLabel: colorLabel.value > 0 ? colorLabel.value : undefined,
      dateRange:
        dateFrom.value && dateTo.value ? { from: dateFrom.value, to: dateTo.value } : undefined,
    }
  }

  /**
   * `toApiFilter()` 的稳定序列化 —— 供 `useJustifiedLayout` 当重算 watch 的唯一筛选源，
   * 取代原先手工枚举各筛选字段的写法（枚举漏项不报错，`colorLabel` 已经漏过一次，T16 遗留）。
   * 用 JSON 而非对象是因为 `watch` 按 `Object.is` 比对非 deep 源，`toApiFilter()` 每次返回
   * 新对象会恒不等；序列化后按值比对，键序由字面量顺序固定，稳定可比。
   * 有意行为变化：只填日期一端时不再触发重算（此前会触发一次空转）。
   */
  const apiFilterKey = computed(() => JSON.stringify(toApiFilter()))

  return {
    mediaTypes,
    fileFormats,
    livePhotoOnly,
    favoritedOnly,
    minRating,
    colorLabel,
    dateFrom,
    dateTo,
    hasActiveFilters,
    apiFilterKey,
    setMediaTypes,
    toggleMediaType,
    setFileFormats,
    toggleFileFormat,
    clearFilters,
    toApiFilter,
  }
})
