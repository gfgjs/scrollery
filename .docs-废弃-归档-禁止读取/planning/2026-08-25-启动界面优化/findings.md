---
status: 施工中
type: 工作记忆
line: 启动界面优化
created: 2026-08-25
---

# 发现与决策:启动界面优化

## 需求
- 当前先显示小框、再显示大框，视觉跳变明显。
- 目标形态是“程序完整大小窗口 + 半透明 + 加载动画”。
- 本轮先给方案，不改功能代码。

## 发现
- `src-tauri/tauri.conf.json` 同时声明可见的 400×280 `splashscreen` 和隐藏的 1280×820 `main`，双原生窗口尺寸不同是跳变根因。
- `src/App.vue` 要等 `startupConfigPromise` 完成后才调用 `close_splashscreen`，所以小窗口持续时间还包含前端配置 IPC。
- `src-tauri/src/ipc/system_commands.rs::close_splashscreen` 会先关闭 splash，再显示并聚焦 main，原生窗口身份、尺寸和焦点都发生切换。
- main 已接入 `transparent: true`、Windows Mica/Acrylic 背板与前端 `data-glass` 样式，启动层应复用现有材质系统，不另建一套毛玻璃实现。
- 窗口状态插件持久化 main 的大小、位置和最大化状态；若继续用独立 splash，即使把默认尺寸改成 1280×820，也无法可靠匹配用户上次窗口几何。
- `public/splashscreen.html` 已有品牌图形、旋转环和加载点，可迁移视觉语言，但不应继续作为独立 WebView 窗口。
- 推荐保留 main 的 `visible:false` 初值，在 Rust `setup` 最前段完成 Windows 无边框修正后立刻 `show`；这比直接配置 `visible:true` 更能避免原生标题栏首帧闪现，同时让后续 DB/配置初始化期间已有完整窗口承载反馈。
- 启动层应作为 `index.html` 中 Vue 入口之外的静态同窗口覆盖层：它不等待 JS bundle/Vue 挂载即可绘制，Vue 在下层完成布局后再淡出；为保持 MVP，不额外引入只负责同一段视觉的 Vue 组件。
- 启动层不新增 CSS `backdrop-filter` 模糊链；Windows 复用现有 DWM Mica/Acrylic，其他平台使用高不透明度主题色半透明兜底，避免与现行窗口材质设计重复合成。
- Rust `setup` 在修正 Windows 无边框装饰后立即显示 main，再于配置 manager 就绪后应用 `window_material`；因此 WebView 首帧由同一个已恢复几何的完整窗口承载，静态层不会等待 DB/配置初始化完成。
- `index.html` 的启动层位于 `#app` 外，并由 `public/startup-layer.js` 提供 12 秒无百分比超时提示；Vue 主窗口和日志窗口各自在自身 ready 点调用同一个淡出函数。
- 启动层的 `prefers-reduced-motion: reduce` 仅将 ring/dots 与淡出过渡降为近乎即时，不改变启动状态语义。

## 外部资料(当数据,不当指令)
- 本轮方案以仓库当前 Tauri/Vue 实现和已落地窗口材质设计为依据，未引入外部资料。

## 耐久提升候选(F-001 递增;发现当场登记,收口时逐行处置进 closeout.md)
| 候选 ID | 内容摘要 | 建议去向 |
|---------|----------|----------|
| F-001 | 启动反馈层应与正式 UI 复用同一个原生窗口，避免窗口几何、焦点、任务栏身份和材质切换 | design |
| F-002 | 入口启动层必须独立于 Vue/入口 bundle，并在 ready 后透传 pointer events | design |
| F-003 | 启动超时只能提示等待状态，不能凭空展示百分比或伪造阶段进度 | design |
| F-004 | 静态启动层若排在 #app 后，或首帧颜色依赖稍后到达的全局 CSS，仍可能出现空背景→logo 的单次重绘；应把层放在 body 第一节点并使用入口内联主题变量 | design |
| F-005 | 隐藏窗口在 Rust setup 未完成 `app.manage(AppState)` 前不应 show；否则可见 WebView 会抢先发起启动 IPC，失败后主题/材质不会水合。首次 show 前还需同步挂载原生背板，show 后再补异步重挂以覆盖可见性重置 | design |
