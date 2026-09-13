<template>
  <!-- 细分格式弹层（S 线 §7）。表面视觉留在此处，UiPopover 只负责定位与 dismiss（非同构包裹）。
       格式项是**可多选**的复选语义（维度内 OR），与文件树三态菜单的
       menuitemradio 正相反 —— 那边三态互斥，这边多选并存。 -->
  <div class="fmt">
    <div class="fmt__search">
      <Search :size="13" class="fmt__search-icon" />
      <input
        ref="searchRef"
        v-model="fmt.query.value"
        class="fmt__search-input"
        type="search"
        :placeholder="t('toolbar.formatSearchPlaceholder')"

      />
    </div>

    <div v-if="fmt.loading.value" class="fmt__hint">{{ t('common.loading') }}</div>
    <!-- 空态分两种,措辞不同:搜索无命中是「换个词」,库内无格式是「先扫描」。
         合成一句「没有格式」会让搜索无命中看起来像库空了。 -->
    <div v-else-if="fmt.groups.value.length === 0" class="fmt__hint">
      {{ fmt.query.value ? t('toolbar.formatNoMatch') : t('toolbar.formatNoneInLibrary') }}
    </div>

    <div v-else class="fmt__groups">
      <section v-for="g in fmt.groups.value" :key="g.mediaType" class="fmt__group">
        <header class="fmt__group-head">
          <span class="fmt__group-title">{{ t(GROUP_LABEL[g.mediaType]) }}</span>
          <button class="fmt__link" @click="onSelectGroup(g)">
            {{ t('toolbar.formatSelectGroup') }}
          </button>
        </header>
        <div class="fmt__opts">
          <button
            v-for="o in g.options"
            :key="o.label"
            class="fmt__opt"
            :class="{ 'fmt__opt--on': o.selected, 'fmt__opt--partial': o.partial }"
            :title="o.exts.join(', ')"
            @click="onToggle(o)"
          >
            <span class="fmt__opt-label">{{ o.label }}</span>
            <!-- availability badge（§7.1）：只是**提示**，不禁用选项 —— 未装插件的 PSD 照样
                 筛得出来，只是可能预览不了（availability ⊥ registered，§2）。 -->
            <span v-if="badgeOf(o)" class="fmt__badge" :title="t(badgeOf(o)!.title)">
              {{ t(badgeOf(o)!.short) }}
            </span>
          </button>
        </div>
      </section>
    </div>

    <div v-if="fmt.selectedCount.value > 0" class="fmt__foot">
      <button class="fmt__link" @click="clearFormats">{{ t('toolbar.formatClear') }}</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, nextTick, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Search } from '@lucide/vue'
import { useFilterStore } from '../../stores/filterStore'
import type { MediaType } from '../../types/media'
import type { Availability } from '../../types/exotic'
import { selectGroup, toggleOption, type FormatGroup, type FormatOption } from './formatFilter.helpers'
import type { useFormatFilter } from '../../composables/useFormatFilter'

// 弹层**不自持** useFormatFilter 实例:剪枝 watch 必须常驻(见 composable 注释),而弹层是
// v-if 卸载的。故由常驻的 GalleryFilterChips 持有并下传,两处共用同一份状态。
const props = defineProps<{ fmt: ReturnType<typeof useFormatFilter> }>()
const fmt = props.fmt

const filter = useFilterStore()
const { t } = useI18n()
const searchRef = ref<HTMLInputElement | null>(null)

/** 四大类的标题 i18n 键，与顶栏 chip 复用同一批 key（同一概念不该有两套措辞）。 */
const GROUP_LABEL: Record<MediaType, string> = {
  image: 'toolbar.filterImages',
  video: 'toolbar.filterVideos',
  document: 'toolbar.filterDocuments',
  audio: 'toolbar.filterAudios',
}

/**
 * 可用态 → badge。只对**需要用户动作**的态出 badge：未装 / 未授权 / 过期。
 *
 * `authorized` 不出 badge（能用就是常态，没必要标）；平台不支持 / 安装损坏等也不出 —— 那些
 * 与「能不能筛」无关，而弹层的语境是筛选，塞进去只会让用户以为筛不出来。
 */
const BADGE: Partial<Record<Availability, { short: string; title: string }>> = {
  availableUninstalled: { short: 'toolbar.fmtBadgeNotInstalled', title: 'toolbar.fmtBadgeNotInstalledHint' },
  installedUnlicensed: { short: 'toolbar.fmtBadgeUnlicensed', title: 'toolbar.fmtBadgeUnlicensedHint' },
  licenseExpired: { short: 'toolbar.fmtBadgeExpired', title: 'toolbar.fmtBadgeExpiredHint' },
}

function badgeOf(o: FormatOption): { short: string; title: string } | null {
  if (!o.pluginId) return null
  // 别名组恒同源（exotic 侧 group 恒 null → 组内只有一个 ext），故取首个即可。
  const av = fmt.availability.value.get(o.exts[0])
  return av ? (BADGE[av] ?? null) : null
}

function onToggle(o: FormatOption) {
  filter.setFileFormats(toggleOption(new Set(filter.fileFormats), o))
}
function onSelectGroup(g: FormatGroup) {
  filter.setFileFormats(selectGroup(new Set(filter.fileFormats), g))
}
function clearFormats() {
  filter.setFileFormats([])
}

onMounted(() => {
  // 开层即拉最新 facet：真相源是库，扫描一次就变，而 0ms 不值得建缓存（D-005）。
  void fmt.refresh()
  // 搜索框自动聚焦：弹层的主要交互是找格式。UiPopover 的焦点陷阱已把焦点圈在层内。
  void nextTick(() => searchRef.value?.focus())
})
</script>

<style scoped>
.fmt {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-sm);
  width: 268px;
  max-height: 60vh;
  padding: var(--spacing-sm);
}

.fmt__search {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  height: var(--control-size-default);
  padding: 0 var(--spacing-sm);
  border: 1px solid var(--color-input-border);
  border-radius: var(--radius-sm);
  background: var(--color-input-bg);
}
.fmt__search-icon {
  flex: none;
  color: var(--color-text-tertiary);
}
.fmt__search-input {
  flex: 1;
  min-width: 0;
  padding: 0;
  border: none;
  background: transparent;
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  outline: none;
}

.fmt__hint {
  padding: 12px 8px;
  color: var(--color-text-tertiary);
  font-size: var(--font-size-sm);
  text-align: center;
}

/* 组区可滚动:格式多时不撑破弹层。横向恒不滚(标签短,换行即可)。 */
.fmt__groups {
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow-y: auto;
}
.fmt__group-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  margin-bottom: 4px;
}
.fmt__group-title {
  color: var(--color-text-tertiary);
  font-size: var(--font-size-xs);
}
.fmt__link {
  padding: 0;
  border: none;
  background: none;
  color: var(--color-accent);
  font-size: var(--font-size-xs);
  cursor: pointer;
}
.fmt__link:hover {
  text-decoration: underline;
}

.fmt__opts {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}
.fmt__opt {
  display: inline-flex;
  align-items: center;
  gap: var(--spacing-xs);
  min-height: 24px;
  padding: 0 var(--spacing-sm);
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
  cursor: pointer;
  transition:
    background var(--transition-fast),
    color var(--transition-fast),
    border-color var(--transition-fast);
}
.fmt__opt:hover {
  background: var(--color-bg-hover);
}
.fmt__opt--on {
  background: var(--color-accent-subtle);
  border-color: transparent;
  color: var(--color-accent-text);
}
/* 半选:边框着色但不填底 —— 与全选(填底)一眼可分,不靠颜色深浅区分(对比度不可靠)。 */
.fmt__opt--partial {
  border-color: var(--color-accent);
  color: var(--color-accent-text);
}

.fmt__badge {
  padding: 0 var(--spacing-xs);
  border-radius: var(--radius-xs);
  background: var(--color-bg-inset);
  color: var(--color-text-tertiary);
  font-size: var(--font-size-2xs);
}
.fmt__opt--on .fmt__badge {
  background: var(--color-bg-active);
  color: var(--color-accent-text);
}

.fmt__foot {
  display: flex;
  justify-content: flex-end;
  padding-top: 4px;
  border-top: 1px solid var(--color-divider);
}
</style>
