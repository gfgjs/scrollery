// 主题视觉矩阵截图捕获器 —— 配色档 × 绘制路径 出图,供人眼过一遍。
//
// 为什么是**捕获器而不是视觉回归门**(有意的取舍):
//   截图基线门(pixel diff)在本仓是幽灵门禁反模式——字体栅格/抗锯齿/Chrome 版本任一变动即红,
//   而红了唯一可能的响应是「重新生成基线」。设阈值前先问「红了我能做什么」,若答案只有「改基线」
//   就别设(判据见 docs/experience.md)。故本脚本只做**捕获**这件苦力,判定留给人眼:
//   token 契约与对比度各有真门把守(npm test 里的生成契约、npm run check:contrast),
//   人眼要抓的是它们**证明不了**的东西——取值合法但视觉失调、自定义配色下观感断层、
//   DOM 与 Canvas 两条绘制路径的差异。
//
// 为什么无 Playwright:本仓已有先例(headless Chrome + `?ui-harness=` 做 DOM 断言)。
//   加 Playwright 要为一次性视觉核对背上浏览器下载与新 devDependency,不值。
//
// 边界(**这不是真机**):headless Chrome 与 Tauri WebView2 同为 Chromium,故 CSS 变量解析/布局
//   /配色可信;但**原生窗口边框、WebView2 特有行为、原生玻璃(DWM 背板)、GPU canvas 路径不在
//   覆盖内**,那些仍须真机。
//
// 用法:先另起 `npm run dev`(或 `npm run tauri dev`),再 `npm run capture:themes`。
//   --out=<dir> 改输出目录;--size=1440,900 改窗口;--scene=gallery|settings|viewer 只跑子集;
//   --palette=light|dark|custom|custom-dark 只跑配色子集;--render=dom|canvas 只跑绘制路径子集。
//
// 默认预算 6 张:配色档(浅色默认 / 深色默认 / 明显自定义)× 绘制路径(DOM / Canvas)。
// 数组顺序即出图顺序;不做全场景展开,要更多档位就显式传参。

import { existsSync, mkdirSync, rmSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { tmpdir } from 'node:os'

const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)))

/** harness 已实现的场景(src/harness/runtime.ts 的 UiHarnessScene 联合类型)。 */
const SCENES = ['gallery', 'settings', 'viewer']

/**
 * 配色档:每档只声明 harness 查询参数,取值合法性与色板生成全在应用侧
 * (harness/ipcFixtures → themeStore → generateTheme),本脚本不做颜色计算、也不认主题 id。
 */
const PALETTES = {
  light: { appearance: 'light' },
  dark: { appearance: 'dark' },
  // 明显自定义档:两套配色都换成偏离出厂的种子,呈现浅色档(深色档另有 custom-dark)。
  custom: { appearance: 'light', seed: 'custom' },
  'custom-dark': { appearance: 'dark', seed: 'custom' },
}

/** 绘制路径:画廊两条实现不同,色板同源不代表绘制结果同源,故两路都出图。 */
const RENDER_MODES = ['dom', 'canvas']

/** 默认矩阵 = 三档配色 × 两条绘制路径 = 6 张(方案 §9 的关键画廊矩阵预算)。 */
const DEFAULT_PALETTES = ['light', 'dark', 'custom']

const DEV_ORIGIN = 'http://127.0.0.1:1420'

function findChrome() {
  if (process.env.CHROME_PATH) return process.env.CHROME_PATH
  const candidates = [
    'C:/Program Files/Google/Chrome/Application/chrome.exe',
    'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
    'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/usr/bin/google-chrome',
    '/usr/bin/chromium',
  ]
  const hit = candidates.find((p) => existsSync(p))
  if (!hit) {
    throw new Error(
      '找不到 Chrome/Edge。设 CHROME_PATH 环境变量指向浏览器可执行文件后重试。',
    )
  }
  return hit
}

function arg(name, fallback) {
  const hit = process.argv.slice(2).find((a) => a.startsWith(`--${name}=`))
  return hit ? hit.slice(name.length + 3) : fallback
}

/** dev server 不在时给出可行动的报错,而不是让整个矩阵静默截成空白页。 */
async function preflight() {
  try {
    const res = await fetch(`${DEV_ORIGIN}/?ui-harness=gallery`)
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
  } catch (err) {
    throw new Error(
      `dev server 未响应(${DEV_ORIGIN}):${err.message}\n` +
        '请先在另一个终端运行 `npm run dev`,再跑本脚本。',
    )
  }
}

function shoot(chrome, url, outFile, size, profileDir) {
  return new Promise((done, fail) => {
    const child = spawn(
      chrome,
      [
        '--headless=new',
        '--disable-gpu',
        '--hide-scrollbars',
        // 截图须与设计像素 1:1,否则跨机 DPI 会让同一矩阵出不同尺寸的图。
        '--force-device-scale-factor=1',
        // harness 首屏要跑完 IPC fixture → 布局 → 缩略图;虚拟时钟快进到静止再截。
        '--virtual-time-budget=10000',
        `--user-data-dir=${profileDir}`,
        `--screenshot=${outFile}`,
        `--window-size=${size}`,
        url,
      ],
      { stdio: ['ignore', 'ignore', 'pipe'] },
    )
    let stderr = ''
    child.stderr.on('data', (b) => (stderr += b))
    child.on('error', fail)
    child.on('close', (code) => {
      if (code === 0 && existsSync(outFile)) done()
      else fail(new Error(`Chrome 退出码 ${code}:${stderr.trim().split('\n').slice(-2).join(' ')}`))
    })
  })
}

function matrixUrl(scene, palette, render) {
  const query = new URLSearchParams({ 'ui-harness': scene, appearance: palette.appearance })
  if (palette.seed) query.set('seed', palette.seed)
  query.set('render', render)
  return `${DEV_ORIGIN}/?${query}`
}

async function main() {
  const outDir = resolve(arg('out', join(ROOT, '.screenshots/theme-matrix')))
  const size = arg('size', '1440,900')
  const onlyScene = arg('scene', null)
  const onlyPalette = arg('palette', null)
  const onlyRender = arg('render', null)

  const paletteNames = onlyPalette ? [onlyPalette] : DEFAULT_PALETTES
  const unknown = paletteNames.find((name) => !(name in PALETTES))
  if (unknown) {
    throw new Error(`未知配色档 --palette=${unknown}(可选:${Object.keys(PALETTES).join(' / ')})`)
  }
  // 默认只拍画廊(方案 §9 的关键矩阵预算);settings/viewer 要显式 --scene= 才纳入。
  const scenes = onlyScene === null ? ['gallery'] : SCENES.filter((s) => s === onlyScene)
  const renders = RENDER_MODES.filter((r) => !onlyRender || r === onlyRender)
  if (scenes.length === 0) throw new Error(`没有匹配的场景(--scene=${onlyScene})`)
  if (renders.length === 0) throw new Error(`没有匹配的绘制路径(--render=${onlyRender})`)

  await preflight()
  const chrome = findChrome()
  mkdirSync(outDir, { recursive: true })
  // 每次跑用独立临时 profile:复用 profile 会带上一次的 localStorage(含首帧主题缓存),
  // 让「首帧着色」这类首次启动行为被上一次的残留污染。
  const profileDir = join(tmpdir(), `scrollery-theme-matrix-${process.pid}`)

  console.log(`浏览器: ${chrome}`)
  console.log(`输出:   ${outDir}`)
  console.log(
    `矩阵:   ${scenes.length} 场景 × ${paletteNames.length} 配色档 × ${renders.length} 绘制路径` +
      ` = ${scenes.length * paletteNames.length * renders.length} 张\n`,
  )

  let ok = 0
  const failed = []
  for (const scene of scenes) {
    for (const name of paletteNames) {
      for (const render of renders) {
        const file = `${scene}-${name}-${render}.png`
        process.stdout.write(`  ${file.padEnd(30)} `)
        try {
          await shoot(chrome, matrixUrl(scene, PALETTES[name], render), join(outDir, file), size, profileDir)
          ok++
          console.log('✓')
        } catch (err) {
          failed.push(`${file}: ${err.message}`)
          console.log(`✗ ${err.message}`)
        }
      }
    }
  }

  rmSync(profileDir, { recursive: true, force: true })
  console.log(`\n完成 ${ok}/${ok + failed.length}`)
  if (failed.length > 0) {
    console.error('失败:\n  ' + failed.join('\n  '))
    process.exit(1)
  }
}

main().catch((err) => {
  console.error(err.message)
  process.exit(1)
})
