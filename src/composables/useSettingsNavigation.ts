import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { SETTINGS_SECTIONS, type SettingsNavId } from '../constants/settingsLayout'

const DEFAULT_SECTION: SettingsNavId = 'appearance'

// 路由卸载设置页后仍保留本次应用会话的最后位置。
let lastPosition: { section: SettingsNavId; scrollTop: number } = {
  section: DEFAULT_SECTION,
  scrollTop: 0,
}

function normalizeSection(value: unknown): SettingsNavId {
  if (value === 'general') return 'appearance'
  return SETTINGS_SECTIONS.some((section) => section.id === value)
    ? (value as SettingsNavId)
    : DEFAULT_SECTION
}

/** 协调设置分类与路由，并在重新进入时恢复上次离开的位置。 */
export function useSettingsNavigation() {
  const route = useRoute()
  const router = useRouter()
  const previousPosition = lastPosition
  const currentSection = ref<SettingsNavId>(
    normalizeSection(route.params.section || previousPosition.section),
  )
  const settingsContent = ref<HTMLElement | null>(null)
  // 首次访问才创建分类内容；离开分类后保留组件，避免丢失表单和任务进度。
  const visitedSections = ref<SettingsNavId[]>([currentSection.value])
  watch(currentSection, (section) => {
    if (!visitedSections.value.includes(section)) {
      visitedSections.value = [...visitedSections.value, section]
    }
  }, { flush: 'sync' })

  function revealCurrentSection() {
    // 窄屏分类横向滚动；恢复位置或搜索跨分类后仍需露出当前分类。
    document
      .querySelector<HTMLElement>('.settings-nav__item.active')
      ?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  }

  function selectSection(section: SettingsNavId) {
    currentSection.value = section
    void router.replace(section === DEFAULT_SECTION ? '/settings' : `/settings/${section}`)
  }

  watch(
    () => route.params.section,
    (section) => {
      // 离开设置时路由参数先被清空，不能因此把待保存的分类改回外观。
      if (route.path !== '/settings' && !route.path.startsWith('/settings/')) return
      currentSection.value = normalizeSection(section)
    },
  )
  watch(
    currentSection,
    () => {
      if (settingsContent.value) settingsContent.value.scrollTop = 0
      revealCurrentSection()
    },
    { flush: 'post' },
  )

  onMounted(() => {
    if (settingsContent.value) {
      settingsContent.value.scrollTop =
        currentSection.value === previousPosition.section ? previousPosition.scrollTop : 0
    }
    revealCurrentSection()
    // 通用入口恢复分类后同步 URL；replace 保持返回应用只需一次后退。
    if (!route.params.section && currentSection.value !== DEFAULT_SECTION) {
      void router.replace(`/settings/${currentSection.value}`)
    }
  })

  onBeforeUnmount(() => {
    lastPosition = {
      section: currentSection.value,
      scrollTop: settingsContent.value?.scrollTop ?? 0,
    }
  })

  return { currentSection, visitedSections, settingsContent, selectSection }
}
