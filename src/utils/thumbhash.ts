// src/utils/thumbhash.ts
// ThumbHash 解码：将大约 28 字节的哈希转换为 32×32 数据 URL 以用作模糊占位符。
// 使用 thumbhash JS 算法（Rust thumbhash crate 的移植）。

/** 将 ThumbHash 字节数组解码为 data: URL（PNG 或类似格式）。`hash` 是一个作为序列化 BLOB 从 Rust 接收的 `number[]`。 */
export function thumbhashToDataURL(hash: number[] | Uint8Array): string {
  // canvas 渲染依赖 DOM;非浏览器环境(如 node 单测)直接空串,解码纯函数另行可测
  if (typeof document === 'undefined') return ''
  const bytes = hash instanceof Uint8Array ? hash : new Uint8Array(hash)

  // thumbhash 解码 — 官方 thumbhash 算法的内联实现
  // See: https://github.com/evanw/thumbhash
  const rgba = thumbHashToRGBA(bytes)
  if (!rgba) return ''

  const canvas = document.createElement('canvas')
  canvas.width = rgba.w
  canvas.height = rgba.h

  const ctx = canvas.getContext('2d')
  if (!ctx) return ''

  const imageData = ctx.createImageData(rgba.w, rgba.h)
  imageData.data.set(rgba.rgba)
  ctx.putImageData(imageData, 0, 0)

  return canvas.toDataURL('image/png')
}

// ── ThumbHash 解码(官方实现的忠实移植) ──────────────────────────────────
// 算法与常量逐行对齐 evanw/thumbhash(MIT)及后端编码所用的 thumbhash crate
// (同算法的官方 Rust 移植);正确性由 thumbhash.spec.ts 以 Rust 编码器+解码器
// 产出的跨语言金标(thumbhash.golden.ts)逐通道对拍锁定。

/** 解码结果:w×h 的 RGBA 像素(非预乘 alpha)。 */
export interface ThumbHashImage {
  w: number
  h: number
  rgba: Uint8Array
}

/**
 * 将 ThumbHash 解码为原始 RGBA 像素(纯函数,不依赖 DOM,可单测)。
 * 对畸形 / 截断输入返回 null(Rust 侧同场景返回 Err)。
 */
export function thumbHashToRGBA(hash: Uint8Array): ThumbHashImage | null {
  if (hash.length < 5) return null

  // ── 头部常量:3 字节 header24 + 2 字节 header16(小端) ──
  const header24 = hash[0] | (hash[1] << 8) | (hash[2] << 16)
  const header16 = hash[3] | (hash[4] << 8)
  const lDC = (header24 & 63) / 63
  const pDC = ((header24 >> 6) & 63) / 31.5 - 1
  const qDC = ((header24 >> 12) & 63) / 31.5 - 1
  const lScale = ((header24 >> 18) & 31) / 31
  const hasAlpha = header24 >> 23 !== 0
  const pScale = ((header16 >> 3) & 63) / 63
  const qScale = ((header16 >> 9) & 63) / 63
  const isLandscape = header16 >> 15 !== 0
  // 亮度通道 DCT 项数:长边固定 7(带 alpha 时 5),短边写在 header16 低 3 位
  const lMax = hasAlpha ? 5 : 7
  const lMin = header16 & 7
  const lx = Math.max(3, isLandscape ? lMax : lMin)
  const ly = Math.max(3, isLandscape ? lMin : lMax)

  let aDC = 1
  let aScale = 1
  let acPos = 5 // AC 系数 nibble 流的起始字节
  if (hasAlpha) {
    if (hash.length < 6) return null
    aDC = (hash[5] & 15) / 15
    aScale = (hash[5] >> 4) / 15
    acPos = 6
  }

  // ── AC 系数:4-bit nibble 流,跨 L/P/Q/A 四通道连续,低半字节在前 ──
  let nibbleIdx = 0
  let truncated = false
  const readNibble = (): number => {
    const byte = hash[acPos + (nibbleIdx >> 1)]
    if (byte === undefined) {
      truncated = true
      return 0
    }
    const v = nibbleIdx & 1 ? byte >> 4 : byte & 15
    nibbleIdx++
    return v
  }
  // 系数遍历顺序(cx*ny < nx*(ny-cy) 的三角区)必须与编码端严格一致
  const decodeChannel = (nx: number, ny: number, scale: number): number[] => {
    const ac: number[] = []
    for (let cy = 0; cy < ny; cy++) {
      for (let cx = cy ? 0 : 1; cx * ny < nx * (ny - cy); cx++) {
        ac.push((readNibble() / 7.5 - 1) * scale)
      }
    }
    return ac
  }
  // 官方实现对 P/Q 色度 AC 乘 1.25,补偿量化造成的饱和度损失
  const lAC = decodeChannel(lx, ly, lScale)
  const pAC = decodeChannel(3, 3, pScale * 1.25)
  const qAC = decodeChannel(3, 3, qScale * 1.25)
  const aAC = hasAlpha ? decodeChannel(5, 5, aScale) : []
  if (truncated) return null

  // ── 输出尺寸:长边 32,短边按近似宽高比缩放(比值取自未钳位的 lMax/lMin) ──
  const ratio = (isLandscape ? lMax : lMin) / (isLandscape ? lMin : lMax)
  if (!Number.isFinite(ratio) || ratio <= 0) return null
  const w = ratio > 1 ? 32 : Math.round(32 * ratio)
  const h = ratio > 1 ? Math.round(32 / ratio) : 32

  const rgba = new Uint8Array(w * h * 4)
  const fx = new Float64Array(7)
  const fy = new Float64Array(7)
  const cxMax = Math.max(lx, hasAlpha ? 5 : 3)
  const cyMax = Math.max(ly, hasAlpha ? 5 : 3)

  for (let y = 0, o = 0; y < h; y++) {
    for (let x = 0; x < w; x++, o += 4) {
      let l = lDC
      let p = pDC
      let q = qDC
      let a = aDC

      // 预计算本像素的余弦基
      for (let cx = 0; cx < cxMax; cx++) fx[cx] = Math.cos((Math.PI / w) * (x + 0.5) * cx)
      for (let cy = 0; cy < cyMax; cy++) fy[cy] = Math.cos((Math.PI / h) * (y + 0.5) * cy)

      // L 通道(三角区遍历,与 decodeChannel 完全同序)
      for (let cy = 0, j = 0; cy < ly; cy++) {
        const fy2 = fy[cy] * 2
        for (let cx = cy ? 0 : 1; cx * ly < lx * (ly - cy); cx++, j++) {
          l += lAC[j] * fx[cx] * fy2
        }
      }

      // P/Q 色度通道(固定 3×3)
      for (let cy = 0, j = 0; cy < 3; cy++) {
        const fy2 = fy[cy] * 2
        for (let cx = cy ? 0 : 1; cx < 3 - cy; cx++, j++) {
          const f = fx[cx] * fy2
          p += pAC[j] * f
          q += qAC[j] * f
        }
      }

      // A 通道(固定 5×5)
      if (hasAlpha) {
        for (let cy = 0, j = 0; cy < 5; cy++) {
          const fy2 = fy[cy] * 2
          for (let cx = cy ? 0 : 1; cx < 5 - cy; cx++, j++) {
            a += aAC[j] * fx[cx] * fy2
          }
        }
      }

      // LPQ → RGB:B = L - 2/3·P;R = (3L - B + Q)/2;G = R - Q
      const b = l - (2 / 3) * p
      const r = (3 * l - b + q) / 2
      const g = r - q
      // 与 Rust 端 `as u8` 一致:clamp 后截断(Uint8Array 赋值即截断),保证跨端对拍稳定
      rgba[o] = Math.min(1, Math.max(0, r)) * 255
      rgba[o + 1] = Math.min(1, Math.max(0, g)) * 255
      rgba[o + 2] = Math.min(1, Math.max(0, b)) * 255
      rgba[o + 3] = Math.min(1, Math.max(0, a)) * 255
    }
  }

  return { rgba, w, h }
}

/** 从 ThumbHash 中提取平均 RGB 颜色。这是 O(1) 的，速度极快，非常适合用作纯色占位符。 */
export function thumbhashToAverageColor(hash: number[] | Uint8Array): string {
  const bytes = hash instanceof Uint8Array ? hash : new Uint8Array(hash)
  if (bytes.length < 5) return '#333333'

  const header = bytes[0] | (bytes[1] << 8) | (bytes[2] << 16)
  const l = (header & 63) / 63
  const p = ((header >> 6) & 63) / 31.5 - 1
  const q = ((header >> 12) & 63) / 31.5 - 1

  // LPQ → RGB,与 thumbHashToRGBA 同一换算(官方 thumb_hash_to_average_rgba 对应式)
  const b = l - (2 / 3) * p
  const r = (3 * l - b + q) / 2
  const g = r - q

  const toHex = (c: number) =>
    Math.max(0, Math.min(255, Math.round(c * 255)))
      .toString(16)
      .padStart(2, '0')
  return `#${toHex(r)}${toHex(g)}${toHex(b)}`
}

/** 从 thumbhash 创建 CSS 模糊占位符字符串。返回一个 `background-image: url(...)` 值。 */
export function thumbhashToBackgroundImage(hash: number[] | Uint8Array): string {
  const url = thumbhashToDataURL(hash)
  return url ? `url(${url})` : ''
}

// ── Thumbhash 生成的异步 / 空闲队列 ─────────────────────────────────────────

const bgCache = new Map<string, string>()
const pendingQueue: { hash: number[] | Uint8Array; key: string; resolve: (val: string) => void }[] =
  []
let isIdleScheduled = false

function processIdleQueue(deadline: IdleDeadline) {
  // 只要空闲帧中至少还剩 2 毫秒，就处理任务
  while (pendingQueue.length > 0 && deadline.timeRemaining() > 2) {
    const task = pendingQueue.shift()!
    if (bgCache.has(task.key)) {
      task.resolve(bgCache.get(task.key)!)
    } else {
      const bg = thumbhashToBackgroundImage(task.hash)
      bgCache.set(task.key, bg)
      task.resolve(bg)
    }
  }

  if (pendingQueue.length > 0) {
    window.requestIdleCallback(processIdleQueue)
  } else {
    isIdleScheduled = false
  }
}

/** 在浏览器空闲期间延迟生成 thumbhash 背景图像。这完全消除了快速滚动期间主线程的卡顿。 */
export function getThumbhashBgAsync(hash: number[] | Uint8Array): Promise<string> {
  const bytes = hash instanceof Uint8Array ? hash : new Uint8Array(hash)
  const key = bytes.join(',')

  if (bgCache.has(key)) {
    return Promise.resolve(bgCache.get(key)!)
  }

  return new Promise((resolve) => {
    pendingQueue.push({ hash: bytes, key, resolve })
    if (!isIdleScheduled) {
      isIdleScheduled = true
      if ('requestIdleCallback' in window) {
        window.requestIdleCallback(processIdleQueue)
      } else {
        setTimeout(() => processIdleQueue({ timeRemaining: () => 50, didTimeout: false }), 1)
      }
    }
  })
}
