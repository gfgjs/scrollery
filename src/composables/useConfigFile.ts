// src/composables/useConfigFile.ts
// 外置配置文件(config.toml)—— 外部编辑器打开 / 状态查询 / 热更新事件消费。
//
// 状态是**模块级单例**(而非每次调用各建一份本地 ref):config-file-changed 须全局生效
// (任意页面都要感知配置热更新,不局限于设置页),且设置页与 App 级都要读到同一份
// path/exists/lastError——若各自 ref 互不相通,App 级收到事件也刷新不了设置页正显示的横幅。
// 事件监听同理只装一次:多次调用 useConfigFile() 幂等,不重复 listen(镜像
// settingsPersistence.installSettingsBridge 的「共享注册 Promise」写法)。真正的常驻挂载点在
// App.vue setup(App 根组件不卸载,等价于 App 生命周期)。
//
// 设置值本身不再由此文件刷新:config-file-changed 携带完整快照,由中央服务统一应用
// (settingsPersistence.installSettingsBridge),此后各消费点读到的就是新值——本文件只负责
// 「文件路径/解析错误」这类**文件级**状态,以及在快照到达后重放那点确实需要单独触发的副作用。
import { ref } from 'vue'
import { invokeIpc } from '../utils/ipc'
import { listenAppEvent } from '../utils/appEvents'
import { IPC, EVENTS } from '../constants/ipc'
import { useConfigStore } from '../stores/configStore'
import { useUiStore } from '../stores/uiStore'
import { logger } from '../utils/logger'

/** 配置文件解析错误(get_config_status.last_error 与 config-file-error 事件共用形状)。 */
export interface ConfigFileParseError {
  message: string
  line: number | null
}

/** get_config_status 的返回形状(字段名为后端 IPC 契约字面量,不做 camelCase 转写)。 */
export interface ConfigFileStatus {
  path: string
  exists: boolean
  last_error: ConfigFileParseError | null
}

const status = ref<ConfigFileStatus | null>(null)
const lastError = ref<ConfigFileParseError | null>(null)
// 最近一次外部编辑触及的「非热更新、需重启应用才生效」键(空数组=无待提示)。
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

/** 用系统默认编辑器打开外置配置文件。失败原样抛出,供调用方按需 toast。 */
async function openInEditor() {
  try {
    await invokeIpc(IPC.OPEN_CONFIG_FILE)
  } catch (e) {
    logger.error('Failed to open config file in external editor', { error: e })
    throw e
  }
}

/** 关闭「需重启生效」提示条(用户可关闭)。 */
function dismissRestartRequiredNotice() {
  restartRequiredKeys.value = []
}

/**
 * 外部编辑热更新后的 DOM 侧重放。
 *
 * 设置值已由中央服务应用,但有一类副作用不是「读值即得」,而是「值变化时要推一次」——
 * 首帧主题快照(供 index.html 预着色)与 configStore 的 CSS 变量/悬停类。前者由
 * uiStore 的快照 watch 覆盖(applyAppearance 内写快照),后者由 configStore 的快照 watch 覆盖,
 * 故这里只需确保两个 store 都已实例化、监听已挂上。
 */
function replayHotEffects() {
  // 实例化即挂上各自的快照 watch(uiStore/configStore 的 watch 在 setup 期建立)。
  void useUiStore()
  void useConfigStore()
}

// 共享注册 Promise(而非布尔标记,镜像 scanStore.ensureThumbGenListener):并发/多次调用
// 都等同一次注册完成,不存在「标记已置真、监听尚未挂上」的漏事件窗口期。
let listenersPromise: Promise<unknown> | null = null

function ensureListeners(): Promise<unknown> {
  listenersPromise ??= Promise.all([
    listenAppEvent<{
      keys: string[]
      restart_required: string[]
    }>(EVENTS.CONFIG_FILE_CHANGED, (e) => {
      // 热应用成功:清错误横幅;快照由中央服务应用,这里只更新文件级状态与重启提示。
      lastError.value = null
      restartRequiredKeys.value = e.payload?.restart_required ?? []
      replayHotEffects()
      void refreshStatus()
    }),
    listenAppEvent<ConfigFileParseError>(EVENTS.CONFIG_FILE_ERROR, (e) => {
      // 语法错误:后端保持旧值生效,前端只更新错误展示,不触发 store 刷新(值本就没变)。
      lastError.value = e.payload
    }),
  ])
  return listenersPromise
}

/**
 * 外置配置文件的状态与操作。每次调用:① 确保全局监听已挂(幂等);② 触发一次状态刷新,
 * 使新挂载的组件(如设置页)立即拿到最新路径/错误,不必等下一次事件。
 * @returns 响应式 `{ status, lastError, openInEditor }`;前两者是跨调用共享的同一份 ref。
 */
export function useConfigFile() {
  void ensureListeners()
  void refreshStatus()
  return { status, lastError, openInEditor, restartRequiredKeys, dismissRestartRequiredNotice }
}
