// 设置页自定义 ICC 导入/枚举/删除(方案 B §0④,SettingsView 特例行「viewerIccManager」),从
// SettingsView.vue 下沉(超长文件拆分方案 tierB-2 §SettingsView.vue ②)。返回值名与模板
// 现有绑定逐一同名、ref 本体不解包不改名(§③ 最大风险红线)。
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { invokeIpc, ipcErrorMessage } from '../utils/ipc'
import { logger } from '../utils/logger'
import { useToastStore } from '../stores/toastStore'
import { useConfigStore } from '../stores/configStore'
import { IPC } from '../constants/ipc'

interface IccProfileInfo {
  id: string
  name: string
  fileSizeBytes: number
}

// ICC 导入错误按后端稳定码(error.rs:178-179)分档,各自专属文案;未知码回退既有通用文案。
const ICC_IMPORT_ERROR_KEY_BY_CODE: Record<string, string> = {
  icc_too_large: 'settings.viewerIccImportFailedTooLarge',
  icc_not_rgb: 'settings.viewerIccImportFailedNotRgb',
  icc_not_display_class: 'settings.viewerIccImportFailedNotDisplayClass',
  icc_transform_unsupported: 'settings.viewerIccImportFailedTransformUnsupported',
  icc_parse_failed: 'settings.viewerIccImportFailedParseFailed',
}

export function useIccProfileManager() {
  const toast = useToastStore()
  const config = useConfigStore()
  const { t } = useI18n()

  const iccProfiles = ref<IccProfileInfo[]>([])

  async function refreshIccProfiles() {
    try {
      iccProfiles.value = await invokeIpc<IccProfileInfo[]>(IPC.LIST_ICC_PROFILES)
    } catch (e) {
      logger.warn('Failed to list ICC profiles', { error: e })
    }
  }

  async function importIccProfile() {
    let info: IccProfileInfo
    try {
      const selected = await openDialog({
        multiple: false,
        title: t('settings.viewerIccImportBtn'),
        filters: [{ name: 'ICC', extensions: ['icc', 'icm'] }],
      })
      if (!selected || typeof selected !== 'string') return
      info = await invokeIpc<IccProfileInfo>(IPC.IMPORT_ICC_PROFILE, { filePath: selected })
    } catch (e) {
      const code = (e as { code?: string } | null)?.code
      const key = code
        ? (ICC_IMPORT_ERROR_KEY_BY_CODE[code] ?? 'settings.viewerIccImportFailedCode')
        : 'settings.viewerIccImportFailed'
      toast.addToast('error', t(key, { error: ipcErrorMessage(e) }))
      return
    }
    await refreshIccProfiles()
    try {
      // 导入后即选用(减少「导入了却忘记切档位」的多一步操作)。
      await config.setViewerColorCustomId(info.id)
      await config.setViewerColorTarget('custom')
      toast.addToast('success', t('settings.viewerIccImportSuccess', { name: info.name }))
    } catch (e) {
      // 导入本身已成功(profile 已入库+已刷新列表),这里失败的只是「设为当前」这一步,
      // 不得复用「导入失败」文案误导用户重新导入。
      toast.addToast(
        'error',
        t('settings.viewerIccSetActiveFailed', { name: info.name, error: ipcErrorMessage(e) }),
      )
    }
  }

  async function selectIccProfile(id: string) {
    await config.setViewerColorCustomId(id)
    await config.setViewerColorTarget('custom')
  }

  async function deleteIccProfile(id: string) {
    try {
      await invokeIpc(IPC.DELETE_ICC_PROFILE, { profileId: id })
      await refreshIccProfiles()
      // 删除当前选中项时后端会复位 viewer_color_target=srgb 并清 custom_id(方案 B §0④);
      // 前端只需从后端重取一遍配置,不在此臆测是否恰是当前选中项。
      await config.refreshFromBackend()
    } catch (e) {
      toast.addToast('error', t('settings.viewerIccDeleteFailed', { error: ipcErrorMessage(e) }))
    }
  }

  return { iccProfiles, refreshIccProfiles, importIccProfile, selectIccProfile, deleteIccProfile }
}

export type IccProfileManager = ReturnType<typeof useIccProfileManager>
