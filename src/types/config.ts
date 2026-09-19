// src/types/config.ts
// 中央设置的 IPC 契约类型(设置集中保存,2026-09-16)。
//
// 唯一真源在后端 config.toml;前端只持一份快照(规范文本值 + 单调序号),不自行持久化偏好。
// 结构类设置(内联表/数组)在 IPC 上一律是**规范 JSON 文本**,由消费方经
// composables/settingsValues 的具名转换读取,禁止各组件自行拼 JSON。

/**
 * 全部已注册设置的生效值快照。
 *
 * - values:键 → 规范文本(布尔恒 true/false、数值十进制、枚举字面量、结构类为规范 JSON 文本)。
 * - revision:后端每次提交递增,用于丢弃过期响应与重复事件。
 * - generation:仅在「恢复默认设置」时递增;写入必须携带当前代次,旧代次一律被拒绝,
 *   防止重置后其他窗口的迟到回写把默认值覆盖回旧偏好。
 */
export interface SettingsSnapshot {
  values: Record<string, string>
  revision: number
  generation: number
}

/**
 * 一次配置提交(UI 批量写入 / 外部编辑热应用 / 恢复默认)的统一回执。
 *
 * keys 是本次真正变化的键;restart_required 是已保存但需重启应用才生效的键;
 * apply_failed 是「文件已保存、运行期应用失败」的键——三者都如实回传,不把部分成功
 * 报成完全成功。后两者沿用后端字面量命名(与 config-file-changed 事件一致),不做转写。
 */
export interface SettingsChange {
  snapshot: SettingsSnapshot
  keys: string[]
  restart_required: string[]
  apply_failed: string[]
}

/** 启动批里的内部状态(留 DB 的业务标记,不属用户设置)。 */
export interface StartupState {
  /** 首启向导标记:仅 'false'(用户完成或跳过过引导)才抑制向导。 */
  firstLaunch: string | null
  /** 详细引导手册已读标记。 */
  guideSeen: string | null
}

/**
 * get_startup_config 的载荷:设置快照与内部状态一次往返取回。
 * 启动只用这一次,不在快照刷新时重放 firstLaunch/guideSeen(重置设置不得重放引导)。
 */
export interface StartupPayload {
  settings: SettingsSnapshot
  state: StartupState
}

/** 设置类键的规范布尔文本。 */
export const SETTING_TRUE = 'true'
export const SETTING_FALSE = 'false'
