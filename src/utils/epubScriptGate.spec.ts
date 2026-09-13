// src/utils/epubScriptGate.spec.ts
// 「脚本化 EPUB 不支持」(设计 §4.8)的执行契约(2026-07-16 安全审查)。
//
// 这条裁决此前**只是文档里的一句话** —— 上游 seam 无人订阅,allow 恒真。本文件钉住它现在真的被执行。
// 用真 EventTarget + 真 CustomEvent 驱动(node 原生有,无需 DOM),复现上游 epub.js loadItem 的调用形状:
//   const detail = { type, isScript, allow: true }
//   eventTarget.dispatchEvent(new CustomEvent('load', { detail }))
//   const allow = await detail.allow   // ← 上游读的是**派发后**的 detail
//   if (!allow) return null

import { describe, it, expect, beforeEach, vi } from 'vitest'

const { loggerWarn } = vi.hoisted(() => ({ loggerWarn: vi.fn() }))
vi.mock('./logger', () => ({
  logger: { debug: vi.fn(), info: vi.fn(), warn: loggerWarn, error: vi.fn() },
}))

import { denyScriptResources } from './epubScriptGate'

/** 复现上游 epub.js loadItem 的那几行 —— 判据必须对着**真实调用形状**,而非我臆想的形状。 */
async function simulateLoadItem(
  target: EventTarget,
  type: string,
  isScript: boolean,
): Promise<boolean> {
  const detail = { type, isScript, allow: true }
  target.dispatchEvent(new CustomEvent('load', { detail }))
  return await detail.allow
}

beforeEach(() => {
  loggerWarn.mockClear()
})

describe('脚本资源被拒', () => {
  it('isScript 的项 → allow 置 false(上游据此 return null,该资源不落 blob URL)', async () => {
    const target = new EventTarget()
    denyScriptResources({ transformTarget: target })
    expect(await simulateLoadItem(target, 'text/javascript', true)).toBe(false)
  })

  it('拒绝时留 warn 诊断 —— 静默拒绝会让「这本书某功能没了」变成玄学', async () => {
    const target = new EventTarget()
    denyScriptResources({ transformTarget: target })
    await simulateLoadItem(target, 'application/javascript', true)
    expect(loggerWarn).toHaveBeenCalled()
  })
})

describe('非脚本资源不受影响(不能把书弄坏)', () => {
  it('XHTML 章节、CSS、字体、图片一律放行', async () => {
    const target = new EventTarget()
    denyScriptResources({ transformTarget: target })
    for (const type of [
      'application/xhtml+xml',
      'text/css',
      'font/woff2',
      'image/jpeg',
      'image/svg+xml',
    ]) {
      expect(await simulateLoadItem(target, type, false)).toBe(true)
    }
  })
})

describe('装配', () => {
  it('无 transformTarget 的 book(SyntheticBook/comic/fb2)静默跳过,不抛', () => {
    expect(() => denyScriptResources({})).not.toThrow()
    expect(() => denyScriptResources(undefined)).not.toThrow()
  })

  it('返回的摘除函数生效后不再拦(卸载后不留悬挂订阅)', async () => {
    const target = new EventTarget()
    const dispose = denyScriptResources({ transformTarget: target })
    expect(await simulateLoadItem(target, 'text/javascript', true)).toBe(false)
    dispose()
    expect(await simulateLoadItem(target, 'text/javascript', true)).toBe(true)
  })
})
