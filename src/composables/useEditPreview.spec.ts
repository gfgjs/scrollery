import { describe, expect, it } from 'vitest'

import { parseEditPreviewPacket, previewPointToSource } from './useEditPreview'

function packet(format: 1 | 2 = 1): Uint8Array {
  const body = Uint8Array.from([10, 20, 30])
  const bytes = new Uint8Array(28 + body.length)
  bytes.set([0x53, 0x45, 0x50, 0x32, 1, format, 0, 0])
  const view = new DataView(bytes.buffer)
  view.setUint32(8, 4000, true)
  view.setUint32(12, 3000, true)
  view.setUint32(16, 2000, true)
  view.setUint32(20, 1500, true)
  view.setUint32(24, body.length, true)
  bytes.set(body, 28)
  return bytes
}

describe('edit preview raw packet', () => {
  it('解析尺寸、格式与编码主体', () => {
    const result = parseEditPreviewPacket(packet(2).buffer)
    expect(result).toMatchObject({
      mimeType: 'image/png',
      sourceWidth: 4000,
      sourceHeight: 3000,
      previewWidth: 2000,
      previewHeight: 1500,
    })
    expect([...result.encoded]).toEqual([10, 20, 30])
  })

  it('拒绝截断、错误 magic 与错误 body 长度', () => {
    expect(() => parseEditPreviewPacket(new Uint8Array(4))).toThrow('truncated')
    const badMagic = packet()
    badMagic[0] = 0
    expect(() => parseEditPreviewPacket(badMagic)).toThrow('magic')
    const badLength = packet()
    new DataView(badLength.buffer).setUint32(24, 99, true)
    expect(() => parseEditPreviewPacket(badLength)).toThrow('body length')
  })

  it('预览坐标按独立 X/Y 比例映射回全尺寸源坐标', () => {
    const result = parseEditPreviewPacket(packet())
    expect(previewPointToSource(1000, 750, result)).toEqual({ x: 2000, y: 1500 })
  })
})
