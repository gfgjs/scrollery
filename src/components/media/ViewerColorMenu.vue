<script setup lang="ts">
// ViewerColorMenu — 大图查看器底栏「渲染色域」切换菜单(D-414 GUI 入口)。
// 三固定目标(sRGB/Display P3/DCI-P3)+ 已导入的自定义 ICC 列表,选中即写 configStore,
// 换源逻辑已由 useViewerColorSource 的 watch 自理,本组件只管选择态与 UI。
import { onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import UiIconButton from '../ui/UiIconButton.vue'
import UiPopover from '../ui/UiPopover.vue'
import { useConfigStore } from '../../stores/configStore'
import { invokeIpc } from '../../utils/ipc'
import { IPC } from '../../constants/ipc'
import { logger } from '../../utils/logger'
import { Palette } from '@lucide/vue'

interface IccProfileInfo {
  id: string
  name: string
  fileSizeBytes: number
}

const { t } = useI18n()
const config = useConfigStore()

const open = ref(false)
const btnRef = ref<InstanceType<typeof UiIconButton>>()
const anchor = ref<HTMLElement | null>(null)
const iccProfiles = ref<IccProfileInfo[]>([])

// 每次打开现取 profiles(不缓存):规避设置页增删自定义 ICC 后 stale 列表。
// 取数令牌:快速开合可致多请求并行,迟到响应丢弃;卸载时令牌递增兜底,不再回写 ref。
let fetchToken = 0
async function refreshIccProfiles(): Promise<void> {
  const my = ++fetchToken
  try {
    const list = await invokeIpc<IccProfileInfo[]>(IPC.LIST_ICC_PROFILES)
    if (my !== fetchToken) return
    iccProfiles.value = list
  } catch (e) {
    if (my !== fetchToken) return
    logger.warn('Failed to list ICC profiles for viewer color menu', { error: e })
    iccProfiles.value = []
  }
}

onBeforeUnmount(() => {
  fetchToken++
})

async function toggle(): Promise<void> {
  if (!open.value) {
    anchor.value = btnRef.value?.el ?? null
    await refreshIccProfiles()
  }
  open.value = !open.value
}

async function selectFixed(val: 'srgb' | 'display-p3' | 'dci-p3'): Promise<void> {
  await config.setViewerColorTarget(val)
  open.value = false
}

// 次序契约(configStore.ts:232 同源):自定义 ICC 须先落 customId 再切 target='custom',
// 反序会让 GET_VIEWER_COLOR_URL 以旧 customId 渲染一帧错色。
async function selectCustom(id: string): Promise<void> {
  await config.setViewerColorCustomId(id)
  await config.setViewerColorTarget('custom')
  open.value = false
}
</script>

<template>
  <div class="viewer-color-menu">
    <UiIconButton
      ref="btnRef"
      :label="t('detail.colorGamut')"
      :active="open || config.viewerColorTarget !== 'srgb'"
      @click="toggle"
    >
      <Palette :size="18" />
    </UiIconButton>
    <UiPopover v-model:open="open" :anchor="anchor" placement="top">
      <div class="viewer-color-menu__list">
        <button
          type="button"
          class="viewer-color-menu__option"
          :class="{ 'is-current': config.viewerColorTarget === 'srgb' }"
          @click="selectFixed('srgb')"
        >
          {{ t('settings.viewerColorSrgb') }}
        </button>
        <button
          type="button"
          class="viewer-color-menu__option"
          :class="{ 'is-current': config.viewerColorTarget === 'display-p3' }"
          @click="selectFixed('display-p3')"
        >
          {{ t('settings.viewerColorDisplayP3') }}
        </button>
        <button
          type="button"
          class="viewer-color-menu__option"
          :class="{ 'is-current': config.viewerColorTarget === 'dci-p3' }"
          @click="selectFixed('dci-p3')"
        >
          {{ t('settings.viewerColorDciP3') }}
        </button>
        <div class="viewer-color-menu__divider" />
        <button
          v-for="p in iccProfiles"
          :key="p.id"
          type="button"
          class="viewer-color-menu__option"
          :class="{
            'is-current': config.viewerColorTarget === 'custom' && config.viewerColorCustomId === p.id,
          }"
          @click="selectCustom(p.id)"
        >
          {{ p.name }}
        </button>
        <span v-if="iccProfiles.length === 0" class="viewer-color-menu__empty">
          {{ t('detail.colorGamutNoCustom') }}
        </span>
      </div>
    </UiPopover>
  </div>
</template>

<style scoped>
/* 弹层表面视觉(UiPopover 只管定位/dismiss):跟随底栏玻璃暗面既有语义 token,六主题自动成立。 */
.viewer-color-menu__list {
  display: flex;
  flex-direction: column;
  min-width: 168px;
  padding: var(--spacing-xs);
}
.viewer-color-menu__option {
  display: flex;
  align-items: center;
  justify-content: flex-start;
  min-height: var(--control-size-compact);
  padding: 0 var(--spacing-sm);
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  text-align: left;
  transition: background var(--transition-fast);
}
.viewer-color-menu__option:hover {
  background: var(--color-bg-hover);
}
.viewer-color-menu__option.is-current {
  background: var(--color-accent-subtle);
  color: var(--color-accent-text);
  font-weight: 500;
}
.viewer-color-menu__divider {
  height: 1px;
  margin: var(--spacing-xs) var(--spacing-sm);
  background: var(--color-divider);
}
.viewer-color-menu__empty {
  padding: var(--spacing-xs) var(--spacing-sm);
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
}
</style>
