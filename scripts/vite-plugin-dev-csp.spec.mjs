// F14:dev 态 CSP 的 WebSocket 来源必须收敛到开发服务器实际地址,不再放行裸 ws:。
//
// 先红后绿:旧实现对 connect-src 追加裸 `ws:`,以下两例会失败(红)。
// 只覆盖 vite.config.ts 的两种**实际**配置——默认 127.0.0.1:1420、显式 TAURI_DEV_HOST 时
// HMR host:1421——来源取自真实 resolved Vite config,不另维护一份地址,也不构造通用配置矩阵。
import { describe, it, expect, afterEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolveConfig } from 'vite'
import devCsp from './vite-plugin-dev-csp.mjs'

const prodCsp = JSON.parse(
  readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf-8'),
).app.security.csp

/** 期望:生产 CSP 一字不改,仅在 connect-src 末尾追加确切的 ws 来源。 */
function expectedCsp(wsOrigin) {
  const connectSrc = prodCsp
    .split(';')
    .map((d) => d.trim())
    .find((d) => d.startsWith('connect-src'))
  return prodCsp.replace(connectSrc, `${connectSrc} ${wsOrigin}`)
}

/** 走插件真实注入路径:用 resolved Vite server 配置构造 dev 上下文。 */
async function injectedCsp(serverOptions) {
  const tags = await devCsp().transformIndexHtml('', {
    server: { config: { server: serverOptions } },
  })
  return tags[0].attrs.content
}

afterEach(() => {
  delete process.env.TAURI_DEV_HOST
})

describe('dev CSP 的 WebSocket 来源收敛到开发服务器', () => {
  it('默认(无 TAURI_DEV_HOST):来源为 127.0.0.1:1420,其余生产指令不变,插件不进生产构建', async () => {
    delete process.env.TAURI_DEV_HOST
    const cfg = await resolveConfig({}, 'serve')
    const content = await injectedCsp(cfg.server)
    expect(content).toBe(expectedCsp('ws://127.0.0.1:1420'))
    expect(content).not.toMatch(/[;\s]ws:[;\s]/)
    // serve-only:build 解析下插件按 apply 被排除,生产构建拿不到 dev CSP 注入点。
    const buildCfg = await resolveConfig({}, 'build')
    expect(buildCfg.plugins.some((p) => p.name === 'scrollery:dev-csp')).toBe(false)
  })

  it('显式 TAURI_DEV_HOST:来源为 HMR host:1421,其余生产指令不变', async () => {
    process.env.TAURI_DEV_HOST = '192.168.1.50'
    const cfg = await resolveConfig({}, 'serve')
    const content = await injectedCsp(cfg.server)
    expect(content).toBe(expectedCsp('ws://192.168.1.50:1421'))
    expect(content).not.toMatch(/[;\s]ws:[;\s]/)
  })
})
