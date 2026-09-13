// Canvas 画廊「换图波」基准驱动器(2026-07-17 防闪烁线沉淀;源自该线阶段 6 的根因定位与 A/B 工具;
// 2026-08-16 缩略图性能线阶段 0 扩展:五场景 + 指标时序化 + 进程内存采样 + --out 落盘)。
// headless Chrome + 裸 CDP(node ≥22 内置 WebSocket,零依赖):
//   1. 打开 ?ui-harness=gallery 场景(canvas 渲染模式经 localStorage 预置,行高/条目数/引擎经 URL 参数);
//   2. 启动落定后开网络仿真(禁缓存 + 每请求 latency)模拟逐图 IO 延迟;
//   3. 以恒速或 burst 速度剖面驱动 .media-grid 滚动;
//   4. 经 window.__scrolleryBench(usePerformanceMonitor 的 DEV 桥)录制并回读
//      coldCellFrames(冷格·帧积分,换图波强度)/ prefetchLoadsStarted / 帧健康等指标;
//      录制期间 250ms 轮询 sampleGauges()(recorder.currentCounters 浅拷贝)补 gauge 时序
//      (gauge 摘要 last-write-wins,不采时序就只剩最后值);Node 侧 500ms 采进程工作集。
//
// 用法: npm run dev 先起 dev server,然后
//   node scripts/bench/canvas-wave-bench.mjs [--rowHeight=200] [--items=10000] [--v=1.5]
//        [--seconds=9] [--latency=40] [--profile=constant|burst] [--label=baseline]
//        [--scenario=cold|warm|ungenned|offline|missing|selectanim] [--availRatio=0.8]
//        [--out=<path.json>] [--pid=<n>]
//   --v 单位 px/ms;--profile=burst 模拟滚轮连拨(450ms 周期:150ms 峰 v×2 + 300ms 谷 v×0.45)。
//   --scenario(审查报告 §6 五场景,默认 cold):
//     cold       已生成·LRU 冷的首次进入(默认;fresh profile + 禁网络缓存);
//     warm       LRU 全热连续滚动——同剖面先滚一遍预热(不入样本),回顶落定后再录;
//     ungenned   未生成冷库首览(&thumbStatus=0,fixture 模拟批量生成逐项回填);
//     offline    offline 高占比(&avail=offline,比例 --availRatio 默认 0.8);
//     missing    missing 高占比(&avail=missing,同上);
//     selectanim 大范围选中动画期间滚动(Ctrl+点格进选择态 → Ctrl+A 全选 → 滚动)。
//   --pid 采样指定进程的工作集(阶段 1 真机配合用;缺省采样本脚本拉起的 headless Chrome)。
//
// 评估边界(诚实标注,详见 worklogs/2026-07-17-Canvas快速拖动防闪烁优化/findings.md 阶段 6):
//   dev server 每主机 6 连接 ≈150 req/s 吞吐钳制——恒速 ≥12px/ms 真飞掠域与 60px 极密域的
//   结论不可用(真机 asset 协议无此限);对比实验须同脚本同参数跑 A/B,不跨域外推。
//   headless Chrome 非 WebView2:绝对值仅作分层定位与 A/B 相对比较,真机终裁在阶段 1。
import { execFile, spawn } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'

const DEV_ORIGIN = 'http://127.0.0.1:1420'

const SCENARIO_URL_PARAMS = {
  cold: '',
  warm: '',
  selectanim: '',
  ungenned: '&thumbStatus=0',
  offline: '&avail=offline',
  missing: '&avail=missing',
}

function arg(name, fallback) {
  const hit = process.argv.slice(2).find((a) => a.startsWith(`--${name}=`))
  return hit ? hit.slice(name.length + 3) : fallback
}

function findChrome() {
  if (process.env.CHROME_PATH) return process.env.CHROME_PATH
  const candidates = [
    'C:/Program Files/Google/Chrome/Application/chrome.exe',
    'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
    'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  ]
  const hit = candidates.find((p) => existsSync(p))
  if (!hit) throw new Error('找不到 Chrome/Edge,设 CHROME_PATH')
  return hit
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
        // 页面内未捕获异常(rAF 回调里的 throw 不会传到 evaluate 结果,只能从此捕获)。
        this.exceptions.push(m.params.exceptionDetails)
      }
    }
  }
  send(method, params = {}, sessionId) {
    return new Promise((resolve, reject) => {
      const id = ++this.nextId
      this.pending.set(id, { resolve, reject, method })
      this.ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }))
    })
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

/** Windows 工作集采样(PowerShell;进程已退出时返回 null 静默跳过)。 */
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

/** 500ms 周期采进程工作集;stop() 后返回 {pid, series:[{tEpochMs, workingSetBytes}]}。 */
function startProcMemSampler(pid) {
  const series = []
  const t0 = Date.now()
  const timer = setInterval(() => {
    void sampleWorkingSet(pid).then((bytes) => {
      if (bytes !== null) series.push({ tEpochMs: Date.now() - t0, workingSetBytes: bytes })
    })
  }, 500)
  return {
    pid,
    stop: () => {
      clearInterval(timer)
      return { pid, series }
    },
  }
}

async function main() {
  const rowHeight = arg('rowHeight', '200')
  const items = arg('items', '10000')
  const v = Number(arg('v', '1.5'))
  const seconds = Number(arg('seconds', '9'))
  const latency = Number(arg('latency', '40'))
  const label = arg('label', 'run')
  const profile = arg('profile', 'constant')
  const scenario = arg('scenario', 'cold')
  const availRatio = arg('availRatio', '0.8')
  const out = arg('out', null)
  const externalPid = arg('pid', null)
  if (!(scenario in SCENARIO_URL_PARAMS)) {
    throw new Error(`未知 --scenario=${scenario};可选:${Object.keys(SCENARIO_URL_PARAMS).join('|')}`)
  }

  try {
    const res = await fetch(`${DEV_ORIGIN}/?ui-harness=gallery`)
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
  } catch (err) {
    throw new Error(`dev server 未响应(${DEV_ORIGIN}): ${err.message};先 npm run dev`)
  }

  const chrome = findChrome()
  const profileDir = mkdtempSync(join(tmpdir(), 'scrollery-bench-'))
  const child = spawn(
    chrome,
    [
      '--headless=new',
      '--remote-debugging-port=0',
      `--user-data-dir=${profileDir}`,
      '--window-size=1280,900',
      '--force-device-scale-factor=1',
      '--no-first-run',
      '--disable-background-timer-throttling',
      'about:blank',
    ],
    { stdio: ['ignore', 'pipe', 'pipe'] },
  )
  const wsUrl = await new Promise((resolve, reject) => {
    let buf = ''
    const onData = (b) => {
      buf += b
      const m = buf.match(/DevTools listening on (ws:\/\/\S+)/)
      if (m) resolve(m[1])
    }
    child.stderr.on('data', onData)
    child.stdout.on('data', onData)
    child.on('exit', () => reject(new Error('chrome 提前退出')))
    setTimeout(() => reject(new Error('等 DevTools ws 超时')), 15000)
  })

  // 进程内存采样:--pid 指定则采外部进程(阶段 1 真机),否则采本 headless Chrome 子进程。
  const memSampler = startProcMemSampler(Number(externalPid ?? child.pid))
  const startedAtEpoch = Date.now()

  const ws = new WebSocket(wsUrl)
  await new Promise((resolve, reject) => {
    ws.onopen = resolve
    ws.onerror = () => reject(new Error('ws 连接失败'))
  })
  const cdp = new Cdp(ws)

  try {
    const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' })
    const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true })
    const S = (m, p) => cdp.send(m, p, sessionId)

    await S('Page.enable')
    await S('Runtime.enable')
    // 应用启动前预置:canvas 渲染模式(useRenderMode 读 localStorage)。
    await S('Page.addScriptToEvaluateOnNewDocument', {
      source: `try{localStorage.setItem('gallery_render_mode','canvas')}catch{}`,
    })
    const availParam =
      scenario === 'offline' || scenario === 'missing' ? `&availRatio=${availRatio}` : ''
    const url = `${DEV_ORIGIN}/?ui-harness=gallery&items=${items}&rowHeight=${rowHeight}&bucket=1${
      SCENARIO_URL_PARAMS[scenario]
    }${availParam}`
    await S('Page.navigate', { url })

    // 等 canvas 网格 + DEV 桥就绪
    const deadline = Date.now() + 30000
    for (;;) {
      const { result } = await S('Runtime.evaluate', {
        expression: `!!document.querySelector('.mgc-canvas') && !!window.__scrolleryBench`,
        returnByValue: true,
      })
      if (result.value === true) break
      if (Date.now() > deadline) throw new Error('等 canvas/__scrolleryBench 超时(canvas 模式没生效?)')
      await sleep(250)
    }
    await sleep(1500) // 首段/首屏落定

    // 启动完成后才开网络仿真:禁缓存 + 每请求 latency,模拟逐图 IO 延迟
    await S('Network.enable')
    await S('Network.setCacheDisabled', { cacheDisabled: true })
    await S('Network.emulateNetworkConditions', {
      offline: false,
      latency,
      downloadThroughput: 50 * 1024 * 1024,
      uploadThroughput: 50 * 1024 * 1024,
    })

    // selectanim 前置:Ctrl+点一格(canvas onClick → toggleSelect 进选择态)→ Ctrl+A 全选
    // (useSelection:ctrl+a 仅在选择态下生效)。modifiers:2 = Ctrl。落点从 canvas 的视口矩形
    // 反推(页面有侧栏/轴栏,固定坐标会点进侧栏);y 取 +130 避开首部分隔符行(38px)。
    if (scenario === 'selectanim') {
      const { result: rectResult } = await S('Runtime.evaluate', {
        expression: `(() => { const r = document.querySelector('.mgc-canvas').getBoundingClientRect();
          return JSON.stringify({ x: r.x, y: r.y, w: r.width, h: r.height }) })()`,
        returnByValue: true,
      })
      const rect = JSON.parse(rectResult.value)
      const clickX = Math.round(rect.x + rect.w * 0.3)
      const clickY = Math.round(rect.y + 130)
      await S('Input.dispatchMouseEvent', {
        type: 'mousePressed',
        x: clickX,
        y: clickY,
        button: 'left',
        clickCount: 1,
        modifiers: 2,
      })
      await S('Input.dispatchMouseEvent', {
        type: 'mouseReleased',
        x: clickX,
        y: clickY,
        button: 'left',
        clickCount: 1,
        modifiers: 2,
      })
      await sleep(300)
      const { result: clickProbe } = await S('Runtime.evaluate', {
        expression: `!!document.querySelector('.selection-toolbar-wrapper')`,
        returnByValue: true,
      })
      console.log(`selectanim Ctrl+点击(${clickX},${clickY})后选中态: ${clickProbe.value}`)
      await S('Input.dispatchKeyEvent', {
        type: 'keyDown',
        modifiers: 2,
        key: 'a',
        code: 'KeyA',
        windowsVirtualKeyCode: 65,
      })
      await S('Input.dispatchKeyEvent', {
        type: 'keyUp',
        modifiers: 2,
        key: 'a',
        code: 'KeyA',
        windowsVirtualKeyCode: 65,
      })
      await sleep(300)
      const { result: selProbe } = await S('Runtime.evaluate', {
        expression: `!!document.querySelector('.selection-toolbar-wrapper')`,
        returnByValue: true,
      })
      console.log(`selectanim Ctrl+A 后选中态探测: ${selProbe.value}`)
      await sleep(300)
    }

    const driver = `(async () => {
      const scroller = document.querySelector('.media-grid')
      if (!scroller) throw new Error('no .media-grid')
      scroller.scrollTop = 0
      await new Promise((r) => setTimeout(r, 400))
      const v = ${v}
      const profile = ${JSON.stringify(profile)}
      // burst:模拟滚轮连拨——每 450ms 周期内 150ms 峰值(v*2)+300ms 谷值(v*0.45),
      // 峰值越 engage、谷值落在滞回带下缘附近,复现「稍快速」闸门翻动域。
      const velocityAt = (elapsed) => {
        if (profile !== 'burst') return v
        return elapsed % 450 < 150 ? v * 2 : v * 0.45
      }
      // 时长驱动滚(预热趟用):滚满 ms 毫秒即停。
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
      // 录制态驱动滚:与既有语义一致,滚到 recorder 自动停(phase 回 idle)。
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
          ? `// 预热趟(不入样本):同剖面先滚一遍让 LRU 装载本区域位图,回顶落定后再录。
      await scrollFor(${seconds * 1000})
      scroller.scrollTop = 0
      await new Promise((r) => setTimeout(r, 800))
      `
          : ''
      }
      await window.__scrolleryBench.start(${seconds})
      // 时序采样:gauge 摘要 last-write-wins,时序结论只能靠录制期轮询(250ms)补。
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
        draw: s.spans['gallery.draw'],
        timeseries: series,
        selectionEngaged: ${
          scenario === 'selectanim'
            ? '!!document.querySelector(".selection-toolbar-wrapper")'
            : 'null'
        },
        // 几何探针:draw() 早退(viewW/viewH 为 0 或 ctx null)时 counters 只有 drawRequests、
        // gauges 全空——此字段用于区分「管线没事做」与「画布没量到」。
        probe: {
          canvasW: cv ? cv.clientWidth : null,
          canvasH: cv ? cv.clientHeight : null,
          canvasBufW: cv ? cv.width : null,
          scrollH: scroller.scrollHeight,
          scrollClientH: scroller.clientHeight,
          endScrollTop: scroller.scrollTop,
        },
      })
    })()`
    const { result, exceptionDetails } = await S('Runtime.evaluate', {
      expression: driver,
      awaitPromise: true,
      returnByValue: true,
      timeout: (seconds * (scenario === 'warm' ? 2 : 1) + 40) * 1000,
    })
    if (exceptionDetails) throw new Error(`页面驱动异常: ${exceptionDetails.text} ${JSON.stringify(exceptionDetails.exception ?? {})}`)
    const data = JSON.parse(result.value)
    if (cdp.exceptions.length > 0) {
      const first = cdp.exceptions[0]
      console.error(
        `页面未捕获异常 ×${cdp.exceptions.length}:`,
        first.text,
        JSON.stringify(first.exception ?? {}),
      )
      data.pageExceptions = cdp.exceptions.map((e) => ({
        text: e.text,
        detail: e.exception?.description ?? null,
      }))
    }
    const procMem = memSampler.stop()
    const meta = {
      label,
      scenario,
      profile,
      rowHeight: Number(rowHeight),
      items: Number(items),
      vPxPerMs: v,
      seconds,
      latencyMs: latency,
      url,
      devOrigin: DEV_ORIGIN,
      startedAtEpoch,
      sampledPid: procMem.pid,
      note: 'headless Chrome 非 WebView2;绝对值仅作分层定位与 A/B 相对比较',
    }
    const payload = { meta, session: data, procMem }

    console.log(`\n== ${label} scenario=${scenario} rowHeight=${rowHeight} v=${v}px/ms latency=${latency}ms items=${items} ==`)
    const { timeseries, ...summary } = data
    console.log(JSON.stringify(summary, null, 2))
    const ws0 = procMem.series.map((s) => s.workingSetBytes)
    if (ws0.length > 0) {
      console.log(
        `procMem(workingSet): samples=${ws0.length} peak=${Math.max(...ws0) / 1048576}MB end=${
          ws0[ws0.length - 1] / 1048576
        }MB`,
      )
    } else {
      console.log('procMem(workingSet): 无样本(进程已退出或采样失败)')
    }
    console.log(`timeseries: ${timeseries.length} 个采样点`)

    if (out) {
      mkdirSync(dirname(out), { recursive: true })
      writeFileSync(out, JSON.stringify(payload, null, 2))
      console.log(`written: ${out}`)
    }
  } finally {
    try {
      child.kill()
    } catch {}
    await sleep(300)
    rmSync(profileDir, { recursive: true, force: true })
  }
}

main().catch((err) => {
  console.error(err.message)
  process.exit(1)
})
