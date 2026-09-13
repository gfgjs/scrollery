// S7「6 主题视觉矩阵」截图捕获器 —— 主题 × 场景 全组合出图,供人眼过一遍。
//
// 为什么是**捕获器而不是视觉回归门**(有意的取舍):
//   截图基线门(pixel diff)在本仓是幽灵门禁反模式——字体栅格/抗锯齿/Chrome 版本任一变动即红,
//   而红了唯一可能的响应是「重新生成基线」。设阈值前先问「红了我能做什么」,若答案只有「改基线」
//   就别设(判据见 docs/experience.md)。故本脚本只做**捕获**这件苦力,判定留给人眼:
//   token 契约(消费≡定义)与对比度已各有真门把守,人眼要抓的是它们**证明不了**的东西——
//   硬编码色不随主题走、取值合法但视觉失调、跨主题观感断层。
//
// 为什么无 Playwright:本仓已有先例(findings 会话续:headless Chrome + `?ui-harness=` 做 DOM 断言)。
//   加 Playwright 要为一次性视觉核对背上浏览器下载与新 devDependency,不值。
//
// 边界(**这不是真机**):headless Chrome 与 Tauri WebView2 同为 Chromium,故 CSS 变量解析/布局
//   /配色可信;但**原生窗口边框、WebView2 特有行为、GPU canvas 路径不在覆盖内**,那些仍须真机。
//
// 用法:先另起 `npm run dev`(或 `npm run tauri dev`),再 `npm run capture:themes`。
//   `--out=<dir>` 改输出目录;`--scene=gallery` / `--theme=fresh-dark` 只跑子集;`--size=1440,900` 改窗口。

import { existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs'
import { join, resolve, basename } from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { tmpdir } from 'node:os'

const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)))
const THEMES_DIR = join(ROOT, 'src/assets/styles/themes')

/** harness 已实现的场景(src/harness/runtime.ts 的 UiHarnessScene 联合类型)。 */
const SCENES = ['gallery', 'settings', 'viewer']

const DEV_ORIGIN = 'http://127.0.0.1:1420'

/**
 * 主题 id 取自 themes/ 下的 CSS 文件名,而**不是**解析 registry.ts。
 * 二者等价有硬门担保:theme-contract.spec 钉死 `[...cssById.keys()].sort() === registryIds.sort()`。
 * 故此处零解析、零漂移——新增主题只要按契约落一个 <id>.css,本矩阵自动把它纳入。
 */
function discoverThemes() {
  return readdirSync(THEMES_DIR)
    .filter((f) => f.endsWith('.css'))
    .map((f) => basename(f, '.css'))
    .sort()
}

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

/** dev server 不在时给出可行动的报错,而不是让 18 张图全部静默截成空白页。 */
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

async function main() {
  const outDir = resolve(arg('out', join(ROOT, '.screenshots/theme-matrix')))
  const size = arg('size', '1440,900')
  const onlyScene = arg('scene', null)
  const onlyTheme = arg('theme', null)
  // --tint=<pct>:主题色浓度矩阵(2026-09-06)用,如 --tint=60;不传走 harness 默认(生产默认 60)。
  const tint = arg('tint', null)
  const tintQuery = tint === null ? '' : `&tint=${encodeURIComponent(tint)}`
  // --text=<pct>:文字浓度矩阵(2026-09-06)用;不传走 harness 默认(生产默认 75)。
  const text = arg('text', null)
  const textQuery = text === null ? '' : `&text=${encodeURIComponent(text)}`
  // 文件名后缀按非默认参数拼,两种浓度独立可辨,同时传则以 -tint..-text.. 连缀。
  const suffix =
    (tint === null ? '' : `-tint${tint}`) + (text === null ? '' : `-text${text}`)

  const themes = discoverThemes().filter((t) => !onlyTheme || t === onlyTheme)
  const scenes = SCENES.filter((s) => !onlyScene || s === onlyScene)
  if (themes.length === 0) throw new Error(`没有匹配的主题(--theme=${onlyTheme})`)
  if (scenes.length === 0) throw new Error(`没有匹配的场景(--scene=${onlyScene})`)

  await preflight()
  const chrome = findChrome()
  mkdirSync(outDir, { recursive: true })
  // 每次跑用独立临时 profile:复用 profile 会带上一次的 localStorage(含主题快照),
  // 让「首帧着色」这类首次启动行为被上一次的残留污染。
  const profileDir = join(tmpdir(), `scrollery-theme-matrix-${process.pid}`)

  console.log(`浏览器: ${chrome}`)
  console.log(`输出:   ${outDir}`)
  console.log(
    `矩阵:   ${themes.length} 主题 × ${scenes.length} 场景` +
      (tint === null ? '' : ` × tint=${tint}`) +
      (text === null ? '' : ` × text=${text}`) +
      ` = ${themes.length * scenes.length} 张\n`,
  )

  let ok = 0
  const failed = []
  for (const theme of themes) {
    for (const scene of scenes) {
      const name = `${scene}-${theme}${suffix}.png`
      const url = `${DEV_ORIGIN}/?ui-harness=${scene}&theme=${theme}${tintQuery}${textQuery}`
      process.stdout.write(`  ${name.padEnd(28)} `)
      try {
        await shoot(chrome, url, join(outDir, name), size, profileDir)
        ok++
        console.log('✓')
      } catch (err) {
        failed.push(`${name}: ${err.message}`)
        console.log(`✗ ${err.message}`)
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
