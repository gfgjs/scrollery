// videoDiagnostics 单测:五条规则各一例 + 回退例
import { describe, it, expect } from 'vitest'
import { diagnoseVideoError } from './videoDiagnostics'

const emptyProbes = { canPlayHevc: '', canPlayAv1: '', canPlayMkv: '' }

describe('diagnoseVideoError', () => {
  it('规则1:mkv 容器且系统不支持 mkv 探针', () => {
    const r = diagnoseVideoError({
      extension: 'mkv',
      mediaErrorCode: 4,
      probes: emptyProbes,
      videoCodec: null,
    })
    expect(r.titleKey).toBe('player.diag.mkvContainer.title')
    expect(r.hintKey).toBe('player.diag.mkvContainer.hint')
    expect(r.technical).toContain('code=4')
    expect(r.technical).toContain('extension=mkv')
  })

  it('规则2a:videoCodec 含 hevc/h265(ffprobe 长名格式)', () => {
    // 注:MP4 codecs= 参数的 fourcc 是 hev1/hvc1(不含字面 "hevc" 子串),此处用 ffprobe
    // codec_long_name 常见格式(含字面 "HEVC"),真正命中 looksHevcByCodec 子串匹配。
    const r = diagnoseVideoError({
      extension: 'mp4',
      mediaErrorCode: 4,
      probes: { ...emptyProbes, canPlayMkv: 'maybe' },
      videoCodec: 'HEVC (Main Profile)',
    })
    expect(r.titleKey).toBe('player.diag.hevcUnsupported.title')
  })

  it('规则2a:videoCodec 显式为 "hevc"(非扩展命名)', () => {
    const r = diagnoseVideoError({
      extension: 'mp4',
      mediaErrorCode: 4,
      probes: { ...emptyProbes, canPlayMkv: 'maybe' },
      videoCodec: 'hevc',
    })
    expect(r.titleKey).toBe('player.diag.hevcUnsupported.title')
  })

  it('规则2b(已收紧):codec 未知+探针空+code4 不再误判 HEVC,落回退', () => {
    const r = diagnoseVideoError({
      extension: 'mp4',
      mediaErrorCode: 4,
      probes: emptyProbes,
      videoCodec: null,
    })
    expect(r.titleKey).toBe('player.diag.unknown.title')
    expect(r.hintKey).toBe('player.diag.unknown.hint')
  })

  it('规则3:AV1 编码但系统/浏览器版本不支持', () => {
    const r = diagnoseVideoError({
      extension: 'mp4',
      mediaErrorCode: 4,
      probes: { canPlayHevc: 'probably', canPlayAv1: '', canPlayMkv: 'maybe' },
      videoCodec: 'av01.0.05M.08',
    })
    expect(r.titleKey).toBe('player.diag.av1Unsupported.title')
  })

  it('规则4:文件不可读(MEDIA_ERR_NETWORK)', () => {
    const r = diagnoseVideoError({
      extension: 'mp4',
      mediaErrorCode: 2,
      probes: { canPlayHevc: 'probably', canPlayAv1: 'probably', canPlayMkv: 'maybe' },
      videoCodec: 'avc1.640028',
    })
    expect(r.titleKey).toBe('player.diag.fileUnreadable.title')
  })

  it('回退:MEDIA_ERR_DECODE 等未归类场景', () => {
    const r = diagnoseVideoError({
      extension: 'mp4',
      mediaErrorCode: 3,
      probes: { canPlayHevc: 'probably', canPlayAv1: 'probably', canPlayMkv: 'maybe' },
      videoCodec: 'avc1.640028',
    })
    expect(r.titleKey).toBe('player.diag.unknown.title')
    expect(r.hintKey).toBe('player.diag.unknown.hint')
  })

  it('规则0:resolveMode=needsComponent 优先于其余规则', () => {
    const r = diagnoseVideoError({
      extension: 'mkv',
      mediaErrorCode: 4,
      probes: emptyProbes,
      videoCodec: null,
      resolveMode: 'needsComponent',
    })
    expect(r.titleKey).toBe('player.diag.needsComponent.title')
    expect(r.hintKey).toBe('player.diag.needsComponent.hint')
    expect(r.technical).toContain('resolveMode=needsComponent')
  })

  it('规则0:resolveMode=needsHevcExt 复用既有 HEVC 引导文案', () => {
    const r = diagnoseVideoError({
      extension: 'mp4',
      mediaErrorCode: 4,
      probes: emptyProbes,
      videoCodec: null,
      resolveMode: 'needsHevcExt',
    })
    expect(r.titleKey).toBe('player.diag.hevcUnsupported.title')
    expect(r.hintKey).toBe('player.diag.hevcUnsupported.hint')
  })

  it('technical 串含 code+extension+probes 原文', () => {
    const r = diagnoseVideoError({
      extension: 'avi',
      mediaErrorCode: null,
      probes: { canPlayHevc: 'probably', canPlayAv1: '', canPlayMkv: '' },
      videoCodec: undefined,
    })
    expect(r.technical).toContain('code=null')
    expect(r.technical).toContain('extension=avi')
    expect(r.technical).toContain('"canPlayHevc":"probably"')
  })
})
