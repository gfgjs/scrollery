// useOcr.spec.ts — 门控三分支 + 空结果 + 复制 + busy 互斥 + extracting toast + closePanel 清空(T9)。
// 环境:node,无 @vue/test-utils(仓内惯例,见 useGalleryQuerySync.spec.ts)——mock vue-router/
// vue-i18n 为最小 fake,toastStore 用真 pinia store(setActivePinia)。useOcr 模块级单例状态跨用例
// 共享,故每例前重置 busy/panelOpen/result。

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { invokeIpc } from '../../utils/ipc'
import type { OcrStatus, OcrResult } from '../../types/ocr'

vi.mock('../../utils/ipc', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../utils/ipc')>()
  return { ...actual, invokeIpc: vi.fn() }
})

const pushCalls: string[] = []
vi.mock('vue-router', () => ({
  useRouter: () => ({
    push: (path: string) => {
      pushCalls.push(path)
      return Promise.resolve()
    },
  }),
}))

vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key }),
}))

import { useOcr, resetOcrStatusCache } from '../useOcr'
import { useToastStore } from '../../stores/toastStore'

function authorizedStatus(installed = true, manifestReady = true): OcrStatus {
  return {
    availability: 'authorized',
    storeUrl: null,
    activeTier: 'pp-ocrv5-mobile',
    tiers: [{ id: 'pp-ocrv5-mobile', displayName: 'Mobile', sizeMb: 30, installed, manifestReady }],
  }
}

function makeResult(lines: OcrResult['lines']): OcrResult {
  return { lines, width: 10, height: 10 }
}

const mockedInvoke = vi.mocked(invokeIpc)

beforeEach(() => {
  setActivePinia(createPinia())
  resetOcrStatusCache()
  pushCalls.length = 0
  mockedInvoke.mockReset()
  const ocr = useOcr()
  ocr.busy.value = false
  ocr.panelOpen.value = false
  ocr.result.value = null
  ocr.sourceLabel.value = ''
})

describe('useOcr 门控三分支', () => {
  it('未授权 → toast 提示 + 跳插件商店,不发提取命令', async () => {
    mockedInvoke.mockResolvedValueOnce({
      availability: 'installedUnlicensed',
      storeUrl: null,
      activeTier: 'pp-ocrv5-mobile',
      tiers: [],
    } satisfies OcrStatus)
    const ocr = useOcr()
    const toast = useToastStore()
    await ocr.extractFromImage(1, 'a.png')
    expect(pushCalls).toEqual(['/plugins'])
    expect(mockedInvoke).toHaveBeenCalledTimes(1) // 只发了 ocr_status,未发 ocr_extract_image
    expect(toast.toasts.some((msg) => msg.message === 'ocr.unlicensed')).toBe(true)
    // extracting toast 挪到门控通过之后:门控失败不得双弹。
    expect(toast.toasts.some((msg) => msg.message === 'ocr.extracting')).toBe(false)
  })

  it('模型未装 → toast 提示 + 跳设置页,不发提取命令', async () => {
    mockedInvoke.mockResolvedValueOnce(authorizedStatus(false))
    const ocr = useOcr()
    const toast = useToastStore()
    await ocr.extractFromImage(1, 'a.png')
    expect(pushCalls).toEqual(['/settings'])
    expect(mockedInvoke).toHaveBeenCalledTimes(1)
    expect(toast.toasts.some((msg) => msg.message === 'ocr.modelMissing')).toBe(true)
    expect(toast.toasts.some((msg) => msg.message === 'ocr.extracting')).toBe(false)
  })

  it('清单未就绪(manifestReady=false)→ 同模型未装:toast + 跳设置,不发提取命令(J14 逐档门)', async () => {
    mockedInvoke.mockResolvedValueOnce(authorizedStatus(true, false))
    const ocr = useOcr()
    const toast = useToastStore()
    await ocr.extractFromImage(1, 'a.png')
    expect(pushCalls).toEqual(['/settings'])
    expect(mockedInvoke).toHaveBeenCalledTimes(1)
    expect(toast.toasts.some((msg) => msg.message === 'ocr.modelMissing')).toBe(true)
    expect(toast.toasts.some((msg) => msg.message === 'ocr.extracting')).toBe(false)
  })

  it('通过 → 发起提取命令,非空结果开面板', async () => {
    mockedInvoke.mockResolvedValueOnce(authorizedStatus()).mockResolvedValueOnce(
      makeResult([
        {
          text: 'hi',
          quad: [
            [0, 0],
            [1, 0],
            [1, 1],
            [0, 1],
          ],
          confidence: 0.9,
        },
      ]),
    )
    const ocr = useOcr()
    const toast = useToastStore()
    await ocr.extractFromImage(1, 'a.png')
    expect(mockedInvoke).toHaveBeenCalledTimes(2)
    expect(ocr.panelOpen.value).toBe(true)
    expect(ocr.result.value?.lines).toHaveLength(1)
    expect(ocr.sourceLabel.value).toBe('a.png')
    // 边界8:busy 置位后即发排队提醒(120s 上限反馈,按钮禁用不够)。
    expect(toast.toasts.some((msg) => msg.message === 'ocr.extracting')).toBe(true)
  })

  it('closePanel 收口:同时清空 panelOpen/result/sourceLabel,防旧结果闪现', async () => {
    mockedInvoke.mockResolvedValueOnce(authorizedStatus()).mockResolvedValueOnce(
      makeResult([
        {
          text: 'hi',
          quad: [
            [0, 0],
            [1, 0],
            [1, 1],
            [0, 1],
          ],
          confidence: 0.9,
        },
      ]),
    )
    const ocr = useOcr()
    await ocr.extractFromImage(1, 'a.png')
    expect(ocr.panelOpen.value).toBe(true)
    ocr.closePanel()
    expect(ocr.panelOpen.value).toBe(false)
    expect(ocr.result.value).toBeNull()
    expect(ocr.sourceLabel.value).toBe('')
  })
})

describe('useOcr 空结果与复制', () => {
  it('空结果 → toast ocr.empty,不开面板', async () => {
    mockedInvoke.mockResolvedValueOnce(authorizedStatus()).mockResolvedValueOnce(makeResult([]))
    const ocr = useOcr()
    const toast = useToastStore()
    await ocr.extractFromImage(1, 'a.png')
    expect(ocr.panelOpen.value).toBe(false)
    expect(toast.toasts.some((msg) => msg.message === 'ocr.empty')).toBe(true)
  })

  it('复制全部调用 navigator.clipboard.writeText,仅用户点击触发', async () => {
    mockedInvoke.mockResolvedValueOnce(authorizedStatus()).mockResolvedValueOnce(
      makeResult([
        {
          text: 'line1',
          quad: [
            [0, 0],
            [1, 0],
            [1, 1],
            [0, 1],
          ],
          confidence: 0.9,
        },
        {
          text: 'line2',
          quad: [
            [0, 0],
            [1, 0],
            [1, 1],
            [0, 1],
          ],
          confidence: 0.9,
        },
      ]),
    )
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.assign(navigator, { clipboard: { writeText } })
    const ocr = useOcr()
    const toast = useToastStore()
    await ocr.extractFromImage(1, 'a.png')
    expect(writeText).not.toHaveBeenCalled()
    await ocr.copyAll()
    expect(writeText).toHaveBeenCalledWith('line1\nline2')
    expect(toast.toasts.some((msg) => msg.message === 'ocr.copied')).toBe(true)
  })
})

describe('useOcr busy 互斥', () => {
  it('提取进行中再次调用直接短路,不重复发命令', async () => {
    let resolveStatus: (v: OcrStatus) => void = () => {}
    mockedInvoke
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveStatus = resolve as (v: OcrStatus) => void
          }),
      )
      .mockResolvedValueOnce(makeResult([])) // 门控通过后第一次调用发起的提取命令(空结果,简化断言)
    const ocr = useOcr()
    const toast = useToastStore()
    const p1 = ocr.extractFromImage(1, 'a.png')
    expect(ocr.busy.value).toBe(true) // 同步置位:门控判定前已置 busy,故此刻已为 true

    const p2 = ocr.extractFromImage(1, 'a.png') // busy 互斥:直接短路,不再发 ocr_status
    expect(mockedInvoke).toHaveBeenCalledTimes(1) // 第二次调用被短路,未追加任何 invoke

    resolveStatus(authorizedStatus(true)) // 让第一次调用门控通过,继续发提取命令
    await Promise.all([p1, p2])
    expect(mockedInvoke).toHaveBeenCalledTimes(2) // ocr_status + ocr_extract_image,各恰一次
    // 短路的 p2 从未进入门控通过后的 toast 段,互斥期间只有 p1 发了一条 extracting。
    expect(toast.toasts.filter((msg) => msg.message === 'ocr.extracting')).toHaveLength(1)
    expect(ocr.busy.value).toBe(false)
  })

  it('提取在途 closePanel 后返回的结果不开面板(迟到结果作废)', async () => {
    let resolveExtract: (v: OcrResult) => void = () => {}
    mockedInvoke.mockResolvedValueOnce(authorizedStatus()).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveExtract = resolve as (v: OcrResult) => void
        }),
    )
    const ocr = useOcr()
    const p = ocr.extractFromImage(1, 'a.png')
    // 等到门控通过、提取命令真正发出(第二次 invokeIpc 调用,此时 resolveExtract 才被赋为真实
    // resolver)再关闭面板——closePanel 令 reqSeq 过期,不要求发生在 invoke resolve 之前的哪一刻。
    await vi.waitFor(() => expect(mockedInvoke).toHaveBeenCalledTimes(2))
    ocr.closePanel()

    resolveExtract(
      makeResult([
        {
          text: 'stale',
          quad: [
            [0, 0],
            [1, 0],
            [1, 1],
            [0, 1],
          ],
          confidence: 0.9,
        },
      ]),
    )
    await p
    expect(ocr.panelOpen.value).toBe(false) // 迟到结果不得把已关闭的面板重新弹开
    expect(ocr.result.value).toBeNull()
  })
})
