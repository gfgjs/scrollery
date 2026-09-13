import { convertFileSrc } from '@tauri-apps/api/core'

/** 已有受控 URL 直接使用；其余值都按本地文件路径交给 Tauri asset protocol。
 * Unix 绝对路径同样以 `/` 开头，不能把它当成 Web 根路径放行。 */
export function resolveAssetUrl(path: string): string {
  if (/^(?:data:|blob:|https?:\/\/|asset:|tauri:|ipc:)/i.test(path)) return path
  return convertFileSrc(path.replace(/\\/g, '/'))
}
