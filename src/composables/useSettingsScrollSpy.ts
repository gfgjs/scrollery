// 设置页导航 scroll-spy 与程序化滚动仲裁,从 SettingsView.vue 下沉(超长文件拆分方案
// tierB-2 §SettingsView.vue ②)。返回值名与模板现有绑定逐一同名、ref 本体不解包不改名
// (§③ 最大风险红线)。settingsSections/normalizeSection/SettingsNavId 保留宿主顶层
// (template v-for 直接消费 settingsSections,不额外绕一层)。
import { ref, nextTick, watch, type Ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { resolveVisibleSection } from '../utils/settingsNav'

export interface SettingsScrollSpyDeps<TSection extends string> {
  settingsSections: ReadonlyArray<{ id: TSection; labelKey: string }>
  normalizeSection: (value: unknown) => TSection
  /** 搜索是否命中该分区(host 的 sectionMatches,依赖 settingsQuery + i18n 语料)。 */
  sectionMatches: (section: TSection) => boolean
  settingsQuery: Ref<string>
}

export function useSettingsScrollSpy<TSection extends string>(deps: SettingsScrollSpyDeps<TSection>) {
  const route = useRoute()
  const router = useRouter()

  const settingsContentRef = ref<HTMLElement | null>(null)
  const currentSection = ref<TSection>(deps.normalizeSection(route.params.section)) as Ref<TSection>

  // ── scroll-spy 与程序化滚动的仲裁（2026-07-16 真机 round10 #1）────────────────────────
  // **不变量：spy 只服务「用户自由滚动」。** currentSection 有 4 个写入方（初始化/点击/spy/路由 watch），
  // 此前无仲裁：点击已明确置位，随后的平滑滚动却让 spy 把沿途经过的每个分区逐个回填，高亮一路闪过目标才
  // 收敛——即真机报的「点击会闪一下才变选中态」。故程序化滚动全程抑制 spy，落定后放行。
  let spySuppressed = false
  let spyReleaseTimer = 0
  let releaseSpy: (() => void) | null = null

  function suppressSpyDuringScroll(root: HTMLElement) {
    // 连点：先撤上一次守卫，否则旧 scrollend 会提前给本次放行。
    releaseSpy?.()
    spySuppressed = true
    const release = () => {
      spySuppressed = false
      if (spyReleaseTimer) {
        clearTimeout(spyReleaseTimer)
        spyReleaseTimer = 0
      }
      root.removeEventListener('scrollend', release)
      releaseSpy = null
    }
    releaseSpy = release
    root.addEventListener('scrollend', release, { once: true })
    // 兜底：①目标恰是当前位置（重复点同一项 / 末分区已触底）→ 不产生 scroll，scrollend 永不来；
    // ②引擎无 scrollend。800ms > 平滑滚动典型时长。
    spyReleaseTimer = window.setTimeout(release, 800)
  }

  function scrollToSection(section: TSection, smooth = true) {
    const root = settingsContentRef.value
    const target = document.getElementById(`settings-${section}`)
    if (!root || !target) return
    suppressSpyDuringScroll(root)
    // 'instant' 而非 'auto'：容器 CSS 有 `scroll-behavior: smooth`，而 'auto' 的语义是**听 CSS 的**——
    // 于是 smooth=false 的调用方（路由 watch / 搜索跳转 / 首屏定位）过去拿到的其实仍是平滑滚动，与声明相反。
    root.scrollTo({ top: Math.max(0, target.offsetTop - 4), behavior: smooth ? 'smooth' : 'instant' })
  }

  function selectSection(section: TSection) {
    deps.settingsQuery.value = ''
    currentSection.value = section
    void router.replace(section === 'general' ? '/settings' : `/settings/${section}`)
    void nextTick(() => scrollToSection(section))
  }

  function onSettingsScroll() {
    if (deps.settingsQuery.value) return
    if (spySuppressed) return
    const root = settingsContentRef.value
    if (!root) return
    // 缺失元素跳过（不参与判据），保持与旧实现同语义；顺序即注册表顺序 = offsetTop 升序。
    const offsets = deps.settingsSections
      .map((section) => {
        const el = document.getElementById(`settings-${section.id}`)
        return el ? { id: section.id, offsetTop: el.offsetTop } : null
      })
      .filter((entry): entry is { id: TSection; offsetTop: number } => entry !== null)
    const visible = resolveVisibleSection(offsets, {
      scrollTop: root.scrollTop,
      clientHeight: root.clientHeight,
      scrollHeight: root.scrollHeight,
    })
    if (visible) currentSection.value = visible
  }

  watch(
    () => route.params.section,
    (section) => {
      const normalized = deps.normalizeSection(section)
      currentSection.value = normalized
      void nextTick(() => scrollToSection(normalized, false))
    },
  )

  watch(deps.settingsQuery, () => {
    const firstMatch = deps.settingsSections.find((section) => deps.sectionMatches(section.id))
    if (!firstMatch) return
    currentSection.value = firstMatch.id
    void nextTick(() => scrollToSection(firstMatch.id, false))
  })

  function disposeScrollSpy() {
    // 卸载时撤掉 spy 守卫，防止 800ms 兜底 timer 在组件消失后仍触发。
    releaseSpy?.()
  }

  return {
    currentSection,
    settingsContentRef,
    onSettingsScroll,
    scrollToSection,
    selectSection,
    disposeScrollSpy,
  }
}

export type SettingsScrollSpy<TSection extends string> = ReturnType<typeof useSettingsScrollSpy<TSection>>
