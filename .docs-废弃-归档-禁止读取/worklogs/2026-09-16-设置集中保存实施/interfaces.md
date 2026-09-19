---
status: snapshot
type: design
created: 2026-09-16
---

# 本轮实施接口与写集

## 统一 IPC

```ts
interface SettingsSnapshot {
  values: Record<string, string>
  revision: number
  generation: number
}
interface SettingsChange {
  snapshot: SettingsSnapshot
  keys: string[]
  restart_required: string[]
  apply_failed: string[]
}
interface StartupPayload {
  settings: SettingsSnapshot
  state: { firstLaunch: string | null; guideSeen: string | null }
}
```

- get_settings_snapshot() → SettingsSnapshot。
- get_startup_config() → StartupPayload，一次共享启动往返。
- set_app_settings({patch: Record<string,string>, generation:number}) → SettingsChange。
- clear_settings() → SettingsChange；不访问 DB 清表。
- config-file-changed → SettingsChange（包含完整快照）。
- values 保持规范文本；结构值在 IPC 使用规范 JSON，磁盘为原生 TOML 数组/内联表。
- generation 仅重置递增，旧 generation patch 拒绝；revision 每次变化递增，丢弃晚到快照。二者只在进程内使用，没有迁移/兼容版本。

## 前端共享 API

`src/stores/settingsPersistence.ts`：settingsReady、settingsValues（readonly shallowRef）、initializeSettings、readSetting、writeSettings(patch,{debounce?})、flushSettings、resetSettings、onSettingsApplied。

初始化前的本地默认只呈现，不能自动保存。水合/重置/外部修改只应用；用户交互才提交。高频前端合并300ms/max1s；后端窗口500ms/max2s。

## 排他写集与代理

| 代理 | 范围 | ID |
|---|---|---|
| Lagrange | 后端 config 核心（除 window.rs）、config_commands/scan clear移出/registry/必要State字段 | 01a0a6a3-4dbf-7473-8eb0-32874625cd2d |
| Schrodinger | settingsPersistence、uiStore/configStore、配置types、IPC、useConfigFile、App启动、DynamicSettingControl、语言文案 | 01a0a6a3-4e7d-7cb3-b494-001e00fd00c4 |
| Planck | 其余 src 偏好消费者及局部测试，保留演示打码现行功能 | 01a0a6a3-4f4a-7053-9c3d-ac5192bfc7c3 |
| Linnaeus | window.rs、lib/lifecycle、system exit/log窗口、window-state依赖/锁文件/default capability、useSettingsLifecycle/CloseConfirmDialog | 01a0a6a4-5830-7c70-868b-cf6ebd2e0cee |

主会话负责设计裁定、关键审查、验证编排与文档；不接手复杂编码。Cargo相关验证集中串行；前端局部测试可按排他文件验证，全局typecheck集中整合后一次执行。禁止安装、升级、全量构建和修改真实设置。
