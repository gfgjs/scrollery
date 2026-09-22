import { sectionSettingKeys, type SettingKey } from './settingsMap'

export type SettingsNavId = 'common' | 'appearance' | 'gallery' | 'media' | 'ai' | 'storage' | 'advanced'

export const SETTINGS_SECTIONS: ReadonlyArray<{ id: SettingsNavId; labelKey: string }> = [
  { id: 'common', labelKey: 'settings.sectionCommon' },
  { id: 'appearance', labelKey: 'settings.sectionAppearance' },
  { id: 'gallery', labelKey: 'settings.sectionGallery' },
  { id: 'media', labelKey: 'settings.sectionMedia' },
  { id: 'ai', labelKey: 'settings.sectionAi' },
  { id: 'storage', labelKey: 'settings.sectionStorage' },
  { id: 'advanced', labelKey: 'settings.sectionAdvanced' },
]

/** 展示与搜索共用分组归属；配置键和控件绑定仍由原注册表管理。 */
export interface SettingsGroup {
  id: string
  section: SettingsNavId
  titleKey: string
  keys: readonly SettingKey[]
  defaultOpen?: boolean
  desktopOnly?: boolean
}

export const SETTINGS_GROUPS: readonly SettingsGroup[] = [
  { id: 'common', section: 'common', titleKey: 'settings.applicationGroup', keys: ['language', 'closeBehavior'] },
  { id: 'general', section: 'appearance', titleKey: 'settings.themeGroup', keys: ['theme', 'uiFontSize'] },
  {
    id: 'layout', section: 'appearance', titleKey: 'settings.layoutGroup',
    keys: ['titlebarMerged', 'toolbarAlign', 'selectionBarDocked', 'selectionBarAlign', 'autoHideChromeWindowed'],
  },
  { id: 'galleryBehavior', section: 'gallery', titleKey: 'settings.galleryInteractionGroup', keys: ['hoverScale', 'hoverAutoplay'] },
  { id: 'viewerColor', section: 'gallery', titleKey: 'settings.viewerColorGroup', keys: ['viewerColorTarget', 'viewerIccManager'], desktopOnly: true },
  {
    id: 'galleryTimeline', section: 'gallery', titleKey: 'settings.timelineGroup', defaultOpen: false,
    keys: ['timelineAxisWidth', 'timelineScrollWidth', 'scrollThumbMinHeight', 'axisViewportOpacity'],
  },
  { id: 'thumbnailDisplay', section: 'media', titleKey: 'settings.thumbnailDisplayGroup', keys: ['showDragHandle', 'minimapRenderMode', 'showThumbInfo'] },
  {
    id: 'thumbnails', section: 'media', titleKey: 'settings.thumbnailGenerationGroup',
    keys: ['thumbDecodeStrategy', 'gpuEngine', 'thumbSize', 'thumbWebpQuality', 'thumbSkipMaxKb', 'fullThumbGen'],
  },
  { id: 'thumbnailCache', section: 'media', titleKey: 'settings.thumbnailCacheGroup', keys: ['thumbCacheDir', 'thumbCacheMaxMb', 'cacheStats'] },
  { id: 'video', section: 'media', titleKey: 'settings.video', keys: sectionSettingKeys('video') },
  { id: 'aiModels', section: 'ai', titleKey: 'settings.aiModels', keys: sectionSettingKeys('aiModels') },
  { id: 'debug', section: 'advanced', titleKey: 'sidebar.debugSettings', keys: sectionSettingKeys('debug') },
  { id: 'danger', section: 'advanced', titleKey: 'settings.dangerZone', keys: sectionSettingKeys('danger'), defaultOpen: false },
  { id: 'reading', section: 'gallery', titleKey: 'settings.reading', keys: [] },
  { id: 'modelLibrary', section: 'ai', titleKey: 'settings.mlTitle', keys: [] },
  { id: 'faceModels', section: 'ai', titleKey: 'settings.fmTitle', keys: [] },
  { id: 'ocrModels', section: 'ai', titleKey: 'settings.ocrTitle', keys: [] },
  { id: 'enhanceModels', section: 'ai', titleKey: 'settings.enhanceTitle', keys: [] },
  { id: 'backup', section: 'storage', titleKey: 'backup.sectionTitle', keys: [] },
  { id: 'rootVisibility', section: 'storage', titleKey: 'settings.rootVisTitle', keys: [] },
  { id: 'networkStorage', section: 'storage', titleKey: 'settings.nsTitle', keys: [] },
  { id: 'knownVolumes', section: 'storage', titleKey: 'settings.volTitle', keys: [] },
]
