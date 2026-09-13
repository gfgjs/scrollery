// src/composables/useConfigFile.ts
// 外置配置文件（config.toml，批次B前端侧）—— 外部编辑器打开 / 状态查询 / 热更新事件消费。
//
// 状态是**模块级单例**（而非每次调用各建一份本地 ref）：config-file-changed 须全局生效
// （任意页面都要感知配置热更新，不局限于设置页），且设置页与 App 级都要读到同一份
// path/exists/lastError——若各自 ref 互不相通，App 级收到事件也刷新不了设置页正显示的横幅。
// 事件监听同理只装一次：多次调用 useConfigFile() 幂等，不重复 listen（镜像
// scanStore.ensureThumbGenListener 的「共享注册 Promise」写法）。真正的常驻挂载点在
// App.vue setup（App 根组件不卸载，等价于 App 生命周期）。

import { ref } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { listenAppEvent } from '../utils/appEvents'
import { IPC, EVENTS } from '../constants/ipc'
import { useConfigStore } from '../stores/configStore'
import { useUiStore } from '../stores/uiStore'
import { logger } from '../utils/logger'

/** 配置文件解析错误（get_config_status.last_error 与 config-file-error 事件共用形状）。 */
export interface ConfigFileParseError {
  message: string
  line: number | null
}

/** get_config_status 的返回形状（字段名为后端 IPC 契约字面量，不做 camelCase 转写）。 */
export interface ConfigFileStatus {
  path: string
  exists: boolean
  last_error: ConfigFileParseError | null
}

interface ConfigFileChangedPayload {
  keys: string[]
  restart_required: string[]
}

const status = ref<ConfigFileStatus | null>(null)
const lastError = ref<ConfigFileParseError | null>(null)
// 批次B深审 #7:最近一次外部编辑触及的「非热更新、需重启应用才生效」键(空数组=无待提示)。
// 信息级横幅用,与 lastError 分开维护——二者互不清空对方(语法错误横幅与本提示可能同时展示)。
const restartRequiredKeys = ref<string[]>([])

async function refreshStatus() {
  try {
    const s = await invokeIpc<ConfigFileStatus>(IPC.GET_CONFIG_STATUS)
    status.value = s
    lastError.value = s.last_error
  } catch (e) {
    logger.error('Failed to get config file status', { error: e })
  }
}

/** 用系统默认编辑器打开外置配置文件。失败原样抛出，供调用方按需 toast。 */
async function openInEditor() {
  try {
    await invokeIpc(IPC.OPEN_CONFIG_FILE)
  } catch (e) {
    logger.error('Failed to open config file in external editor', { error: e })
    throw e
  }
}

/** 关闭「需重启生效」提示条（批次B深审 #7，用户可关闭）。 */
function dismissRestartRequiredNotice() {
  restartRequiredKeys.value = []
}

// 批次B深审 #8:外部编辑热键的前端副作用定向重放。只对「能不写回后端地单独触发」的两类
// 效果做定向重放，其余（thumb_size/thumb_webp_quality/enable_video_cover/
// enable_video_keyframes 的派生控件耦合逻辑）与 configStore 里带 saveConfig 写回的 setter
// 拆不开——硬拆会在这条外部编辑热更新路径上把值又写回 config.toml，形成假循环，故不拆，
// 改走 restartRequiredKeys 提示条让用户去设置页手动重新触发派生。
const DERIVATION_RESTART_KEYS = new Set([
  'thumb_size',
  'thumb_webp_quality',
  'enable_video_cover',
  'enable_video_keyframes',
  'ai_hq_cache_enabled',
])

async function replayHotEffects(changedKeys: string[]) {
  if (changedKeys.includes('language')) {
    try {
      // uiStore.applyLanguage 只应用（切 i18n locale + <html lang>），不含
      // invokeIpc(SET_APP_CONFIG) 写回；uiStore.setLanguage 会写回，此处绝不能用它，
      // 否则「外部编辑器刚改的值」被立即回写一遍，形成假循环。
      const lang = await invokeIpc<string | null>(IPC.GET_APP_CONFIG, { key: 'language' })
      if (lang) useUiStore().applyLanguage(lang)
    } catch (e) {
      logger.error('Failed to reapply language after external config edit', { error: e })
    }
  }
  if (changedKeys.some((k) => DERIVATION_RESTART_KEYS.has(k))) {
    // configStore.restartDerivation 是独立动作（只 invoke start_derivation，不写配置），
    // 与 setThumbSize 等耦合 setter 不同，可安全单独调用，使新档位/开关立即应用到派生流水线
    // （后端 apply_setting_effects 已在 watcher 回调里把受影响项复位为 pending，见 lib.rs）。
    void useConfigStore().restartDerivation()
  }
}

// 共享注册 Promise（而非布尔标记，镜像 scanStore.ensureThumbGenListener）：并发/多次调用
// 都等同一次注册完成，不存在「标记已置真、监听尚未挂上」的漏事件窗口期。
let listenersPromise: Promise<unknown> | null = null

function ensureListeners(): Promise<unknown> {
  listenersPromise ??= Promise.all([
    listenAppEvent<ConfigFileChangedPayload>(EVENTS.CONFIG_FILE_CHANGED, (e) => {
      // 热应用成功：清错误横幅，复用既有加载路径刷新两个 store（不新写并行取数逻辑）。
      // 常规刷新只读取配置；旧主题迁移会补写一次统一的明暗配对，值一致后不再写回。
      lastError.value = null
      restartRequiredKeys.value = e.payload.restart_required
      void useConfigStore().refreshFromBackend()
      void useUiStore().refreshFromBackend()
      void refreshStatus()
      void replayHotEffects(e.payload.keys)
    }),
    listenAppEvent<ConfigFileParseError>(EVENTS.CONFIG_FILE_ERROR, (e) => {
      // 语法错误：后端保持旧值生效，前端只更新错误展示，不触发 store 刷新（值本就没变）。
      lastError.value = e.payload
    }),
  ])
  return listenersPromise
}

/**
 * 外置配置文件的状态与操作。每次调用：① 确保全局监听已挂（幂等）；② 触发一次状态刷新，
 * 使新挂载的组件（如设置页）立即拿到最新路径/错误，不必等下一次事件。
 * @returns 响应式 `{ status, lastError, openInEditor }`；前两者是跨调用共享的同一份 ref。
 */
export function useConfigFile() {
  void ensureListeners()
  void refreshStatus()
  return { status, lastError, openInEditor, restartRequiredKeys, dismissRestartRequiredNotice }
}
