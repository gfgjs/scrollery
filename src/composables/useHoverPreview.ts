// src/composables/useHoverPreview.ts
// 视频 / 动态照片格子的悬停自动播放预览（需求1, §3.1）。
//
// 设计要点（§3.1）：
//  - **共享池（容量 1）**：模块级 `activeId` 保证同一时刻最多一个格子在预览，移到新格子时
//    旧格子立即卸载 —— 严禁每格一个 video（百万级库内存/解码爆炸）。
//  - **悬停防抖 200ms**：快速划过不触发解码。
//  - **compact / 选择模式 / 设置关闭** 下禁用。
//  - 源解析：普通视频 → 原文件；Apple Live / Google Motion Photo → get_companion_video_url
//    （后端按 companion MOV 或内嵌 MP4 偏移自动抽出，前端无需区分）。
//  - **超大视频降级（§3.3）**：判「重」（分辨率超 4K / 体积超 10GB，见 HEAVY_VIDEO_*）的视频
//    不解码播放，改用预生成的关键帧 sprite 做悬停 scrub（鼠标横向位置 → background-position
//    切帧）；无 sprite 时回退为直接播放。DB 尺寸不可信（enrich 前占位）由 onPreviewMeta 的
//    真实 metadata 运行时补判兜底。

import { ref, computed, onBeforeUnmount, onDeactivated } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { IPC } from '../constants/ipc'
import { useUiStore } from '../stores/uiStore'
import { resolveAssetUrl } from '../utils/assetUrl'

// 同一时刻仅允许一个格子预览（池容量 = 1）。
const activeId = ref<number | null>(null)
// 已解析的预览源缓存（id → convertFileSrc URL），避免重复 hover 重复 IPC。
// 文件被移动/删除时缓存的 URL 变死路径——由 onPreviewError（<video> @error）逐条失效，
// 下次 hover 重新走 GET_MEDIA_DETAIL 拿新路径。
const srcCache = new Map<number, string>()
// 已解析的 sprite 源缓存（id → URL，仅存正结果，路径在缓存目录内稳定可长期持有）。
// 负结果（查过但无 sprite）单独记录于 spriteMissAt 并带 TTL：sprite 可能随后被派生流水线
// 补产（手动「提取关键帧」/后台补跑），永久负缓存会让本场会话 scrub 永远瘫痪。
const spriteCache = new Map<number, string>()
const spriteMissAt = new Map<number, number>()
// 运行时实测判「重」名单：DB 尺寸不可信时（扫描期视频统一占位 1280×720，enrich 前无从辨别），
// 播放起步的 loadedmetadata 会带回真实分辨率；超 4K 者记入此集，当场降级并在后续 hover 直接走
// scrub-only。会话级缓存，无需失效（分辨率是文件固有属性）。
const runtimeHeavy = new Set<number>()
// 当前活动实例的清理入口（随 activeId 同步指向持有者的 onLeave）。窗口失焦/页签隐藏时鼠标事件
// 不会到来，若不主动收尾，后台会一直白解码视频；模块级注册一次（应用单例生命周期，无需反注册）。
let activeLeave: (() => void) | null = null
// document 一并判存在:node 测试环境只桩 window 时本模块仍可安全载入。
if (typeof window !== 'undefined' && typeof document !== 'undefined') {
  window.addEventListener('blur', () => activeLeave?.())
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') activeLeave?.()
  })
}

// 悬停延迟 /「重」视频判据（超过则悬停走 sprite scrub-only 降级、绝不解码大文件）由 uiStore
// 响应式下发（设置键 hover_delay_ms / heavy_video_max_pixels / heavy_video_max_bytes，经
// get_startup_config + config-file-changed 热更新），消费点直接读 `ui.hoverDelayMs` 等，
// 默认值与迁出前常量一致。两条判据依据（为何是这两个数字）：
//  ① 像素面积 > 4K：成本是**持续解码负载**（随分辨率×编码涨，编码轴 codec 需 Rust 铺路，暂缺）。
//     用严格大于——手机拍摄常见即 3840×2160 标准 4K，恰好 4K 放行播放，只挡真超 4K（5K/8K/DCI
//     4096 宽等）。4K HEVC 悬停解码在弱设备上可能卡，但那由 codec 轴接管（phase-2）。
//  ② fileSize > 10GB：仅作病态大文件兜底。**播放是渐进流式**（起播只读头 + 首个 GOP，不读整
//     文件），故文件大小与起播延迟几乎无关（实测 SSD 上数十 GB 仍秒开）——size 基本是红鲱鱼，
//     阈值故意抬到 10GB，只挡真正异常的巨型文件（如高码率无损）。
// TODO(phase-2)：接 video_meta.video_codec（HEVC/AV1 long-GOP 才是 seek/软解真凶）需给
//   LayoutRowItem 加行列或懒取 IPC，届时把 codec 并入本判据。
// sprite 负结果（无 sprite）重试间隔：TTL 内不重复 IPC，过期后重查——覆盖「悬停过→用户手动
// 触发关键帧提取→sprite 迟到」的场景，scrub 能力在一个 TTL 内恢复而非瘫到重启。
const SPRITE_MISS_TTL_MS = 30_000
// 关键帧 sprite 的帧数兜底默认值（与后端 derive/video.rs::KEYFRAME_COUNT / schema.rs
// video_keyframe_count 键缺省值一致）。小批 C2（2026-07-22）起改由 ui.videoKeyframeCount
// 响应式下发（经 get_startup_config），此常量仅在取不到该值时兜底——不再是恒定生效值，
// 之前用户改 video_keyframe_count 后新雪碧图会被本处硬编码 10 错切，本批修复该缺口。
const KEYFRAME_COUNT_FALLBACK = 10
// 横移死区（px）：悬停播放态下，光标相对进入点的水平位移累计超过此值，才认作「想逐帧看」的
// scrub 意图并切入 scrub。小于此值的微抖不触发——否则光标停在格上的轻微抖动会反复打断正在播放
// 的预览（手势消歧核心：播放要静止、scrub 要移动，两者本质对立）。切入后锁定 scrub 至移出本格，
// 不回切播放，避免 move↔stop 来回频闪。
const SCRUB_DEADZONE_PX = 24

export interface HoverPreviewOptions {
  id: () => number
  mediaType: () => string
  isLivePhoto: () => boolean
  fileSize: () => number
  /** 视频内在分辨率（像素）——判「重」用（面积 > 4K 走 scrub-only，恰好 4K 放行播放）。
   *  注意：DB 值在 enrich 前是扫描占位（视频统一 1280×720，非 0），无法在此辨真伪——超 4K 的
   *  未 enrich 视频会先按不重起播，由 onPreviewMeta 的 loadedmetadata 真实尺寸当场补判降级
   *  （运行时兜底，见 runtimeHeavy）。缺失（如 lab 视图不发尺寸）传 0 即可，同样由兜底接住。 */
  videoWidth: () => number
  videoHeight: () => number
  compact: () => boolean
  isSelectionMode: () => boolean
}

export function useHoverPreview(opts: HoverPreviewOptions) {
  const ui = useUiStore()
  const previewSrc = ref<string>('') // <video> 源（直接播放模式）
  const spriteSrc = ref<string>('') // sprite 图源（scrub 模式；play 态下亦可后台预取待命）
  const scrubFrame = ref<number>(0) // 当前 scrub 帧索引 0..KEYFRAME_COUNT-1
  // 悬停子态：idle 未激活 / play 自动播放 / scrub 逐帧。同一格内 play→scrub 由横移死区触发，
  // 单向不回切（见 SCRUB_DEADZONE_PX）。isPreviewing/isScrubbing 均据此 mode 门控，故 play 态
  // 下即便 spriteSrc 已预取就绪，也因 mode!=='scrub' 而不显 sprite（预取无副作用）。
  const mode = ref<'idle' | 'play' | 'scrub'>('idle')
  // 进入 play 态后首个移动事件记录的光标 X（px，相对格左缘），横移死区判定基准；null=尚未记录。
  let playEntryX: number | null = null
  let timer: ReturnType<typeof setTimeout> | null = null
  // 防止「快速划走」后异步源解析仍然生效。
  let resolveToken = 0

  const isPreviewing = computed(
    () => activeId.value === opts.id() && mode.value === 'play' && !!previewSrc.value,
  )
  const isScrubbing = computed(
    () => activeId.value === opts.id() && mode.value === 'scrub' && !!spriteSrc.value,
  )

  // scrub 背景样式：sprite 为水平条带，按帧索引平移 background-position。
  const scrubStyle = computed(() => {
    const cols = ui.videoKeyframeCount || KEYFRAME_COUNT_FALLBACK
    const posX = cols > 1 ? (scrubFrame.value / (cols - 1)) * 100 : 0
    return {
      backgroundImage: `url("${spriteSrc.value}")`,
      backgroundSize: `${cols * 100}% 100%`,
      backgroundPosition: `${posX}% 0%`,
      backgroundRepeat: 'no-repeat',
    }
  })

  function eligible(): boolean {
    if (!ui.hoverAutoplay) return false
    if (opts.compact()) return false
    if (opts.isSelectionMode()) return false
    return opts.mediaType() === 'video' || opts.isLivePhoto()
  }

  async function resolveVideoSrc(): Promise<string> {
    const id = opts.id()
    const cached = srcCache.get(id)
    if (cached) return cached
    let url: string
    if (opts.mediaType() === 'video') {
      const detail = await invokeIpc<{ absPath: string }>(IPC.GET_MEDIA_DETAIL, { id })
      url = resolveAssetUrl(detail.absPath)
    } else {
      // Apple Live / Google Motion Photo：后端 get_companion_video_url 自动处理两种来源。
      const path = await invokeIpc<string>(IPC.GET_COMPANION_VIDEO_URL, { itemId: id })
      url = resolveAssetUrl(path)
    }
    srcCache.set(id, url)
    return url
  }

  async function resolveSprite(): Promise<string | null> {
    const id = opts.id()
    const cached = spriteCache.get(id)
    if (cached) return cached
    // 负缓存带 TTL：过期后允许重查（sprite 可能已被流水线补产）。IPC 失败不记负——下次 hover 重试。
    const missAt = spriteMissAt.get(id)
    if (missAt !== undefined && Date.now() - missAt < SPRITE_MISS_TTL_MS) return null
    const path = await invokeIpc<string | null>(IPC.GET_KEYFRAME_SPRITE, { itemId: id })
    if (path) {
      const url = resolveAssetUrl(path)
      spriteCache.set(id, url)
      spriteMissAt.delete(id)
      return url
    }
    spriteMissAt.set(id, Date.now())
    return null
  }

  function onEnter() {
    if (!eligible()) return
    if (timer) clearTimeout(timer)
    playEntryX = null
    const myToken = ++resolveToken
    timer = setTimeout(async () => {
      // 防抖窗内条件可能已变（进入选择模式/缩放跌入 compact/设置关闭）——起播前复查资格。
      if (!eligible()) return
      try {
        // 「重」视频：直接走 sprite scrub-only 降级（绝不解码大文件），无 sprite 才回退播放。
        // 判据见上方 HEAVY_VIDEO_* 注释；runtimeHeavy=此前播放实测出的超 4K（DB 占位辨不出的）。
        const heavy =
          opts.mediaType() === 'video' &&
          (opts.videoWidth() * opts.videoHeight() > ui.heavyVideoMaxPixels ||
            opts.fileSize() > ui.heavyVideoMaxBytes ||
            runtimeHeavy.has(opts.id()))
        if (heavy) {
          const sprite = await resolveSprite()
          if (myToken !== resolveToken) return // 已划走
          if (sprite) {
            spriteSrc.value = sprite
            scrubFrame.value = 0
            previewSrc.value = ''
            mode.value = 'scrub'
            activeId.value = opts.id()
            activeLeave = onLeave
            return
          }
          // 无 sprite → 落到下方直接播放
        }
        const url = await resolveVideoSrc()
        if (myToken !== resolveToken) return // 已划走，放弃
        previewSrc.value = url
        spriteSrc.value = ''
        scrubFrame.value = 0
        mode.value = 'play'
        activeId.value = opts.id()
        activeLeave = onLeave
      } catch {
        // 无 companion / 解析失败 → 不预览
      }
    }, ui.hoverDelayMs)
  }

  // 格级横移：x=光标相对格左缘（px），width=格宽（px）。play 态判横移死区决定是否切 scrub；
  // scrub 态把 x 映射为帧索引。由宿主 <video>/格根的 mousemove 驱动（单一入口，见 MediaThumb）。
  function onMove(x: number, width: number) {
    if (activeId.value !== opts.id()) return
    if (mode.value === 'play') {
      if (playEntryX === null) {
        playEntryX = x
        // 播放态首个移动才预取雪碧图待命（供越死区切 scrub 时无空窗）：静止观看的 hover 根本
        // 不会切 scrub，起播即预取会给每次视频悬停白付一次 IPC。仅当仍停在本格且确有 sprite
        // 时落地；划走/换格由 activeId 守卫丢弃；失败静默（下次 hover 由 resolveSprite 重试）。
        if (opts.mediaType() === 'video' && !spriteSrc.value) {
          resolveSprite()
            .then((s) => {
              if (activeId.value === opts.id() && mode.value === 'play' && s) spriteSrc.value = s
            })
            .catch(() => {})
        }
        return
      }
      // 死区未越 / 雪碧图尚未预取就绪 → 维持播放（sprite 未就绪时切过去会白屏，故等就绪）。
      if (Math.abs(x - playEntryX) < SCRUB_DEADZONE_PX || !spriteSrc.value) return
      mode.value = 'scrub' // 锁定 scrub 至移出本格（onLeave 复位），不回切
    }
    if (mode.value === 'scrub') {
      const cols = ui.videoKeyframeCount || KEYFRAME_COUNT_FALLBACK
      const f = Math.min(Math.max(width > 0 ? x / width : 0, 0), 1)
      scrubFrame.value = Math.min(cols - 1, Math.floor(f * cols))
    }
  }

  // 播放起步的 loadedmetadata 带回**真实**分辨率（DB 在 enrich 前只有 1280×720 扫描占位，
  // onEnter 的静态判据对未 enrich 的超 4K 视频必然漏判）。此处当场补判：真超 4K → 记入
  // runtimeHeavy（后续 hover 直进 scrub-only），并立即尝试切 scrub 止损——metadata 阶段只解析
  // 了容器头，抢在持续解码干满前降级。无 sprite 则维持播放（与 onEnter 的 heavy-无-sprite
  // 回退播放同一决策，保持行为一致）。由宿主 <video> 的 @loadedmetadata 驱动。
  function onPreviewMeta(width: number, height: number) {
    if (opts.mediaType() !== 'video') return
    if (width * height <= ui.heavyVideoMaxPixels) return
    const id = opts.id()
    runtimeHeavy.add(id)
    if (activeId.value !== id || mode.value !== 'play') return
    resolveSprite()
      .then((sprite) => {
        if (activeId.value !== id || mode.value !== 'play' || !sprite) return
        spriteSrc.value = sprite
        scrubFrame.value = 0
        previewSrc.value = ''
        mode.value = 'scrub'
      })
      .catch(() => {})
  }

  // 宿主 <video> 加载失败（文件被移动/删除后缓存 URL 变死路径、偶发解码失败）→ 失效本条源缓存
  // 并结束预览；下次 hover 重新经 GET_MEDIA_DETAIL 解析新路径，不会整场吃死一条坏 URL。
  function onPreviewError() {
    srcCache.delete(opts.id())
    if (activeId.value === opts.id()) onLeave()
  }

  function onLeave() {
    resolveToken++ // 作废在途解析
    if (timer) {
      clearTimeout(timer)
      timer = null
    }
    if (activeId.value === opts.id()) activeId.value = null
    if (activeLeave === onLeave) activeLeave = null
    previewSrc.value = ''
    spriteSrc.value = ''
    scrubFrame.value = 0
    mode.value = 'idle'
    playEntryX = null
  }

  onBeforeUnmount(onLeave)
  // KeepAlive 停用（如 MediaGrid 被缓存换出）不触发 unmount，鼠标 leave 也不会来——不清理则
  // activeId 钉死在离场格上，回来后整墙悬停预览失效。stub 组件外无 KeepAlive 时本钩子不注册。
  onDeactivated(onLeave)

  return {
    isPreviewing,
    previewSrc,
    isScrubbing,
    spriteSrc,
    scrubStyle,
    onEnter,
    onLeave,
    onMove,
    onPreviewMeta,
    onPreviewError,
  }
}
