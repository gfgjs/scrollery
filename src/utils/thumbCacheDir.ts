// src/utils/thumbCacheDir.ts
// 缩略图缓存根目录单源:此前 MediaGrid / personStore / ContentViewer / HGalleryLab / SettingsView
// 各自 IPC 取一份 + 各自反斜杠归一,同一常量五份私有拷贝、五处漂移面。统一为模块级缓存的惰性
// getter:首个调用者发 IPC,并发调用共享同一 inflight,成功后整场复用;失败清 inflight 下次重试
// (调用方按各自既有惯例 catch 降级)。目录本身进程内不变(config_commands 读内存配置),缓存安全。
// 注:SettingsView 的展示取数有意不迁——保留 OS 原生反斜杠展示形态,归一化 getter 只服务
// URL/签名类消费方。其「更改缓存目录」流程在写配置成功后调 setThumbCacheDir() 同步本模块缓存,
// 防其他消费方读到旧值。

import { invokeIpc } from './ipc'
import { IPC } from '../constants/ipc'

let cached: string | null = null
let inflight: Promise<string> | null = null
// 单调 epoch:setThumbCacheDir 时自增,作废在途 IPC 结果,防旧值竞态冲写新值。
let epoch = 0

/** 取归一化(正斜杠)的缩略图缓存根目录;整场缓存,失败向调用方抛出且不缓存失败。 */
export async function getThumbCacheDir(): Promise<string> {
  if (cached) return cached
  if (!inflight) {
    const e = ++epoch
    inflight = invokeIpc<string>(IPC.GET_THUMB_CACHE_DIR)
      .then((dir) => {
        const norm = dir.replace(/\\/g, '/')
        // epoch 已变(期间发生过 setThumbCacheDir)则不冲写,返回当前真值。
        if (e === epoch) cached = norm
        return cached ?? norm
      })
      .finally(() => {
        inflight = null
      })
  }
  return inflight
}

/**
 * SettingsView「更改缓存目录」在写配置成功后调用:直写模块缓存并作废在途请求,
 * 保证其他消费方 getThumbCacheDir() 读到新值而非旧缓存。返回归一化(正斜杠)后的值。
 */
export function setThumbCacheDir(dir: string): string {
  epoch++
  cached = dir.replace(/\\/g, '/')
  inflight = null
  return cached
}
