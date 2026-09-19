// scripts/vite-plugin-theme-default-css.ts
// 默认主题 CSS 与首帧脚本的交付插件(主题配色重构,方案 §7)。
//
// 分工:
//  - transformIndexHtml 注入默认主题 CSS(:root 浅色 / html[data-color-scheme='dark'] 深色)。
//    这份 CSS 与运行时**同源**:都由 src/themes 的 generateTheme 现算,不手写第二套色值;
//    首帧缓存缺失时(首次启动、刚恢复默认、外部改动后)由它兜底,不再有六套启动主题分支。
//  - 按 /theme-bootstrap.js 交付首帧脚本。脚本源码由 src/themes/bootstrapScript.ts 生成
//    (内嵌 generate.ts 的变量清单,校验「整份完整」才恢复),故不能在 public/ 手写一份。
//    dev 走中间件、build 走资产,URL 与 index.html 的引用一致。
//
// 本插件不引入新依赖、不写文件系统、不参与应用打包(脚本作为独立资产产出,不进入口 chunk)。

import type { Plugin } from 'vite'
import { themeDefaultCss } from '../src/themes/apply'
import { THEME_BOOTSTRAP_URL, buildBootstrapScript } from '../src/themes/bootstrapScript'
import { DEFAULT_THEME_DEFINITION } from '../src/themes/presets'

export default function themeDefaultCssPlugin(): Plugin {
  // 启动脚本只依赖纯数据,构建期生成一次即可(dev 与 build 内容一致)。
  const bootstrap = buildBootstrapScript()
  const bootstrapFile = THEME_BOOTSTRAP_URL.replace(/^\//, '')

  return {
    name: 'scrollery:theme-default-css',

    configureServer(server) {
      // 直接 use 的中间件先于 Vite 内部中间件执行(public/ 下已无此文件,不会与之争抢)。
      server.middlewares.use((req, res, next) => {
        if (req.url?.split('?')[0] !== THEME_BOOTSTRAP_URL) {
          next()
          return
        }
        res.setHeader('Content-Type', 'text/javascript; charset=utf-8')
        res.setHeader('Cache-Control', 'no-cache')
        res.end(bootstrap)
      })
    },

    generateBundle() {
      this.emitFile({ type: 'asset', fileName: bootstrapFile, source: bootstrap })
    },

    transformIndexHtml() {
      return [
        {
          tag: 'style',
          attrs: { id: 'theme-default-css' },
          children: themeDefaultCss(DEFAULT_THEME_DEFINITION),
          injectTo: 'head-prepend',
        },
      ]
    },
  }
}
