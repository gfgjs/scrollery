// src/composables/reader/useReaderBookPrefs.ts
// 每书设置（编码/重排/简繁/主题，结构拆分自 DocumentViewer.vue §S12 + §S32 + load() 中每书 prefs
// 解析段）。remount 判据 + remount 前位置捕获是红线（§3 风险 2），本文件只调用 root 注入的
// requestRemount(true)，不自写 capture+bump。
import { ref, type Ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { IPC } from '../../constants/ipc'
import { invokeIpc, ipcErrorMessage } from '../../utils/ipc'
import { useToastStore } from '../../stores/toastStore'
import type { ReaderBookPrefs, ZhConvertConfig } from '../../types/reader'

export interface UseReaderBookPrefsDeps {
  id: Ref<number>
  /** 仅有的 capture+bump 实现在 root（红线：不可在此另写一份）。 */
  requestRemount: (captureFirst: boolean) => void
}

export function useReaderBookPrefs(deps: UseReaderBookPrefsDeps) {
  // 每书设置（R2-6c，txt）：编码覆盖（'' = 自动）+ 二级重排。随文档加载从 reader_book_prefs 读取。
  const bookEncoding = ref('')
  const bookReflow = ref(false)
  // 简繁转换（R4）：每书档位（'' = 关）。txt/md/epub 通用（纯显示层）。随文档加载从 reader_book_prefs 读。
  const bookZhConvert = ref<ZhConvertConfig | ''>('')
  // 每书阅读主题覆盖（R3）：'' = 跟随全局槽；随文档从 reader_book_prefs.theme 读。
  const bookReaderTheme = ref('')
  const toast = useToastStore()
  const { t } = useI18n()
  let loadGeneration = 0

  /** 换文档时的同步复位（与原 load() 顶部内联复位一致，供 load() 在拉取新 prefs 之前调用）。 */
  function resetLocal() {
    loadGeneration++
    bookEncoding.value = ''
    bookReflow.value = false
    bookZhConvert.value = ''
    bookReaderTheme.value = ''
  }

  // 每书设置：简繁（txt/md/epub 通用）+ 编码/重排（仅 txt）。从 reader_book_prefs 读，供设置面板显示 +
  // textSource.reflow 透传。须在 detail.value=d（触发 BookReader 挂载读 textSource）之前完成（由 root load() 保证调用时机）。
  async function loadForItem(itemId: number) {
    const generation = ++loadGeneration
    const prefsJson = await invokeIpc<string | null>(IPC.GET_READER_BOOK_PREFS, { itemId }).catch(
      () => null,
    )
    if (generation !== loadGeneration || deps.id.value !== itemId) return
    if (prefsJson) {
      try {
        const p = JSON.parse(prefsJson) as ReaderBookPrefs
        bookEncoding.value = typeof p.encoding === 'string' ? p.encoding : ''
        bookReflow.value = p.reflow === true
        bookZhConvert.value = typeof p.zhConvert === 'string' ? p.zhConvert : ''
        // 每书主题（R3）：存字符串即收；有效性由 BookReader.resolveReaderColors 兜底（未注册/kind 不符→跟随全局）。
        bookReaderTheme.value = typeof p.theme === 'string' ? p.theme : ''
      } catch {
        /* 损坏 prefs 忽略，用默认（自动编码 + 不重排 + 不转换） */
      }
    }
  }

  // 每书设置变更（R2-6c / R4 / R3）：写 reader_book_prefs。编码/重排改 canonical、简繁不可逆 → 须重挂载；
  // 每书主题（R3）是纯显示层且可逆（BookReader watch bookReaderTheme 实时重着色）→ 只改主题时**不重挂载**
  // （避免无谓 remount + 丢阅读位置）。
  async function onBookPrefsChange(v: {
    encoding: string
    reflow: boolean
    zhConvert: ZhConvertConfig | ''
    theme: string
  }) {
    const itemId = deps.id.value
    // 仅当改 canonical / 不可逆的字段变化时才重挂载；主题变化经 watch 实时生效。
    const needsRemount =
      v.encoding !== bookEncoding.value ||
      v.reflow !== bookReflow.value ||
      v.zhConvert !== bookZhConvert.value
    bookEncoding.value = v.encoding
    bookReflow.value = v.reflow
    bookZhConvert.value = v.zhConvert
    bookReaderTheme.value = v.theme
    const prefs: ReaderBookPrefs = {
      v: 1,
      reflow: v.reflow,
      ...(v.encoding ? { encoding: v.encoding } : {}),
      ...(v.zhConvert ? { zhConvert: v.zhConvert } : {}),
      ...(v.theme ? { theme: v.theme } : {}),
    }
    try {
      await invokeIpc(IPC.SET_READER_BOOK_PREFS, {
        itemId,
        prefs: JSON.stringify(prefs),
      })
    } catch (e) {
      if (deps.id.value === itemId) {
        toast.addToast('error', t('doc.openFailed', { error: ipcErrorMessage(e) }))
      }
      return
    }
    if (deps.id.value !== itemId) return
    if (needsRemount) {
      // 同上:remount 前保留当前阅读位置。编码/重排改 canonical,旧 CFI 可能无法精确解析→foliate 优雅回退章首(仍优于跳回开卷);简繁是显示层,CFI 仍有效。
      deps.requestRemount(true)
    } // 重挂载：编码重解码 / reflow 重分段 / 简繁按新档位重转。
  }

  return {
    bookEncoding,
    bookReflow,
    bookZhConvert,
    bookReaderTheme,
    resetLocal,
    loadForItem,
    onBookPrefsChange,
  }
}
