import { defineStore } from 'pinia'
import { invokeIpc } from '../utils/ipc'
import { IPC } from '../constants/ipc'
import {
  applyUiFontSize,
  applyTimelineScrollWidth,
  applyTimelineAxisWidth,
  applyAxisViewportOpacity,
} from '../utils/uiScale'
import { logger, setLoggerEnabled } from '../utils/logger'

export const useConfigStore = defineStore('config', {
  state: () => ({
    thumbSkipMaxKb: 200,
    // 缓存 LRU 预算默认 10GB(2026-07-19 裁决:不做访问追踪,放宽预算降驱逐频率;
    // 与后端 DEFAULT_THUMB_CACHE_MAX_MB 同值)。
    thumbCacheMaxMb: 10240,
    thumbSize: 512,
    // 缩略图 WebP 编码质量:1..=99 有损(默认 80),100=无损。变更后后端会把存量已生成项
    // 与封面派生一并复位,按需以新质量重生成。
    thumbWebpQuality: 80,
    timelineScrollWidth: 8,
    timelineAxisWidth: 44,
    // 滚动条 thumb 与时间轴视窗共享的最小高度(px):二者同高、同 thumbGeometry 几何。纯 JS 数值,
    // 经 MediaGrid 以 prop 下发(非 CSS 变量);默认 48。
    scrollThumbMinHeight: 48,
    // 轴视窗不透明度缩放(百分比,100=默认):时间轴/minimap 半透明拖动视窗共享,经 CSS 变量
    // --axis-viewport-opacity(pct/100 无量纲乘数)下发。范围 20–200(上限保 color-mix ≤100%)。
    axisViewportOpacity: 100,
    uiFontSize: 13,
    enableHoverScale: true,
    logLevel: 'info',
    thumbStrategy: 'cpu',
    gpuEngine: 'wic',
    aiProviderOverride: 'auto',
    aiBatchSize: 0,
    // AI 模型下载首选源：'official'=官方 HuggingFace 优先，'mirror'=国内镜像 hf-mirror.com 优先。
    aiDownloadSource: 'official',
    // 视频派生开关：是否提取视频封面 / 关键帧雪碧图（默认开启）。
    enableVideoCover: true,
    enableVideoKeyframes: true,
    // AI 高清缓存开关（opt-in，默认关）：开启后后台静默为每张图生成短边≥336 的 WebP 缓存，
    // 使 CLIP 分析解码该小缓存而非全分辨率原图，大幅降低分析时的 CPU 占用。
    aiHqCache: false,
    // 查看器渲染色域(自定义 ICC 与色域切换,方案 B §0⑤):target 枚举 srgb/display-p3/dci-p3/custom,
    // 默认 srgb(=零派生,直显原图);custom 时 customId 指向已导入 ICC 的 16-hex id。
    viewerColorTarget: 'srgb',
    viewerColorCustomId: '',
    // OCR 文字提取（B′ 路线,T10）:当前使用中的档位 id,默认标准档 pp-ocrv5-mobile。
    ocrTier: 'pp-ocrv5-mobile',
    isLoaded: false,
  }),

  actions: {
    async loadConfig() {
      if (this.isLoaded) return
      try {
        const fetchInt = async (key: string, defaultVal: number) => {
          const val = await invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key })
          return val ? parseInt(val, 10) : defaultVal
        }
        const fetchStr = async (key: string, defaultVal: string) => {
          const val = await invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key })
          return val ? val : defaultVal
        }
        const fetchBool = async (key: string, defaultVal: boolean) => {
          const val = await invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key })
          return val ? val === 'true' : defaultVal
        }

        this.thumbSkipMaxKb = await fetchInt('thumb_skip_max_kb', 200)
        this.thumbCacheMaxMb = await fetchInt('thumb_cache_max_mb', 10240)
        this.thumbSize = await fetchInt('thumb_size', 512)
        this.thumbWebpQuality = await fetchInt('thumb_webp_quality', 80)
        this.timelineScrollWidth = await fetchInt('timeline_scroll_width', 8)
        this.timelineAxisWidth = await fetchInt('timeline_axis_width', 44)
        this.scrollThumbMinHeight = await fetchInt('scroll_thumb_min_height', 48)
        this.axisViewportOpacity = await fetchInt('axis_viewport_opacity', 100)
        this.uiFontSize = await fetchInt('ui_font_size', 13)
        this.enableHoverScale = await fetchBool('enable_thumb_hover_scale', true)
        this.logLevel = await fetchStr('log_level', 'info')
        // off 档联动(方案 §4):启动时按已加载的 logLevel 同步前端队列开关态。
        setLoggerEnabled(this.logLevel !== 'off')
        this.thumbStrategy = await fetchStr('thumb_strategy', 'cpu')
        this.gpuEngine = await fetchStr('gpu_engine', 'wic')
        this.aiProviderOverride = await fetchStr('ai_provider_override', 'auto')
        this.aiBatchSize = await fetchInt('ai_batch_size', 0)
        this.aiDownloadSource = await fetchStr('ai_download_source', 'official')
        this.enableVideoCover = await fetchBool('enable_video_cover', true)
        this.enableVideoKeyframes = await fetchBool('enable_video_keyframes', true)
        this.aiHqCache = await fetchBool('ai_hq_cache_enabled', false)
        this.viewerColorTarget = await fetchStr('viewer_color_target', 'srgb')
        this.viewerColorCustomId = await fetchStr('viewer_color_custom_id', '')
        this.ocrTier = await fetchStr('ocr_active_tier', 'pp-ocrv5-mobile')

        // 应用一些 CSS 变量（thumbStrategy/gpuEngine 单一来源即本 store，无需再镜像至 uiStore）。
        document.documentElement.style.setProperty(
          '--scrollbar-width',
          `${this.timelineScrollWidth}px`,
        )
        document.documentElement.style.setProperty(
          '--timeline-axis-width',
          `${this.timelineAxisWidth}px`,
        )
        applyAxisViewportOpacity(this.axisViewportOpacity)

        applyUiFontSize(this.uiFontSize)

        if (this.enableHoverScale) {
          document.documentElement.classList.remove('disable-hover-scale')
        } else {
          document.documentElement.classList.add('disable-hover-scale')
        }

        this.isLoaded = true
      } catch (e) {
        logger.error('Failed to load config', { error: e })
      }
    },

    async saveConfig(key: string, value: string) {
      await invokeIpc(IPC.SET_APP_CONFIG, { key, value })
    },

    // 外置配置文件（config.toml，批次B）热更新到达时刷新（composables/useConfigFile.ts 消费）：
    // 复用 loadConfig 的既有取值路径，不新写并行取数逻辑。loadConfig 首行有 isLoaded 幂等短路，
    // 这里先复位再调用以强制重新走一遍——只 get 不 set，绕开任何 setXxx 的 SET_APP_CONFIG 写回，
    // 避免「外部编辑器改的值」被内存里的旧值立即覆盖写回、形成假循环。
    async refreshFromBackend() {
      this.isLoaded = false
      await this.loadConfig()
    },

    async setThumbSkipMaxKb(val: number) {
      this.thumbSkipMaxKb = val
      await this.saveConfig('thumb_skip_max_kb', val.toString())
    },
    async setThumbCacheMaxMb(val: number) {
      this.thumbCacheMaxMb = val
      await this.saveConfig('thumb_cache_max_mb', val.toString())
    },
    async setThumbSize(val: number) {
      this.thumbSize = val
      await this.saveConfig('thumb_size', val.toString())
      // 后端已把存量项+封面派生行复位(⑧),重启派生流水线使 video/audio/epub 封面
      // 立即按新档位重生成(与 setThumbWebpQuality 同构;pdf/svg 由 DocThumbRenderer 泵接续)。
      await this.restartDerivation()
    },
    // 编码质量(1-100,100=无损)。后端 set_app_config 已把存量项+封面派生复位;
    // 这里再重启派生流水线,使 video/audio/epub 封面立即按新质量重生成
    // (pdf/svg 由 DocThumbRenderer 泵随 media_enriched 事件自行接续)。
    async setThumbWebpQuality(val: number) {
      const clamped = Math.min(100, Math.max(1, Math.round(val)))
      this.thumbWebpQuality = clamped
      await this.saveConfig('thumb_webp_quality', clamped.toString())
      await this.restartDerivation()
    },
    async setTimelineScrollWidth(val: number) {
      this.timelineScrollWidth = val
      await this.saveConfig('timeline_scroll_width', val.toString())
      applyTimelineScrollWidth(val)
    },
    async setTimelineAxisWidth(val: number) {
      this.timelineAxisWidth = val
      await this.saveConfig('timeline_axis_width', val.toString())
      applyTimelineAxisWidth(val)
    },
    async setScrollThumbMinHeight(val: number) {
      // 纯 JS 数值(非 CSS 变量):写库即可,响应式 state 变化经 MediaGrid prop 实时下发到组件。
      this.scrollThumbMinHeight = val
      await this.saveConfig('scroll_thumb_min_height', val.toString())
    },
    async setAxisViewportOpacity(val: number) {
      // 先收敛再落库,与 applyAxisViewportOpacity 的收敛区间一致(20–200)。
      const clamped = Math.min(200, Math.max(20, Math.round(val)))
      this.axisViewportOpacity = clamped
      await this.saveConfig('axis_viewport_opacity', clamped.toString())
      applyAxisViewportOpacity(clamped)
    },
    async setUiFontSize(val: number) {
      this.uiFontSize = val
      await this.saveConfig('ui_font_size', val.toString())
      // 单一事实源(P1-18):与 App.vue 启动应用共用同一函数,消除 1px 漂移。
      applyUiFontSize(val)
    },
    async setEnableHoverScale(val: boolean) {
      this.enableHoverScale = val
      await this.saveConfig('enable_thumb_hover_scale', val.toString())
      if (val) {
        document.documentElement.classList.remove('disable-hover-scale')
      } else {
        document.documentElement.classList.add('disable-hover-scale')
      }
    },
    async setLogLevel(val: string) {
      this.logLevel = val
      // 切换态同步(方案 §4):先切前端队列开关,off 时立即清空在途队列,不等 saveConfig 落库往返。
      setLoggerEnabled(val !== 'off')
      await this.saveConfig('log_level', val)
    },
    async setThumbStrategy(val: string) {
      this.thumbStrategy = val
      await this.saveConfig('thumb_strategy', val)
    },
    async setGpuEngine(val: string) {
      this.gpuEngine = val
      await this.saveConfig('gpu_engine', val)
    },
    async setAiProviderOverride(val: string) {
      this.aiProviderOverride = val
      await this.saveConfig('ai_provider_override', val)
    },
    async setAiBatchSize(val: number) {
      this.aiBatchSize = val
      await this.saveConfig('ai_batch_size', val.toString())
    },
    async setAiDownloadSource(val: string) {
      this.aiDownloadSource = val
      await this.saveConfig('ai_download_source', val)
    },
    async setEnableVideoCover(val: boolean) {
      this.enableVideoCover = val
      await this.saveConfig('enable_video_cover', val.toString())
      await this.restartDerivation()
    },
    async setEnableVideoKeyframes(val: boolean) {
      this.enableVideoKeyframes = val
      await this.saveConfig('enable_video_keyframes', val.toString())
      await this.restartDerivation()
    },
    // AI 高清缓存开关：开启 → 重启派生流水线，后台静默 backfill 并生成 ai_thumb 缓存；
    // 关闭 → 重启后生产者排除 ai_thumb（已生成的缓存仍保留并被分析复用）。
    async setAiHqCache(val: boolean) {
      this.aiHqCache = val
      await this.saveConfig('ai_hq_cache_enabled', val.toString())
      await this.restartDerivation()
    },
    // 重启派生流水线，使新开关立即生效：开启 → 接续该 kind 的待处理项；
    // 关闭 → 重启后生产者会排除该 kind（在途任务恢复为待处理并暂停）。失败静默（仅尽力而为）。
    // 查看器渲染色域(方案 B §0⑤/⑥):不调 restartDerivation——viewer_color 不是派生流水线 kind,
    // 换值只影响下次 GET_VIEWER_COLOR_URL 的 target 解析,useViewerColorSource 的 watch 会自行感知。
    // 次序契约:先 await 持久化、后改本地 state——GET_VIEWER_COLOR_URL 后端从 ConfigManager
    // 单源读 target(不收前端传参),而本地 state 一变 useViewerColorSource 的 watch 即发渲染
    // IPC;若先改本地,渲染请求与 set_config 竞速,偶发按旧值渲染(切换不生效)。保存失败时
    // 本地不变,UI 与后端保持一致。
    async setViewerColorTarget(val: string) {
      await this.saveConfig('viewer_color_target', val)
      this.viewerColorTarget = val
    },
    async setViewerColorCustomId(val: string) {
      await this.saveConfig('viewer_color_custom_id', val)
      this.viewerColorCustomId = val
    },
    // OCR 档位切换(T10):照 setAiDownloadSource 全套三点姿态(state/load/setter)。
    async setOcrTier(val: string) {
      this.ocrTier = val
      await this.saveConfig('ocr_active_tier', val)
    },
    async restartDerivation() {
      try {
        await invokeIpc(IPC.START_DERIVATION)
      } catch (e) {
        logger.warn('Failed to restart derivation pipeline after toggle', { error: e })
      }
    },
  },
})
