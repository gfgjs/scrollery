// src/composables/reader/useReaderBookPrefs.ts
// 每书设置；需重挂载时调用 root 的 requestRemount(true)，统一先捕获阅读位置。
import { ref, type Ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { IPC } from '../../constants/ipc'
import { invokeIpc, ipcErrorMessage } from '../../utils/ipc'
import { useToastStore } from '../../stores/toastStore'
import type { MarkdownEngine, ReaderBookPrefs, ZhConvertConfig } from '../../types/reader'

export interface UseReaderBookPrefsDeps {
  id: Ref<number>
  /** 仅有的 capture+bump 实现在 root（红线：不可在此另写一份）。 */
  requestRemount: (captureFirst: boolean) => void
}

export function useReaderBookPrefs(deps: UseReaderBookPrefsDeps) {
  // txt 编码覆盖（空值为自动）与二级重排。
  const bookEncoding = ref('')
  const bookReflow = ref(false)
  // txt/md/epub 的显示层简繁转换，空值为关闭。
  const bookZhConvert = ref<ZhConvertConfig | ''>('')
  // 每书主题覆盖，空值为跟随全局槽。
  const bookReaderTheme = ref('')
  const bookMarkdownEngine = ref<MarkdownEngine>('native')
  const toast = useToastStore()
  const { t } = useI18n()
  let loadGeneration = 0
  let changeGeneration = 0
  const pendingWrites = new Map<number, Promise<void>>()
  const currentTextPrefs = () => ({
    encoding: bookEncoding.value,
    reflow: bookReflow.value,
    zhConvert: bookZhConvert.value,
  })
  type TextPrefs = ReturnType<typeof currentTextPrefs>
  const sameTextPrefs = (left: TextPrefs, right: TextPrefs) =>
    left.encoding === right.encoding && left.reflow === right.reflow && left.zhConvert === right.zhConvert
  // 界面保留失败后的选择，已应用基准只能在加载或成功重挂时推进。
  let appliedTextPrefs = currentTextPrefs()

  /** 换文档时在拉取新偏好之前同步复位，并作废旧请求。 */
  function resetLocal() {
    loadGeneration++
    bookEncoding.value = ''
    bookReflow.value = false
    bookZhConvert.value = ''
    bookReaderTheme.value = ''
    bookMarkdownEngine.value = 'native'
    appliedTextPrefs = currentTextPrefs()
  }

  // 每书设置：简繁（txt/md/epub 通用）+ 编码/重排（仅 txt）。从 reader_book_prefs 读，供设置面板显示 +
  // textSource.reflow 透传。须在 detail.value=d（触发 BookReader 挂载读 textSource）之前完成（由 root load() 保证调用时机）。
  async function loadForItem(itemId: number) {
    const generation = ++loadGeneration
    // 切走后立即返回时，先等这本书已提交的偏好落盘，避免用旧快照覆盖刚选的引擎。
    await waitForPendingWrites(itemId)
    if (generation !== loadGeneration || deps.id.value !== itemId) return
    // 读取失败交由父层阻止开卷并提供重试，不能当作无偏好后用默认快照覆盖已有设置。
    const prefsJson = await invokeIpc<string | null>(IPC.GET_READER_BOOK_PREFS, { itemId })
    if (generation !== loadGeneration || deps.id.value !== itemId) return
    bookMarkdownEngine.value = 'native'
    if (prefsJson) {
      try {
        const p = JSON.parse(prefsJson) as ReaderBookPrefs
        bookEncoding.value = typeof p.encoding === 'string' ? p.encoding : ''
        bookReflow.value = p.reflow === true
        bookZhConvert.value = typeof p.zhConvert === 'string' ? p.zhConvert : ''
        // 主题有效性由 BookReader.resolveReaderColors 校验，无效时跟随全局。
        bookReaderTheme.value = typeof p.theme === 'string' ? p.theme : ''
        if (p.markdownEngine === 'vditor' || p.markdownEngine === 'markdown-editor') {
          bookMarkdownEngine.value = p.markdownEngine
        }
      } catch {
        /* 损坏 prefs 忽略，用默认（自动编码 + 不重排 + 不转换） */
      }
    }
    appliedTextPrefs = currentTextPrefs()
  }

  function persistCurrentPrefs(itemId: number): Promise<void> {
    const prefs: ReaderBookPrefs = {
      v: 1,
      reflow: bookReflow.value,
      ...(bookEncoding.value ? { encoding: bookEncoding.value } : {}),
      ...(bookZhConvert.value ? { zhConvert: bookZhConvert.value } : {}),
      ...(bookReaderTheme.value ? { theme: bookReaderTheme.value } : {}),
      ...(bookMarkdownEngine.value !== 'native' ? { markdownEngine: bookMarkdownEngine.value } : {}),
    }
    // 同一本书按操作顺序写入完整快照，避免较慢的引擎保存覆盖随后主题修改；换书可独立写入。
    const write = (pendingWrites.get(itemId) ?? Promise.resolve()).then(() =>
      invokeIpc<void>(IPC.SET_READER_BOOK_PREFS, { itemId, prefs: JSON.stringify(prefs) }),
    )
    const settled = write.catch(() => {})
    pendingWrites.set(itemId, settled)
    void settled.then(() => {
      if (pendingWrites.get(itemId) === settled) pendingWrites.delete(itemId)
    })
    return write
  }

  /** 等当前文档已经排队的偏好写入收口；覆盖源文件前用来隔离旧编码快照。 */
  function waitForPendingWrites(itemId: number): Promise<void> {
    return pendingWrites.get(itemId) ?? Promise.resolve()
  }

  /** 成功切换路由后保存每篇 Markdown 的引擎选择，并保留其它每书偏好。 */
  async function saveMarkdownEngine(engine: MarkdownEngine): Promise<boolean> {
    const itemId = deps.id.value
    const generation = loadGeneration
    const change = ++changeGeneration
    const previous = bookMarkdownEngine.value
    bookMarkdownEngine.value = engine
    try {
      await persistCurrentPrefs(itemId)
      return true
    } catch (e) {
      if (generation === loadGeneration && change === changeGeneration && deps.id.value === itemId) {
        bookMarkdownEngine.value = previous
        toast.addToast('error', t('doc.openFailed', { error: ipcErrorMessage(e) }))
      }
      return false
    }
  }

  // 编码、重排或简繁变化需重挂载；主题可实时重着色，单独变更时保留阅读器实例。
  async function onBookPrefsChange(v: {
    encoding: string
    reflow: boolean
    zhConvert: ZhConvertConfig | ''
    theme: string
  }) {
    const itemId = deps.id.value
    const generation = loadGeneration
    const change = ++changeGeneration
    bookEncoding.value = v.encoding
    bookReflow.value = v.reflow
    bookZhConvert.value = v.zhConvert
    bookReaderTheme.value = v.theme
    const savedTextPrefs = currentTextPrefs()
    try {
      await persistCurrentPrefs(itemId)
    } catch (e) {
      if (generation === loadGeneration && change === changeGeneration && deps.id.value === itemId) {
        toast.addToast('error', t('doc.openFailed', { error: ipcErrorMessage(e) }))
      }
      return
    }
    if (generation !== loadGeneration || deps.id.value !== itemId) return
    // 重挂消费当前响应式值；旧快照成功时不能提前应用尚未落盘的新选择。
    if (!sameTextPrefs(savedTextPrefs, currentTextPrefs())) return
    if (!sameTextPrefs(savedTextPrefs, appliedTextPrefs)) {
      // 同上:remount 前保留当前阅读位置。编码/重排改 canonical,旧 CFI 可能无法精确解析→foliate 优雅回退章首(仍优于跳回开卷);简繁是显示层,CFI 仍有效。
      deps.requestRemount(true)
      appliedTextPrefs = savedTextPrefs
    } // 重挂载：编码重解码 / reflow 重分段 / 简繁按新档位重转。
  }

  return {
    bookEncoding,
    bookReflow,
    bookZhConvert,
    bookReaderTheme,
    bookMarkdownEngine,
    resetLocal,
    loadForItem,
    onBookPrefsChange,
    saveMarkdownEngine,
    waitForPendingWrites,
  }
}
