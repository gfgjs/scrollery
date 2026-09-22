import { open } from '@tauri-apps/plugin-shell'

/** 只接受后端给出的购买地址；格式检查同时保护过期前端状态。 */
export function purchaseUrl(raw: string | null): string | null {
  if (!raw) return null
  try {
    const url = new URL(raw)
    const host = url.hostname
    if (
      url.protocol !== 'https:' || url.username || url.password ||
      host === 'localhost' || host.endsWith('.localhost') ||
      host.includes(':') || /^[\d.]+$/.test(host) ||
      ['invalid', 'test', 'example', 'example.com', 'example.net', 'example.org'].some(
        (suffix) => host === suffix || host.endsWith(`.${suffix}`),
      )
    ) return null
    return url.href
  } catch {
    return null
  }
}

/** 在系统浏览器中打开有效购买地址。 */
export async function openPurchase(raw: string | null): Promise<void> {
  const url = purchaseUrl(raw)
  if (url) await open(url)
}
