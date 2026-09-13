import { describe, expect, it, vi } from 'vitest'
import { invokeIpc } from '../utils/ipc'
import { useImageEditor } from './useImageEditor'

vi.mock('../utils/ipc', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../utils/ipc')>()
  return { ...actual, invokeIpc: vi.fn() }
})

describe('useImageEditor 拉直状态', () => {
  it('使用最大内接尺寸作为 crop 坐标系并参与 hasChanges', () => {
    const editor = useImageEditor()
    editor.setNaturalSize(4000, 3000)
    editor.setRotateFine(45)

    expect(editor.rotateFine.value).toBe(45)
    expect([editor.postWidth.value, editor.postHeight.value]).toEqual([2424, 1818])
    expect(editor.hasChanges.value).toBe(true)

    editor.setCropRect({ x: 0.25, y: 0.25, width: 0.5, height: 0.5 })
    expect(editor.cropToPixelRect()).toEqual({ x: 606, y: 455, width: 1212, height: 909 })
  })

  it('90° 后交换 fine rotate 输入尺寸，角度变化清空旧 crop', () => {
    const editor = useImageEditor()
    editor.setNaturalSize(4000, 3000)
    editor.rotateCw()
    editor.setCropRect({ x: 0.1, y: 0.1, width: 0.8, height: 0.8 })
    editor.setRotateFine(-45)

    expect([editor.preFineWidth.value, editor.preFineHeight.value]).toEqual([3000, 4000])
    expect([editor.postWidth.value, editor.postHeight.value]).toEqual([1818, 2424])
    expect(editor.crop.value).toBeNull()
  })

  it('UI 写入按闭区间和 0.1° 步进收敛，reset 恢复 v1 零值', () => {
    const editor = useImageEditor()
    editor.setRotateFine(99)
    expect(editor.rotateFine.value).toBe(45)
    editor.setRotateFine(-2.26)
    expect(editor.rotateFine.value).toBe(-2.3)
    editor.resetOps()
    expect(editor.rotateFine.value).toBe(0)
    expect(editor.hasChanges.value).toBe(false)
  })
})

describe('useImageEditor 调色状态', () => {
  it('滑杆写入 clamp 到 [-100,100] 并取整,非有限值归 0', () => {
    const editor = useImageEditor()
    editor.setBrightness(150)
    expect(editor.brightness.value).toBe(100)
    editor.setContrast(-333)
    expect(editor.contrast.value).toBe(-100)
    editor.setSaturation(12.6)
    expect(editor.saturation.value).toBe(13)
    editor.setBrightness(Number.NaN)
    expect(editor.brightness.value).toBe(0)
    editor.setSaturation(-0.4)
    expect(Object.is(editor.saturation.value, -0)).toBe(false)
    expect(editor.saturation.value).toBe(0)
  })

  it('调色参与 hasChanges,reset 连带清零,且不清空裁剪框', () => {
    const editor = useImageEditor()
    editor.setNaturalSize(4000, 3000)
    editor.setCropRect({ x: 0.1, y: 0.1, width: 0.8, height: 0.8 })
    editor.setContrast(30)
    // 调色不改变几何坐标系:与 90°/拉直不同,不应清空已拉的框。
    expect(editor.crop.value).not.toBeNull()
    expect(editor.hasChanges.value).toBe(true)

    editor.resetOps()
    expect(editor.contrast.value).toBe(0)
    expect(editor.hasChanges.value).toBe(false)
  })

  it('保存载荷:全零不携带 adjust(D-106),非零携带整数三参数', async () => {
    const mock = vi.mocked(invokeIpc)
    mock.mockResolvedValue({ status: 'saved', newItemId: 7 })
    const editor = useImageEditor()
    editor.setNaturalSize(100, 50)

    await editor.save(1)
    let payload = mock.mock.calls[0][1] as { ops: { adjust?: unknown } }
    expect(payload.ops.adjust).toBeUndefined()

    editor.setBrightness(20)
    editor.setContrast(-5)
    await editor.save(1)
    payload = mock.mock.calls[1][1] as { ops: { adjust?: unknown } }
    expect(payload.ops.adjust).toEqual({ brightness: 20, contrast: -5, saturation: 0 })
    expect(editor.status.value).toBe('done')
  })
})
