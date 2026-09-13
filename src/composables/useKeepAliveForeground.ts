// KeepAlive「前台/后台」生命周期编排(2026-09-12 冷启动选择态 ESC 失效治本)。
//
// 问题:被 <KeepAlive> 保活的组件若本身是异步子树(`defineAsyncComponent`、路由壳内的延迟组件),
// 冷启动首拍只收到 mounted,**收不到 activated**。Vue 只在挂载的 vnode 带
// COMPONENT_SHOULD_KEEP_ALIVE 时才补发激活钩子(见 runtime-core 的 mountComponent),而异步
// wrapper 每拍新建的内层 vnode 不带该标志;异步子树又晚于 KeepAlive 的 activate() 落地,
// 于是首拍激活整体缺失,直到一次异组件路由往返才补上。
//
// 解法:把「进入前台」做成幂等装配,并让 mounted 首拍就走同一入口;另用「与挂载同拍」标记区分
// 首次装配与真实复激活——复激活才需要做几何/焦点/滚动位恢复(mounted 侧有它自己的首算恢复路径,
// 两者都做即双恢复)。
import { onActivated, onBeforeUnmount, onDeactivated, onMounted } from 'vue'

export interface KeepAliveForegroundOptions {
  /**
   * 前台判据:为 false 时组件在屏但不接管前台职责(如 `/view` 覆盖层存续期,或异步子树在其它
   * 路由后台 resolve)。判据须覆盖路由语义本身,不能只看「覆盖层是否在」——否则后台 resolve
   * 会把文档级监听挂到当前并不呈现画廊的页面上。
   */
  isForeground: () => boolean
  /** 进入前台:幂等的运行时装配(文档级监听 / 轴控件门控 / 性能基准等)。 */
  enter: () => void
  /** 进入后台或失活:摘除前台副作用。 */
  leave: () => void
  /** 真实复激活(非与挂载同拍的那次):几何/焦点/滚动位恢复归调用方。 */
  onReactivate: () => void
}

export interface KeepAliveForeground {
  /** 进入前台:组件外触发路径(覆盖层撤下)与激活路径共用的同一入口。 */
  enterForeground: () => void
  /**
   * 后台摘除入口:组件外触发路径(如 `/view` 覆盖层进入的 sync watcher)与失活路径共用的幂等出口。
   */
  enterBackground: () => void
}

export function useKeepAliveForeground(options: KeepAliveForegroundOptions): KeepAliveForeground {
  // 「与本次挂载同拍的首个 activated」标记。异步子树缺激活时为 false 且永不置位,此时装配由
  // mounted 首拍负责;同步子树的首个激活与 mounted 同拍,也只补装配、不恢复。
  let pairedWithMount = false

  // enter/leave 不加去重状态:装配动作本身幂等(同函数同目标的 add/removeEventListener 是空操作),
  // 而记状态会在「覆盖层路径直接装配 → 后续失活」时错判成已装配、跳过摘除,反而漏监听。
  function enterForeground(): void {
    pairedWithMount = false
    options.enter()
  }

  function enterBackground(): void {
    pairedWithMount = false
    options.leave()
  }

  onMounted(() => {
    if (!options.isForeground()) {
      // 初始即后台(深链 `/view`、或异步子树在异组件路由后台 resolve):后台状态必须落地,
      // 否则在屏标志仍是初值,后台的 compute/焦点收回会误跑。
      enterBackground()
      return
    }
    // 装配必须在挂载同步前缀完成:异步子树的 activated 可能迟到或根本不来。
    options.enter()
    pairedWithMount = true
  })

  onActivated(() => {
    if (!options.isForeground()) {
      enterBackground()
      return
    }
    const paired = pairedWithMount
    pairedWithMount = false
    options.enter()
    if (!paired) options.onReactivate()
  })

  onDeactivated(enterBackground)
  // 真·卸载也走同一摘除出口(KeepAlive 缓存整体销毁时 deactivated 不保证先行)。
  onBeforeUnmount(enterBackground)

  return { enterForeground, enterBackground }
}
