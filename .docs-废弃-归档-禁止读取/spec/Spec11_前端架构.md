---
id: 2026-07-24-Spec11_前端架构
status: active
type: canon
line: asbuilt-spec
created: 2026-07-24
---

# Spec11-前端架构

> 一句话:本篇讲 Scrollery 前端(`src/`)的总体骨架——入口/分流、19 个 Pinia store、路由、虚拟滚动、主题、i18n、命令体系、IPC 调用层、UI 原语库;各功能模块的 UI 细节交叉引对应规格篇(见 §7)。读前无需前置知识,读后再按需下钻各子系统篇。

## 1. 概览

Scrollery 前端是 Vue 3(Composition API)+ TypeScript(`strict: true`,`tsconfig.json:18`)单页应用,经 Tauri v2 IPC 与 Rust 后端通信,构建工具为 Vite。应用有两种窗口形态:主窗口(完整 AppShell + 路由)与独立日志窗口(仅 Pinia + i18n 的裸挂载,见 §3.6)。

**代码位置**(顶层目录职责):

| 目录 | 职责 |
|---|---|
| `src/main.ts` | 应用入口,窗口分流(主窗/日志窗)、Pinia/Router/i18n 挂载、全局错误兜底 |
| `src/App.vue` | AppShell 根组件:标题栏/侧栏/内容区/底栏槽位组装、全局弹层挂载点 |
| `src/components/` | Vue SFC 组件库,按功能域分类(`media/`、`layout/`、`sidebar/`、`settings/`、`common/`、`ui/` 等) |
| `src/composables/` | Vue 3 可组合函数(`useXxx`),逻辑复用,≥30 个 |
| `src/stores/` | 19 个 Pinia store(见 §2) |
| `src/views/` | 路由目标页面级组件 |
| `src/router/index.ts` | Vue Router 路由表,16 条路由 |
| `src/themes/` | 主题注册表 + CSS 变量主题包 |
| `src/i18n/` | vue-i18n 配置 + `locales/zh-CN`、`locales/en-US` |
| `src/commands/` | 命令注册体系:builtins + registry + keybinding + contextMenu |
| `src/constants/ipc.ts` | 全部 IPC 命令名常量(`IPC.*`) |
| `src/utils/ipc.ts` | 统一 IPC 调用封装 + 错误结构化解析 |
| `src/types/` | 全局 TS 类型(业务模型、IPC 请求/响应类型、组件 props) |
| `src/harness/` | UI 测试 harness(浏览器场景下的 IPC mock,`isUiHarness` 判据) |
| `src/perf/` | 性能探针(画廊布局耗时、span 计时) |
| `src/vendor/` | 第三方代码内联 |
| `src/assets/` | 全局样式入口、字体 |

**在整机中的位置**:

```
用户交互
   │
   ▼
Vue 组件(views/components) ── 读写 ──▶ Pinia store(19 个,内存态投影)
   │                                        │
   │ 调用                                    │ invokeIpc(IPC.xxx, args)
   ▼                                        ▼
composables(useXxx 封装交互/派生逻辑) ──▶ src/utils/ipc.ts::invokeIpc
                                             │
                                    Tauri IPC(@tauri-apps/api/core invoke)
                                             │
                                             ▼
                                   Rust 后端 command(src-tauri/**)
                                             │
                                        SQLite / 文件系统 / 子进程
```

store 不直接持久化——它是后端状态(DB/config.toml)在前端内存中的只读投影;写操作一律经 `invokeIpc` 回后端落盘,前端不维护独立真相源(见 §2 状态归属)。

## 2. 数据模型与状态

### 2.1 Pinia stores(19 个,`src/stores/*.ts`)

全部经 `defineStore(<name>, setup)` 的 Composition API 写法(唯一例外 `configStore` 用 options 写法,`src/stores/configStore.ts:12`)。

| Store(defineStore 名) | 文件:行 | 管什么状态 |
|---|---|---|
| `ui` | `src/stores/uiStore.ts:119` | 外观/主题(appearance、light/darkThemeId)、布局模式(titlebar merged/split)、侧栏折叠、字号/滚动条宽/轴不透明度、`userGuideOpen`(详细引导手册开关) |
| `config` | `src/stores/configStore.ts:12` | 应用配置镜像:扫描路径、导出选项、性能调优参数、`firstLaunch`、`guide_seen` 等 app_config 键值 |
| `viewer` | `src/stores/viewerStore.ts:117` | `activeViewer`(当前查看项,`shallowRef`)、缩放/平移、沉浸模式 |
| `media` | `src/stores/mediaStore.ts:19` | 媒体列表/布局行、筛选后的可见集合、EXIF 等媒体元数据缓存 |
| `search` | `src/stores/searchStore.ts:25` | 搜索词、语义搜索结果、搜索进度 |
| `filter` | `src/stores/filterStore.ts:8` | 筛选条件:日期范围、标签、大小、颜色标签、搜索谓词 |
| `view` | `src/stores/viewStore.ts:13` | 当前导航视图维度(activeCollection/activePerson 等,配合路由参数水合) |
| `history` | `src/stores/historyStore.ts:109` | 导航历史、撤销/重做栈 |
| `scan` | `src/stores/scanStore.ts:32` | 扫描任务进度、发现文件数、扫描错误 |
| `fileJob` | `src/stores/fileJobStore.ts:20` | 复制/移动/删除/导出等文件操作任务的进度与状态 |
| `ai` | `src/stores/aiStore.ts:17` | AI/ML 全局状态:OCR/分类模型加载、RunTokenSlot 槽位管理 |
| `face` | `src/stores/faceStore.ts:20` | 人脸检测矩形、聚类结果、人物 ID 映射 |
| `person` | `src/stores/personStore.ts:13` | 人物墙卡片、当前选中人物 |
| `collection` | `src/stores/collectionStore.ts:17` | 集合/相册列表、集合内容、编辑态 |
| `derivation` | `src/stores/derivationStore.ts:29` | 缩略图/关键帧/ICC 转换派生队列、生成进度 |
| `export` | `src/stores/exportStore.ts:62` | 导出队列、格式选项、导出对话框开关(`openExportDialog`) |
| `backup` | `src/stores/backupStore.ts:67` | 备份集合列表、恢复进度、校验状态 |
| `toast` | `src/stores/toastStore.ts:11` | 全局提示消息队列 |
| `logWindow` | `src/stores/logWindowStore.ts:26` | 日志行(`liveEntries`/`historyEntries`,均 `shallowRef`)、过滤条件 |

**store=只读投影,写回 DB 原则**:store 内的写操作函数(如 `uiStore.setAppearance`、`setThemeForKind`)本地先行更新内存态以做乐观 UI 响应,随即用 `invokeIpc(IPC.SET_APP_CONFIG, …)` 异步落盘;失败仅记日志(`logConfigSaveError`),不回滚内存态(`src/stores/uiStore.ts:227-244`)。没有 store 直接读写 `localStorage`/文件作为权威源——`uiStore` 唯一的 `localStorage` 使用是主题快照(`THEME_SNAPSHOT_KEY`,§3.3)与排序模式埋点,二者均标注为「非权威、仅加速/埋点,损坏可静默降级」(`src/stores/uiStore.ts:190-211`)。

大数组/大对象字段用 `shallowRef` 而非 `ref`(§4 契约),已核实用例:`viewerStore.activeViewer`(`viewerStore.ts:120`)、`logWindowStore.liveEntries`/`historyEntries`(`logWindowStore.ts:28,137`)、`useBucketVirtualScroll` 的 `segments`(`useBucketVirtualScroll.ts:163`)。

## 3. 关键流程与算法

### 3.1 应用入口与窗口分流

`src/main.ts:31-60`:

1. 挂载前先装 `installGlobalErrorHandlers()`(`main.ts:14`),覆盖 mount 前抛出的错误。
2. 首帧主题着色由 `index.html` 内联脚本负责(读 localStorage 快照);`main.ts:18-22` 仅在该脚本未生效时按 `prefers-color-scheme` 兜底补一次 `data-theme`。
3. 判定窗口 label:`isUiHarness` 或非 Tauri 环境固定为 `'main'`,否则读 `getCurrentWindow().label`(`main.ts:31-32`)。
4. `windowLabel === 'logs'` → 动态 `import('./views/LogWindowView.vue')`,仅挂 Pinia + i18n,不接路由/AppShell(`main.ts:34-43`)——避免把日志窗口专属依赖(`@tanstack/vue-virtual`)静态打入两窗口共享的单一 entry chunk,顶爆包体预算门(`scripts/vite-plugin-bundle-budget.mjs`)。
5. 否则挂载完整 `App`:装 Pinia/Router/i18n,`registerBuiltins()` 注册命令(§3.5),`app.mount('#app')`(`main.ts:45-59`)。

### 3.2 路由(vue-router,`src/router/index.ts`)

`createWebHashHistory()`(hash 路由,适配 Tauri WebView),16 条路由,全部 `component: () => import(...)` 懒加载分割:

| 路径 | 组件 | 说明 |
|---|---|---|
| `/` | MediaGrid.vue | 全部媒体 |
| `/folder/:id` | MediaGrid.vue | 按文件夹过滤 |
| `/favorites` | MediaGrid.vue | 收藏 |
| `/live-photos` | MediaGrid.vue | Live Photo smart-album |
| `/recent` | MediaGrid.vue | 最近添加 smart-album |
| `/collections` | CollectionsView.vue | 集合列表 |
| `/collections/:id` | MediaGrid.vue | 集合内容网格 |
| `/persons` | PersonsView.vue | 人物墙 |
| `/persons/:id` | MediaGrid.vue | 指定人物照片网格 |
| `/plugins` | PluginStoreView.vue | 插件商店 |
| `/settings/:section?` | SettingsView.vue | 设置 |
| `/doc/:id` | DocumentViewer.vue | 文档浏览 |
| `/audio/:id` | AudioPlayer.vue | 音频播放 |
| `/view/:id` | ContentViewer.vue | 统一图/视查看器 |
| `/hgallery-lab` | HGalleryLabView.vue | 横向画廊实验室 |
| `/trash` | MediaGrid.vue | 回收站 |

MediaGrid.vue 被 8 条不同语义的路由复用(全部媒体/文件夹/收藏/smart-album/集合内容/人物照片/回收站),按路由参数与 `viewStore` 的当前视图维度区分数据源,而非各建一个组件——`App.vue` 用 `KeepAlive :include="['MediaGrid']"` 只保活 MediaGrid,进出查看器路由零重挂零重算(`App.vue:68-76`)。

`meta.title` 存 i18n key,`router.afterEach` 与语言切换 `watch` 都会刷新 `document.title`(`router/index.ts:103-111`)——只在其一时机刷新会导致切语言后标题滞留旧语言。

### 3.3 虚拟滚动(BucketVirtualScroll,`src/composables/useBucketVirtualScroll.ts`)

用于 MediaGrid 的大列表渲染,基于 `@tanstack/vue-virtual` 之外自研的 bucket 分段引擎(与该库的关系:仓内 grep 未见 MediaGrid 直接调用 `@tanstack/vue-virtual` API,该依赖被 `useBucketVirtualScroll.ts` 之外的场景〔如日志窗口列表〕使用;此处「基于」应理解为并存的虚拟化方案——**待核实**具体 `@tanstack/vue-virtual` 消费点)。

机制(`useBucketVirtualScroll.ts:1-100`):
- **等高算术分段**:段边界固定为 `0, S, 2S, …`,容器总高 = 真实逻辑总高;每段一个绝对定位 `div`(`top=seg.start`,`height=段真实高`),仅渲染愿望窗口内 2-3 个段,不为全量段渲染占位符。
- 段高自适应:`clamp(行高 × ROWS_PER_SEGMENT(20), MIN_SEGMENT_PX(1000), SEGMENT_PX(4000))`(`useBucketVirtualScroll.ts:97-100` 附近,纯函数、单测锁定)。
- **B3 段级坐标映射**:总高超过 `BUCKET_NATIVE_MAX = 16_000_000`px(WebView2 单元素高度钳制 2^24 留余量)时进入映射态,spacer 封顶,段以 `(seg.start − anchorDelta)` 物理定位(`useBucketVirtualScroll.ts:21-37`)。
- **滚动语义按输入源分类**:滚轮/触摸/滚动键走局部 1:1(零补偿);滚动条拖动/轨道点击/巨跳走逐事件全局线性重锚;压缩债仅在滚动停稳(`SCROLL_SETTLE_MS=200ms`)时原子偿还,避免与拖拽手势互搏——这是「快滚防闪」的核心手段(手势进行中绝不写 `scrollTop`)。
- **画廊唯一虚拟滚动实现(P22 收敛)**:原先与「方案 A」(行级坐标压缩)双引擎并存、由 `ui.bucketSegmentedScroll` 开关互斥激活;方案 A 及其开关已退役,其 `SAFE_MAX` 平移态由上述 B3 段级映射态覆盖,bucket 区分滚轮/键盘/触摸输入；单元与 fixture 验证范围及未测平台边界见下方验证报告。Canvas 与 DOM 两种渲染仍并存,超大库 Canvas 能力上限 `CANVAS_MAX_TOTAL_HEIGHT = 10_000_000` 在引擎内单点声明。实施验证见 [滚动实施验证](../reviews/2026-09-15-全仓代码消融/滚动实施验证.md)。

理由链见 [videos-pipeline 系列非本篇范畴]；机制细节归属 [Spec02-扫描与画廊](./Spec02_扫描与画廊.md)(本篇仅摘要虚拟滚动作为前端总体架构的组成部分)。

### 3.4 主题系统(`src/themes/`)

- `BUILTIN_THEMES`(`src/themes/registry.ts:28-71`):6 主题——`ink`(暗)、`porcelain`(亮)、`moonlight`(亮,出厂默认)、`xuan`(亮)、`obsidian`(暗)、`dai`(暗)。每主题声明 `id`/`nameKey`(i18n `themes.<id>`)/`kind`(light|dark)/`preview` 四色/`source: 'builtin'`(预留 `'external'` 供未来主题商店)。
- 出厂默认:`DEFAULT_LIGHT_THEME='moonlight'`、`DEFAULT_DARK_THEME='ink'`(`registry.ts:74-75`)。
- 明暗双槽位模型:`appearance`(system/light/dark)+ 分别持久化的 `lightThemeId`/`darkThemeId`,对应后端 `app_config` 键 `theme_light`/`theme_dark`(`uiStore.ts:240`)。
- 应用:`applyAppearance()`(`uiStore.ts:194-225`)写 `document.documentElement` 的 `data-theme`(具体主题 id,CSS 变量选择器目标)与 `data-color-scheme`(仅 light/dark 两态,供跨主题的明暗分支选择器使用,组件禁止直接选择具体主题 id)。
- 持久化两层:权威层是后端 `app_config`(`invokeIpc(IPC.SET_APP_CONFIG, …)`);`localStorage['scrollery.themeSnapshot.v1']` 是非权威首帧加速快照,供 `index.html` 内联脚本在样式解析前抢先着色,防 FOUC,损坏时静默回退 `matchMedia`。
- mac 专属:`isMac` 时同步原生窗口 `setTheme`(system→null 跟随 OS),Windows 因自绘标题栏(frameless,无原生 caption)不需要。

### 3.5 i18n(`src/i18n/index.ts`)

`vue-i18n` `createI18n({ legacy:false, locale, fallbackLocale:'en-US', messages:{ 'zh-CN', 'en-US' } })`。默认语言取 `navigator.language`,以 `zh` 开头则 `zh-CN`,否则 `en-US`(`i18n/index.ts:6-7`)。切换语言实时生效:`router` 用 `watch(i18n.global.locale, …)` 联动刷新 `document.title`(§3.2);语言资源在 `src/i18n/locales/`,`localeIntegrity.spec.ts` 校验两语言 key 集合一致。

### 3.6 命令/快捷键体系(`src/commands/`)

- `commandRegistry`(`src/commands/registry.ts`):应用级单例命令注册表。
- `registerBuiltins()`(`src/commands/index.ts:16-25`):幂等注册 6 组内建命令——`globalCommands`、`gridCommands`、`viewerImageCommands`、`viewerVideoCommands`、`viewerAudioCommands`、`viewerReaderCommands`(均在 `src/commands/builtins/`)。`main.ts:57` 在 mount 前调用,此时只填充命令定义(标题惰性求值、图标 `markRaw`),不实例化 store,故 mount 前调用安全。
- 消费方三桶:右键菜单(`resolveMediaContextCommands`,`contextMenu.ts`)、上下文工具栏/命令面板(经 `commandRegistry` 直接查询)、键盘分发(`dispatchKeybinding`/`formatKeybinding`,`keybinding.ts`)。
- `AppToolbar` 挂 document 级 `keydown` 分发,严格 `v-if` 限定在画廊路由——若在非画廊页也挂载会与 `AppShell` 的兜底分发双执行(`App.vue:50-57` 注释)。

### 3.7 IPC 调用层

- `src/constants/ipc.ts`:`IPC` 常量对象,所有后端命令名(snake_case 字符串值)按功能分区注册(Scan/Layout/Media/… 等注释分区,`constants/ipc.ts:6-60` 起)。
- `src/utils/ipc.ts`:
  - `IpcCommand = (typeof IPC)[keyof typeof IPC]`(`ipc.ts:17`)——类型层强制 `invokeIpc` 只接受已登记命令值,裸字符串编译期拒绝。
  - `invokeIpc<T>(cmd, args)`(`ipc.ts:86-93`):UI harness 场景下路由到 `invokeHarness`(mock),否则 `invoke<T>(cmd, args)`,catch 统一 `parseAppError` 后 throw。
  - `parseAppError(e)`(`ipc.ts:58-66`):结构化 `{code,message}`(后端 `AppError` 序列化)原样取用;裸字符串(未迁移旧命令)降级 `code='Unknown'`;其余异常尽力取 message。
  - `IpcError extends Error`(`ipc.ts:43-50`):携带 `code: AppErrorCode`,调用方按 `code` 分流(如 `'Cancelled'`/`'Db'`),不匹配错误文案。
  - `invokeIpcRaw<T>`(`ipc.ts:101-112`):字节负载直传(避免 JSON 数字数组膨胀),元数据走 headers,用于大二进制命令(如文档缩略图)。
  - `generateOperationId()`(`ipc.ts:73-78`):生成操作关联 id,贯穿前端发起→command→spawn_blocking/后台流水线,供 tracing 跨边界串联。
- 全量 IPC 命令表与错误码权威定义在 [Spec10-IPC与错误契约](./Spec10_IPC与错误契约.md),本节仅摘要调用层机制。

### 3.8 UI 原语库(`src/components/ui/`)

`UiButton`/`UiDialog`/`UiCheckbox`/`UiEmptyState`/`UiField`/`UiIconButton`/`UiPopover`/`UiSelect`/`UiToggle`,每个原语配同名 `.spec.ts` 单测。理由链见 [Part5 §前端体验重构](../refactor_2026/Part5_前端体验重构.md)——该原语库是 Part5 之后的 UIUX 深度重构(S1 阶段)产物,详见 [docs/designs/2026-07-11-前端UIUX深度审计与重构方案.md](../designs/2026-07-11-前端UIUX深度审计与重构方案.md)。浮层定位统一走 `@floating-ui/vue`;图标统一走 `@lucide/vue`。

## 4. 契约与不变量(施工红线)

| 不变量 | 为什么 |
|---|---|
| TS `strict: true`(`tsconfig.json:18`),禁 `any` | 强类型是低能力 LLM 代理与人类共同维护大型前端的底线;`any` 会让 IPC 命令名/store 字段的改名破坏悄无声息地绕过编译期检查 |
| 大数组/大对象状态用 `shallowRef` 非 `ref` | Vue 深响应式对大数组逐元素 proxy 开销随规模线性增长,画廊/日志等列表规模可达数万项;`shallowRef` 只追踪整体替换,需配合不可变更新(替换整个数组/对象引用)才触发响应式(`viewerStore.ts:118-119`、`viewerStore.spec.ts:67,89` 已用单测锁定该行为) |
| 长列表必须走虚拟化(BucketVirtualScroll 等) | 画廊/文档等列表项数可达十万级,若全量挂载 DOM 节点会导致布局风暴与内存暴涨;§3.3 已是唯一权威虚拟化机制之一 |
| IPC 命令名一律走 `IPC.*` 常量,禁裸字符串 | `IpcCommand` 类型收窄到 `IPC` 常量值集合(`ipc.ts:17`),后端命令改名后前端若仍用旧字符串会编译期报错,而非运行期静默失败 |
| 错误按 `code` 分流,不匹配裸字符串/文案 | `AppErrorCode` 是稳定枚举(`ipc.ts:23-40`),message 仅供展示/日志;文案可能被 i18n 化或措辞调整,只有 code 是分流契约 |
| 主题只用 CSS 变量,组件禁止硬编码颜色/选择具体主题 id | `data-theme` 决定变量取值,`data-color-scheme` 仅供明暗二态分支;组件对具体主题 id 写选择器会绕过主题商店的解耦边界(注册表机制,`themes/registry.ts` 头部注释) |
| store 是只读投影,写操作必须回后端落盘 | 前端无独立持久化真相源;直接改 store 而不经 `invokeIpc` 会导致刷新/重启后状态丢失且与其它窗口/日志窗口不同步 |
| `registerBuiltins()` 幂等且仅填充命令定义,不实例化 store | 该函数在 `mount` 前调用(`main.ts:57`),此时 Pinia 已装但组件树未渲染;若命令定义内联实例化 store 可能提前触发副作用 |

## 5. 边界与失败模式

- **超大列表/快滚**:见 §3.3 的段级映射(总高超 16M px 阈值,`BUCKET_NATIVE_MAX`,`useBucketVirtualScroll.ts:47`)与「压缩债仅停稳时偿还」机制,避免手势进行中的坐标补偿与浏览器原生滚动竞速产生闪帧。
- **IPC 错误呈现**:调用方 catch 后按 `IpcError.code` 分流处理;无法结构化分流的展示类场景用 `ipcErrorMessage(e)`(`ipc.ts:115-118`)取展示文案,常经 `toastStore` 弹出提示。日志见 `logger`(`src/utils/logger.ts`,`installGlobalErrorHandlers` 挂在 mount 前捕获早期异常,`main.ts:14`)。
- **日志窗口独立失败域**:日志窗口是独立挂载(仅 Pinia+i18n,无 Router/AppShell),其崩溃不影响主窗口;反之主窗口早期错误由全局兜底捕获后仍可能因窗口分流判定(`main.ts:31-32`)在日志窗口挂载前发生——此路径下错误只能落到 `installGlobalErrorHandlers` 的兜底,不经日志窗口 UI 展示(**待核实**:是否有跨窗口日志转发兜底路径)。
- **首启态**:`OnboardingWizard`(`first_launch` 缺省时显示)与详细引导手册 `UserGuide`(`ui.userGuideOpen`,独立于 `first_launch`,首次关闭手册写 `guide_seen=true`)两套弹层并存、互不阻塞(`App.vue:98-103`,`uiStore.ts:83,164-180`)。细节见 [Spec12-配置状态日志](./Spec12_配置状态日志.md)。

## 6. 重建指引(从零实现)

**依赖(`package.json`,版本已核实)**:

| 包 | 版本 | 用途 |
|---|---|---|
| `vue` | 3.5.13 | 框架 |
| `vue-router` | 4.6.4 | 路由(hash 模式) |
| `pinia` | 3.0.4 | 状态管理(Composition API store) |
| `@tauri-apps/api` | 2 | IPC/窗口/系统 API |
| `@tauri-apps/plugin-dialog` | 2.7.1 | 文件选择对话框 |
| `@tauri-apps/plugin-os` | 2.3.2 | OS 信息 |
| `@tauri-apps/plugin-shell` | 2.3.5 | 外部命令执行 |
| `@tauri-apps/plugin-window-state` | 2.4.1 | 窗口状态持久化 |
| `vue-i18n` | 9.14.5 | 国际化 |
| `@floating-ui/vue` | 2.0.1 | 浮层定位 |
| `@lucide/vue` | 1.17.0 | 图标 |
| `@tanstack/vue-virtual` | 3.13.33 | 虚拟滚动(部分列表场景;画廊主列表用自研 BucketVirtualScroll,§3.3) |
| `@fontsource-variable/inter` | 5.2.8 | 可变字体 |

（`cropperjs`/`pdfjs-dist`/`shiki`/`onnxruntime-node` 等为特定功能篇〔编辑器/阅读器/AI〕专属依赖,不在本篇总架构范围,见 §7 交叉引用。）

**构建顺序(建议,自底向上)**:
1. `constants/ipc.ts` + `utils/ipc.ts`(IPC 契约层,一切上层依赖它)。
2. `stores/`(先 `configStore`/`uiStore` 这类基础态,再业务 store)。
3. `themes/` + `i18n/`(全局视觉/语言,组件树渲染前需要)。
4. `router/index.ts` + `views/` 空壳页面。
5. `commands/`(builtins + registry)。
6. `components/ui/` 原语库 → 各功能域 `components/`。
7. `App.vue` + `main.ts` 组装收口。

**坑与教训**:详见 [docs/experience.md](../experience.md) §4(UI 重构类)/§14(前端状态与响应式类)——具体条目号需在施工时按当次触发的教训对照,本篇不逐条复制。

**验收**:
- 类型检查:`npm run typecheck`(`vue-tsc --noEmit`)。
- 单元测试:`npm run test`(`vitest run`),覆盖 composables/stores/utils(如 `stores/*.spec.ts`、`composables/useBucketVirtualScroll.spec.ts`、`themes/*.spec.ts`、`i18n/localeIntegrity.spec.ts`)。
- Lint:`npm run lint`(ESLint),格式修复用 `npm run lint:fix`(**禁**用仓库级 `npm run format`,见项目 AGENTS.md)。
- 主题对比度门:`npm run check:contrast`;主题矩阵截图:`npm run capture:themes`。
- CI 门禁权威定义见 `.github/workflows/ci.yml`(仓库级 gate source,本篇不复制)。

## 7. 关联

- 上游正典(理由/历史):[Part5-前端体验重构](../refactor_2026/Part5_前端体验重构.md)——UI 原语库/路由化/明暗双主题槽位的最初立项;[Part4-AI与人脸插件化](../refactor_2026/Part4_AI与人脸插件化.md) §5.1 文档浏览器路由懒加载出处。当前 6 主题、路由完整 16 条、UI 原语库均为 Part5 之后 UIUX 深度重构(2026-07-11 线)的产物,而非 Part5 原始范围——Part5 计划为初版路由化雏形,当前实现已在其上叠加 S1-S7 全套重构(理由见下条设计文档)。
- 相关设计:[docs/designs/2026-07-11-前端UIUX深度审计与重构方案.md](../designs/2026-07-11-前端UIUX深度审计与重构方案.md)、[docs/designs/2026-07-06-前端UI优化与多主题系统.md](../designs/2026-07-06-前端UI优化与多主题系统.md)(主题商店解耦边界出处)、[docs/designs/2026-07-02-horizontal-gallery-lab.md](../designs/2026-07-02-horizontal-gallery-lab.md)(`/hgallery-lab` 路由出处)。
- 经验索引:[docs/experience.md](../experience.md) §4、§14。
- 各子系统 UI 细节交叉引:画廊虚拟滚动/布局细节 → [Spec02-扫描与画廊](./Spec02_扫描与画廊.md);编辑器(裁剪/调色)→ [Spec04-图像与色彩管线](./Spec04_图像与色彩管线.md);播放器(视频/音频进度条、硬解、悬停 scrub)→ [Spec05-视频与音频](./Spec05_视频与音频.md);阅读器(PDF/EPUB/文本)→ [Spec07-文档与阅读器](./Spec07_文档与阅读器.md);AI/人脸/OCR 结果面板 → [Spec06-AI人脸OCR](./Spec06_AI人脸OCR.md);设置页/日志窗口细节 → [Spec12-配置状态日志](./Spec12_配置状态日志.md)。
- IPC 全命令表 + 错误码权威定义 → [Spec10-IPC与错误契约](./Spec10_IPC与错误契约.md)。
