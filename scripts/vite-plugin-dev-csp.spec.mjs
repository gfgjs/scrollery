// dev CSP 派生的纯函数测试。
//
// 用真实 tauri.conf.json 的 CSP 字符串做主 fixture(而非手写一份「像」prod 的样例)——
// 手写样例会在 prod CSP 改动时悄悄脱钩,测的是「我以为的 prod CSP」而非「真的 prod CSP」。
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import { deriveDevCsp } from './vite-plugin-dev-csp.mjs'

const PROD_CSP = JSON.parse(
  readFileSync(
    resolve(dirname(fileURLToPath(import.meta.url)), '../src-tauri/tauri.conf.json'),
    'utf-8',
  ),
).app.security.csp

describe('deriveDevCsp', () => {
  it('对真实 prod CSP:除 connect-src 追加 ws: 外,其余每条指令逐值不变', () => {
    const dev = deriveDevCsp(PROD_CSP)
    const prodDirectives = new Map(
      PROD_CSP.split(';')
        .map((d) => d.trim())
        .filter(Boolean)
        .map((d) => {
          const [name, ...values] = d.split(/\s+/)
          return [name, values]
        }),
    )
    const devDirectives = new Map(
      dev
        .split(';')
        .map((d) => d.trim())
        .filter(Boolean)
        .map((d) => {
          const [name, ...values] = d.split(/\s+/)
          return [name, values]
        }),
    )

    expect(devDirectives.size).toBe(prodDirectives.size) // 未新增/丢失任何指令
    for (const [name, values] of prodDirectives) {
      if (name === 'connect-src') continue
      expect(devDirectives.get(name)).toEqual(values) // 非 connect-src 逐值一致
    }
  })

  it('connect-src 追加 ws:,不动其余值(顺序与内容都保留)', () => {
    const dev = deriveDevCsp(PROD_CSP)
    const connectSrc = dev
      .split(';')
      .map((d) => d.trim())
      .find((d) => d.startsWith('connect-src'))
    expect(connectSrc).toContain('ws:')
    // prod 原有的每个 connect-src 值都还在(未被替换或丢失)。
    const prodConnectValues = PROD_CSP.split(';')
      .map((d) => d.trim())
      .find((d) => d.startsWith('connect-src'))
      .split(/\s+/)
      .slice(1)
    for (const v of prodConnectValues) expect(connectSrc).toContain(v)
  })

  it('script-src 不放宽(dev 不因此获得 unsafe-inline/unsafe-eval)', () => {
    const dev = deriveDevCsp(PROD_CSP)
    const scriptSrc = dev
      .split(';')
      .map((d) => d.trim())
      .find((d) => d.startsWith('script-src'))
    expect(scriptSrc).not.toContain('unsafe-inline')
    expect(scriptSrc).not.toContain("'unsafe-eval'")
  })

  it('已含 ws: 的 connect-src 不重复追加(幂等)', () => {
    const csp = "default-src 'self'; connect-src 'self' ws:"
    const dev = deriveDevCsp(csp)
    const wsCount = (dev.match(/ws:/g) || []).length
    expect(wsCount).toBe(1)
  })

  it('缺失 connect-src 时兜底新增一条(防御性分支,不假设 prod 恒有该指令)', () => {
    const csp = "default-src 'self'; script-src 'self'"
    const dev = deriveDevCsp(csp)
    expect(dev).toContain('connect-src')
    expect(dev).toContain('ws:')
  })
})
