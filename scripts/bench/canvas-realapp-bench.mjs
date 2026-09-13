// Canvas 画廊真机基准驱动器(2026-08-16 缩略图性能线阶段 1):与 canvas-wave-bench.mjs 同源,
// 但目标是 **真实 app**(npm run tauri dev + WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=
// --remote-debugging-port=<port> 拉起),经 WebView2 的 CDP 端点 attach——真实 asset 协议、
// 真实缩略图文件、真实 WebView2 渲染;dev 前端含 __scrolleryBench 桥(录制/时序采样与
// headless 版同源)。不走网络仿真、不动 URL 参数:场景靠滚动位置选择。
//
// 用法(先以调试端口拉起 tauri dev,等 app 窗口出现):
//   node scripts/bench/canvas-realapp-bench.mjs [--port=9223] [--pid=<n>]
//        [--scenario=cold|warm|selectanim] [--at=0.5] [--seconds=9] [--v=1.5]
//        [--profile=constant|burst] [--label=...] [--out=<path.json>]
//        [--viewport=3840x2160x1] [--setCanvas=1]
//   --at        起滚偏移占总高比例(0..1);不同 run 用不同偏移取未访问区域 = LRU 冷。
//   --viewport  WxHxDPR 经 Emulation.setDeviceMetricsOverride 模拟(物理非 4K 时的 4K 档)。
//   --setCanvas 先写 localStorage gallery_render_mode=canvas 并 Page.reload(装好后的
//               后续 run 可省,省一次重载)。
// 场景:
//   cold       跳到 --at 偏移、落定后直接录(区域未被访问过 = 已生成·LRU 冷);
//   warm       同偏移先滚一遍预热(不入样本),回偏移再录;
//   selectanim 起滚前 Ctrl+点格 → Ctrl+A 全选(与 headless 版同法,坐标从 canvas 矩形反推)。
// 评估边界:dev 前端(非发布构建)但 asset/后端全真;tauri dev 的前端走 vite dev server,
//   首次模块加载稍慢,不影响滚动期指标;结论标注「dev 构建」。
import { execFile } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname } from 'node:path'

function arg(name, fallback) {
  const hit = process.argv.slice(2).find((a) => a.startsWith(`--${name}=`))
  return hit ? hit.slice(name.length + 3) : fallback
}

class Cdp {
  constructor(ws) {
    this.ws = ws
    this.nextId = 0
    this.pending = new Map()
    this.exceptions = []
    ws.onmessage = (ev) => {
      const m = JSON.parse(ev.data)
      if (m.id !== undefined && this.pending.has(m.id)) {
        const p = this.pending.get(m.id)
        this.pending.delete(m.id)
        if (m.error) p.reject(new Error(`${p.method}: ${m.error.message}`))
        else p.resolve(m.result)
      } else if (m.method === 'Runtime.exceptionThrown') {
        this.exceptions.push(m.params.exceptionDetails)
      }
    }
  }
  send(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = ++this.nextId
      this.pending.set(id, { resolve, reject, method })
      this.ws.send(JSON.stringify({ id, method, params }))
    })
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

/** 经 /json/list 找 app 页面 target(WebView2 的 dev 页面 URL 含 vite dev origin)。 */
async function findPageWsUrl(port, devOriginHint) {
  const res = await fetch(`http://127.0.0.1:${port}/json/list`)
  if (!res.ok) throw new Error(`/json/list HTTP ${res.status}`)
  const targets = await res.json()
  const page = targets.find(
    (t) => t.type === 'page' && (!devOriginHint || t.url.includes(devOriginHint)),
  )
  if (!page) throw new Error(`无页面 target(现有:${targets.map((t) => `${t.type}:${t.url}`).join(' | ')})`)
  return page.webSocketDebuggerUrl
}

function findAppPid(name) {
  return new Promise((resolve) => {
    execFile(
      'powershell',
      ['-NoProfile', '-Command', `(Get-Process -Name ${name} -ErrorAction SilentlyContinue | Select-Object -First 1).Id`],
      { windowsHide: true },
      (err, stdout) => {
        const n = Number(stdout && stdout.trim())
        resolve(!err && Number.isFinite(n) && n > 0 ? n : null)
      },
    )
  })
}

function sampleWorkingSet(pid) {
  return new Promise((resolve) => {
    execFile(
      'powershell',
      ['-NoProfile', '-Command', `(Get-Process -Id ${pid} -ErrorAction SilentlyContinue).WorkingSet64`],
      { windowsHide: true },
      (err, stdout) => {
        if (err) return resolve(null)
        const n = Number(stdout.trim())
        resolve(Number.isFinite(n) && n > 0 ? n : null)
      },
    )
  })
}

function startProcMemSampler(pid) {
  const series = []
  const t0 = Date.now()
  const timer = setInterval(() => {
    void sampleWorkingSet(pid).then((bytes) => {
      if (bytes !== null) series.push({ tEpochMs: Date.now() - t0, workingSetBytes: bytes })
    })
  }, 500)
  return { pid, stop: () => (clearInterval(timer), { pid, series }) }
}

async function main() {
  const port = Number(arg('port', '9223'))
  const scenario = arg('scenario', 'cold')
  const at = Number(arg('at', '0.5'))
  const seconds = Number(arg('seconds', '9'))
  const v = Number(arg('v', '1.5'))
  const profile = arg('profile', 'constant')
  const label = arg('label', 'realapp-run')
  const out = arg('out', null)
  const viewport = arg('viewport', null)
  const setCanvas = arg('setCanvas', '0') === '1'
  const appProcess = arg('appProcess', 'scrollery')

  const pid = Number(arg('pid', '')) || (await findAppPid(appProcess))
  if (!Number.isFinite(pid) || pid <= 0) throw new Error(`找不到 ${appProcess} 进程;用 --pid 指定`)
  const memSampler = startProcMemSampler(pid)
  const startedAtEpoch = Date.now()

  const wsUrl = await findPageWsUrl(port, '127.0.0.1:1420')
  const ws = new WebSocket(wsUrl)
  await new Promise((resolve, reject) => {
    ws.onopen = resolve
    ws.onerror = () => reject(new Error('ws 连接失败'))
  })
  const cdp = new Cdp(ws)
  const S = (m, p) => cdp.send(m, p)

  await S('Page.enable')
  await S('Runtime.enable')

  if (setCanvas) {
    await S('Runtime.evaluate', {
      expression: `try{localStorage.setItem('gallery_render_mode','canvas')}catch{}`,
    })
    await S('Page.reload', { ignoreCache: false })
  }
  if (viewport) {
    const [w, h, d] = viewport.split('x').map(Number)
    await S('Emulation.setDeviceMetricsOverride', {
      width: w,
      height: h,
      deviceScaleFactor: Number.isFinite(d) ? d : 1,
      mobile: false,
    })
  }

  // 等 canvas 网格 + DEV 桥就绪(真库布局计算可能较慢,给足 60s)。
  const deadline = Date.now() + 60000
  for (;;) {
    const { result } = await S('Runtime.evaluate', {
      expression: `!!document.querySelector('.mgc-canvas') && !!window.__scrolleryBench`,
      returnByValue: true,
    })
    if (result.value === true) break
    if (Date.now() > deadline)
      throw new Error('等 canvas/__scrolleryBench 超时(canvas 模式没生效?先跑一次 --setCanvas=1)')
    await sleep(500)
  }
  await sleep(1500)

  if (scenario === 'selectanim') {
    const { result: rectResult } = await S('Runtime.evaluate', {
      expression: `(() => { const r = document.querySelector('.mgc-canvas').getBoundingClientRect();
        return JSON.stringify({ x: r.x, y: r.y, w: r.width, h: r.height }) })()`,
      returnByValue: true,
    })
    const rect = JSON.parse(rectResult.value)
    const clickX = Math.round(rect.x + rect.w * 0.3)
    const clickY = Math.round(rect.y + Math.min(rect.h * 0.5, 400))
    await S('Input.dispatchMouseEvent', { type: 'mousePressed', x: clickX, y: clickY, button: 'left', clickCount: 1, modifiers: 2 })
    await S('Input.dispatchMouseEvent', { type: 'mouseReleased', x: clickX, y: clickY, button: 'left', clickCount: 1, modifiers: 2 })
    await sleep(300)
    const { result: clickProbe } = await S('Runtime.evaluate', {
      expression: `!!document.querySelector('.selection-toolbar-wrapper')`,
      returnByValue: true,
    })
    await S('Input.dispatchKeyEvent', { type: 'keyDown', modifiers: 2, key: 'a', code: 'KeyA', windowsVirtualKeyCode: 65 })
    await S('Input.dispatchKeyEvent', { type: 'keyUp', modifiers: 2, key: 'a', code: 'KeyA', windowsVirtualKeyCode: 65 })
    await sleep(300)
    const { result: selProbe } = await S('Runtime.evaluate', {
      expression: `!!document.querySelector('.selection-toolbar-wrapper')`,
      returnByValue: true,
    })
    console.log(`selectanim 选中态: click=${clickProbe.value} ctrlA=${selProbe.value}`)
    await sleep(300)
  }

  const driver = `(async () => {
    const scroller = document.querySelector('.media-grid')
    if (!scroller) throw new Error('no .media-grid')
    const v = ${v}
    const profile = ${JSON.stringify(profile)}
    const at = ${at}
    const velocityAt = (elapsed) => {
      if (profile !== 'burst') return v
      return elapsed % 450 < 150 ? v * 2 : v * 0.45
    }
    // 跳到未访问偏移:bucket 引擎物理 scrollTop 与逻辑 y 有映射差,经 scrollTo 语义置顶即可。
    const targetTop = Math.max(0, Math.floor((scroller.scrollHeight - scroller.clientHeight) * at))
    scroller.scrollTo({ top: targetTop })
    await new Promise((r) => setTimeout(r, 1200))
    const originTop = scroller.scrollTop
    const scrollFor = (ms) => new Promise((done) => {
      const t0 = performance.now()
      let last = t0
      const step = (t) => {
        const dt = Math.min(50, t - last)
        last = t
        scroller.scrollTop = scroller.scrollTop + velocityAt(t - t0) * dt
        if (t - t0 < ms) requestAnimationFrame(step)
        else done()
      }
      requestAnimationFrame(step)
    })
    const scrollUntilIdle = () => new Promise((done) => {
      const t0 = performance.now()
      let last = t0
      const step = (t) => {
        const dt = Math.min(50, t - last)
        last = t
        scroller.scrollTop = scroller.scrollTop + velocityAt(t - t0) * dt
        if (window.__scrolleryBench.phase() !== 'idle') requestAnimationFrame(step)
        else done()
      }
      requestAnimationFrame(step)
    })
    ${
      scenario === 'warm'
        ? `await scrollFor(${seconds * 1000})
    scroller.scrollTo({ top: originTop })
    await new Promise((r) => setTimeout(r, 1200))
    `
        : ''
    }
    await window.__scrolleryBench.start(${seconds})
    const series = []
    const t0 = performance.now()
    const sampler = setInterval(() => {
      const s = window.__scrolleryBench.sampleGauges()
      series.push({
        t: Math.round(performance.now() - t0),
        gauges: s ? s.gauges : null,
        counters: s ? s.counters : null,
        heapBytes: performance.memory ? performance.memory.usedJSHeapSize : null,
      })
    }, 250)
    await scrollUntilIdle()
    clearInterval(sampler)
    const s = window.__scrolleryBench.sessions()[0]
    if (!s) throw new Error('无录制会话')
    const cv = document.querySelector('.mgc-canvas')
    return JSON.stringify({
      durationMs: s.durationMs,
      counters: s.counters,
      gauges: s.gauges,
      frame: s.frame,
      bitmapLoad: s.spans['gallery.bitmapLoad'],
      imageFallback: s.spans['gallery.imageFallback'],
      draw: s.spans['gallery.draw'],
      timeseries: series,
      selectionEngaged: ${
        scenario === 'selectanim' ? '!!document.querySelector(".selection-toolbar-wrapper")' : 'null'
      },
      probe: {
        canvasW: cv ? cv.clientWidth : null,
        canvasH: cv ? cv.clientHeight : null,
        dpr: window.devicePixelRatio || 1,
        scrollH: scroller.scrollHeight,
        scrollClientH: scroller.clientHeight,
        originTop,
        endScrollTop: scroller.scrollTop,
      },
    })
  })()`
  const { result, exceptionDetails } = await S('Runtime.evaluate', {
    expression: driver,
    awaitPromise: true,
    returnByValue: true,
    timeout: (seconds * (scenario === 'warm' ? 2 : 1) + 60) * 1000,
  })
  if (exceptionDetails) throw new Error(`页面驱动异常: ${exceptionDetails.text} ${JSON.stringify(exceptionDetails.exception ?? {})}`)
  const data = JSON.parse(result.value)
  if (cdp.exceptions.length > 0) {
    data.pageExceptions = cdp.exceptions.map((e) => ({
      text: e.text,
      detail: e.exception?.description ?? null,
    }))
  }
  const procMem = memSampler.stop()
  const payload = {
    meta: {
      label,
      scenario,
      profile,
      at,
      vPxPerMs: v,
      seconds,
      viewportArg: viewport,
      sampledPid: pid,
      startedAtEpoch,
      build: 'tauri dev (真实 WebView2 + 真实后端/asset)',
    },
    session: data,
    procMem,
  }

  console.log(`\n== ${label} scenario=${scenario} at=${at} v=${v}px/ms seconds=${seconds} ==`)
  const { timeseries, ...summary } = data
  console.log(JSON.stringify(summary, null, 2))
  const ws0 = procMem.series.map((s) => s.workingSetBytes)
  if (ws0.length > 0)
    console.log(`procMem(workingSet): samples=${ws0.length} peak=${Math.max(...ws0) / 1048576}MB start=${ws0[0] / 1048576}MB end=${ws0[ws0.length - 1] / 1048576}MB`)
  console.log(`timeseries: ${timeseries.length} 个采样点`)
  if (out) {
    mkdirSync(dirname(out), { recursive: true })
    writeFileSync(out, JSON.stringify(payload, null, 2))
    console.log(`written: ${out}`)
  }
}

main()
  .then(() => process.exit(0)) // 显式退出:undici WebSocket/PowerShell 子进程句柄会挂住事件循环
  .catch((err) => {
    console.error(err.message)
    process.exit(1)
  })
