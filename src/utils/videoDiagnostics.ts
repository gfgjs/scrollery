// src/utils/videoDiagnostics.ts
// 视频播放失败诊断:根据扩展名/MediaError code/canPlayType 探针推断可能原因

/** MediaError.code 标准值(与 DOM MediaError 常量对齐,避免依赖 DOM lib 枚举) */
const MEDIA_ERR_NETWORK = 2

/**
 * 诊断输入。probes 为 `<video>.canPlayType()` 对各编码/容器的探测结果
 * (空字符串代表浏览器/系统判定不支持)。
 */
export interface VideoDiagnosisInput {
  extension: string
  mediaErrorCode: number | null
  probes: {
    canPlayHevc: string
    canPlayAv1: string
    canPlayMkv: string
  }
  videoCodec?: string | null
  /**
   * 视频格式扩展子系统(design.md §5.3)的播放链路解析结论(useVideoSource `resolveMode`)。
   * 有值时说明 host 侧 `resolve_video_playback` 判定表(§5.1)已给出明确归因——须优先于下方
   * 基于 MediaError/canPlayType 的猜测链,给出针对性引导(替代笼统的「不支持」)。
   */
  resolveMode?: 'needsComponent' | 'needsHevcExt'
}

/** 诊断结果:i18n 键名(标题/提示)+ 供日志/技术详情面板使用的原文串。 */
export interface VideoDiagnosisResult {
  titleKey: string
  hintKey: string
  technical: string
}

/**
 * 已知限制:AC-3 等系统未安装解码器的音轨在多数平台上表现为静音播放而非
 * `error` 事件,不会触发 <video> 的 error 处理路径,因此本 util 不覆盖、
 * 也无法覆盖该场景(需要额外的"有画面无声音"探测,不属于错误诊断范畴)。
 */
export function diagnoseVideoError(input: VideoDiagnosisInput): VideoDiagnosisResult {
  const { extension, mediaErrorCode, probes, videoCodec, resolveMode } = input
  const ext = extension.toLowerCase().replace(/^\./, '')
  const technical = `code=${String(mediaErrorCode)} extension=${extension} probes=${JSON.stringify(probes)} resolveMode=${resolveMode ?? 'n/a'}`

  // 规则 0(视频格式扩展子系统,design.md §5.1/§5.3):host 侧判定表已给出明确归因时,优先用它——
  // 不必再靠下方 MediaError/canPlayType 猜测链推断,能给出针对性引导(下载组件/装 HEVC 扩展)。
  if (resolveMode === 'needsComponent') {
    return {
      titleKey: 'player.diag.needsComponent.title',
      hintKey: 'player.diag.needsComponent.hint',
      technical,
    }
  }
  if (resolveMode === 'needsHevcExt') {
    // 复用既有 HEVC 引导文案(与规则 2a 同源):useVideoSource 侧 mediaCapabilities 实测仅用于
    // 「可解码则直接当 direct 播放、绕过此诊断路径」的短路判断,不代表走到这里就排除了误报——
    // 试播仍失败时以 host 侧原始判定(needsHevcExt)归因,不再靠前端探测结果二次纠偏。
    return {
      titleKey: 'player.diag.hevcUnsupported.title',
      hintKey: 'player.diag.hevcUnsupported.hint',
      technical,
    }
  }

  // 规则 1:mkv 容器且系统/浏览器完全不支持 mkv 探针
  if (ext === 'mkv' && probes.canPlayMkv === '') {
    return { titleKey: 'player.diag.mkvContainer.title', hintKey: 'player.diag.mkvContainer.hint', technical }
  }

  // 规则 2a:videoCodec 明确标识 HEVC/H.265
  const codecLower = (videoCodec ?? '').toLowerCase()
  const looksHevcByCodec = codecLower.includes('hevc') || codecLower.includes('h265') || codecLower.includes('h.265')
  if (looksHevcByCodec) {
    return { titleKey: 'player.diag.hevcUnsupported.title', hintKey: 'player.diag.hevcUnsupported.hint', technical }
  }

  // 规则 3:videoCodec 明确标识 AV1 且探针不支持
  const looksAv1ByCodec = codecLower.includes('av01') || codecLower.includes('av1')
  if (looksAv1ByCodec && probes.canPlayAv1 === '') {
    return { titleKey: 'player.diag.av1Unsupported.title', hintKey: 'player.diag.av1Unsupported.hint', technical }
  }

  // 规则 2b(已收紧,采纳 opus 深审):此前「codec 未知 + HEVC 探针为空 + code=4」即判定 HEVC 缺扩展,
  // 但 Windows 未装 HEVC 扩展的机器上 canPlayHevc 探针恒为空,该规则会把"文件缺失/损坏"等一切
  // code=4 场景误诊成 HEVC——必须有 videoCodec 实证(规则 2a)才能判 HEVC,故此分支删除,未知场景落
  // 规则 5 的通用回退(不新增 i18n 键,technical 串已含 probes 矩阵供排查)。

  // 规则 4:文件不可读(网络/IO 错误,或 code 缺失的场景——404 等已由调用方
  // 提前拦截,此处只处理走到播放器层面的 network 错误)
  if (mediaErrorCode === MEDIA_ERR_NETWORK) {
    return { titleKey: 'player.diag.fileUnreadable.title', hintKey: 'player.diag.fileUnreadable.hint', technical }
  }

  // 规则 5:未知回退(含 MEDIA_ERR_DECODE=3 等未归类场景)
  return { titleKey: 'player.diag.unknown.title', hintKey: 'player.diag.unknown.hint', technical }
}
