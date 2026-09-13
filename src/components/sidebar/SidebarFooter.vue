<template>
  <!-- 设置 + 主题切换——固定在滚动区域下方。 -->
  <div class="sidebar-footer">
    <UiIconButton
      :label="$t('sidebar.settings')"
      :active="route.path === '/settings'"
      @click="toggleSettings"
    >
      <Settings :size="18" />
    </UiIconButton>
    <!-- 三态循环 亮→暗→跟随系统(P2 修复:原二态循环使 system 从此处不可达)。
         图标显示当前模式本身(Sun=亮/Moon=暗/Monitor=跟随系统),而非"将切换到"的目标。 -->
    <UiIconButton :label="$t('sidebar.toggleTheme')" @click="ui.cycleAppearance()">
      <Sun v-if="ui.appearance === 'light'" :size="18" />
      <Moon v-else-if="ui.appearance === 'dark'" :size="18" />
      <Monitor v-else :size="18" />
    </UiIconButton>
  </div>
</template>

<script setup lang="ts">
import { Settings, Sun, Moon, Monitor } from '@lucide/vue'
import { useRoute, useRouter } from 'vue-router'
import { useUiStore } from '../../stores/uiStore'
import UiIconButton from '../ui/UiIconButton.vue'

const ui = useUiStore()
const route = useRoute()
const router = useRouter()

function toggleSettings() {
  if (route.path.startsWith('/settings')) {
    // 再点关闭:回进设置前的页面(与设置页返回钮同语义;直达无历史则回首页)。
    if (window.history.state?.back) void router.back()
    else void router.replace('/')
  } else {
    void router.push('/settings')
  }
}
</script>

<style scoped>
.sidebar-footer {
  border-top: 1px solid var(--color-border-subtle);
  padding: var(--spacing-xs) var(--spacing-sm);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--spacing-sm);
  flex-shrink: 0;
}
</style>
