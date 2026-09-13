// buildInfoSegments 单测:段产出与顺序即折叠契约(重要度降序,Priority+ 从尾折)。
import { describe, it, expect } from 'vitest'
import { buildInfoSegments } from './statusBarFileInfo.helpers'
import type { ViewerFileInfo } from '../../stores/viewerStore'

function info(partial: Partial<ViewerFileInfo> = {}): ViewerFileInfo {
  return {
    rating: 0,
    isFavorited: false,
    colorLabel: 0,
    fileSize: 2_400_000,
    width: null,
    height: null,
    durationMs: null,
    ...partial,
  }
}

describe('buildInfoSegments', () => {
  it('图片:文件名/尺寸/大小/格式,无时长;顺序=重要度降序', () => {
    const segs = buildInfoSegments('IMG_0001.jpg', 'jpg', info({ width: 4000, height: 3000 }))
    expect(segs.map((s) => s.key)).toEqual(['fileName', 'dims', 'fileSize', 'format'])
    expect(segs[1].text).toBe('4000×3000')
    expect(segs[3].text).toBe('JPG')
  })

  it('视频:尺寸与时长并存,时长排尺寸后', () => {
    const segs = buildInfoSegments(
      'clip.mp4',
      'mp4',
      info({ width: 1920, height: 1080, durationMs: 754_000 }),
    )
    expect(segs.map((s) => s.key)).toEqual(['fileName', 'dims', 'duration', 'fileSize', 'format'])
    expect(segs[2].text).toBe('12:34')
  })

  it('音频:无尺寸,有时长', () => {
    const segs = buildInfoSegments('song.flac', 'flac', info({ durationMs: 61_000 }))
    expect(segs.map((s) => s.key)).toEqual(['fileName', 'duration', 'fileSize', 'format'])
  })

  it('标量未就绪(info=null):仅出文件名与格式,不出大小/尺寸/时长', () => {
    const segs = buildInfoSegments('book.epub', 'epub', null)
    expect(segs.map((s) => s.key)).toEqual(['fileName', 'format'])
  })

  it('零值尺寸已在投影层归一 null(此处防回归:null 不产尺寸段)', () => {
    const segs = buildInfoSegments('doc.pdf', 'pdf', info())
    expect(segs.some((s) => s.key === 'dims')).toBe(false)
  })

  it('空标题/空格式不产段(外部文件兜底)', () => {
    const segs = buildInfoSegments('', '', info())
    expect(segs.map((s) => s.key)).toEqual(['fileSize'])
  })
})
