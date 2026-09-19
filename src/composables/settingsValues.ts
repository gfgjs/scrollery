// src/composables/settingsValues.ts
// 中央设置值的轻量读取层(设置集中保存,批次B):把 readSetting 的规范文本收敛为具名类型。
//
// 为什么单独一层:结构类设置(内联表/数组)在 IPC 上是规范 JSON 文本,若各消费方各写一遍
// JSON.parse 与字段校验,口径会漂。此处只做「规范文本 → 类型」的最小转换,不承担持久化
// (写入一律走 settingsPersistence.writeSettings)。
//
// 文本口径与后端 schema 一致:布尔恒 'true'/'false';数值为十进制文本;枚举为选项字面量;
// 数组/内联表为规范 JSON 文本。缺键或非法文本一律回落调用方给定的默认值。

import { readSetting } from '../stores/settingsPersistence'

/** 读布尔设置;缺键回落 fallback(仅 'true' 视为真,与 schema 的规范文本一致)。 */
export function readSettingBool(key: string, fallback: boolean): boolean {
  const raw = readSetting(key)
  if (raw === undefined) return fallback
  return raw === 'true'
}

/** 读数值设置;缺键或非有限数回落 fallback。 */
export function readSettingNumber(key: string, fallback: number): number {
  const raw = readSetting(key)
  if (raw === undefined) return fallback
  const n = Number(raw)
  return Number.isFinite(n) ? n : fallback
}

/** 读枚举设置;缺键或不在候选集内回落 fallback。 */
export function readSettingEnum<T extends string>(
  key: string,
  allowed: readonly T[],
  fallback: T,
): T {
  const raw = readSetting(key)
  return raw !== undefined && (allowed as readonly string[]).includes(raw) ? (raw as T) : fallback
}

/** 把结构设置的规范 JSON 文本解析为 T;空值或非法 JSON 回落 fallback(由调用方判形状)。 */
export function parseSettingJson<T>(raw: string | undefined, fallback: T): T {
  if (!raw) return fallback
  try {
    return JSON.parse(raw) as T
  } catch {
    return fallback
  }
}
