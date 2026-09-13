// view.d.ts —— foliate-js `view.js` 的类型 shim(Scrollery vendored,§R2-1)。
//
// 为何存在:tsconfig 无 `allowJs` 且 `include` 只含 .ts/.d.ts/.tsx/.vue,故 vendored `.js`
// 不进 vue-tsc 类型检查;但 .vue/.ts 侧 `import '.../view.js'` 需要一个模块声明才能解析
// (否则 strict 下 TS7016 隐式 any 报错)。此 sibling `.d.ts` 只声明 **Scrollery 实际消费**
// 的公开面,刻意宽松(大量 unknown / 索引签名)——它是消费契约,不是对 view.js 的完整反射;
// 权威 API 以同目录 view.js 源码为准(见 VENDOR.md)。
//
// 构建期分工:vue-tsc 读此 shim,vite/rollup 打包真实 view.js。

/** TOC / pageList 树节点。 */
export interface FoliateTocItem {
  label: string
  href: string
  subitems?: FoliateTocItem[]
}

/** 一个 spine section(章)。foliate 各格式 loader 产出的 section 形状。 */
export interface FoliateSection {
  /** 章标识。foliate 各 loader 用 string(comic=文件名)或 number(fb2=序号);SyntheticBook 用 number。 */
  id?: string | number
  linear?: string
  cfi?: string
  size?: number
  /** 渲染入口:返回章文档的可加载 URL(paginator 塞 iframe);可异步 → 按章懒加载。 */
  load?(): string | Promise<string>
  /** 章卸载时回收 load 建立的资源(如 revokeObjectURL)。 */
  unload?(): void
  createDocument?(): Promise<Document>
  resolveHref?(href: string): string
  mediaOverlay?: unknown
}

/** BookModel:makeBook / SyntheticBook 都实现这个面(view.open 消费)。 */
export interface FoliateBook {
  sections: FoliateSection[]
  toc?: FoliateTocItem[]
  pageList?: FoliateTocItem[]
  metadata?: Record<string, unknown> & { language?: string; title?: unknown }
  /** 阅读方向:'rtl' 时 goLeft/goRight 语义自动对换(竖排/RTL 复用点)。 */
  dir?: string
  rendition?: { layout?: string }
  landmarks?: Array<{ type: string[]; href: string }>
  getCover?(): Promise<Blob | null>
  /** 释放 book 持有的资源(SyntheticBook / comic / fb2 用它回收 blob URL);由宿主在卸载时调。 */
  destroy?(): void
  /**
   * 资源载入的拦截口(上游 Loader.eventTarget,epub.js 的 `this.transformTarget = this.#loader.eventTarget`)。
   * 派发两类事件,均在**资源变成 blob URL 之前**:
   *   · 'load' —— detail `{ type, isScript, allow }`。把 `allow` 置 false 即拒绝该资源(loadItem 返回 null)。
   *     **这是上游为「宿主决定要不要脚本」预留的唯一 seam**(epub.js loadItem)。
   *   · 'data' —— detail `{ type, data }`,上游 paginator 用它改写 CSS 的视口单位。
   * 仅 EPUB loader 提供(SyntheticBook / comic / fb2 无),故可选。
   */
  transformTarget?: EventTarget
  splitTOCHref?(href: string): unknown
  getTOCFragment?(doc: Document, id: string | null): unknown
  resolveHref?(href: string): { index: number; anchor?: (doc: Document) => Range | Element }
  resolveCFI?(cfi: string): { index: number; anchor: (doc: Document) => Range | Element }
  isExternal?(href: string): boolean
  [k: string]: unknown
}

/** 'relocate' 事件 detail —— 进度与定位的权威来源。 */
export interface FoliateRelocateDetail {
  /** 全书 progression 0..1(页脚百分比)。 */
  fraction?: number
  /** 当前位置 CFI(cfi: 进度持久化 + loc1 epub 主键)。 */
  cfi?: string
  tocItem?: { label?: string; href?: string; id?: number } | null
  pageItem?: { label?: string } | null
  index?: number
  range?: Range
  location?: { current?: number; total?: number; next?: number }
  [k: string]: unknown
}

/** 'load' 事件 detail —— 章文档载入完成,替换规则/简繁在此挂点(等价 epub.js hooks.content)。 */
export interface FoliateLoadDetail {
  doc: Document
  index: number
}

/** 搜索命中摘要:命中片段 + 前后各约 50 字上下文(带省略号),供结果列表渲染。 */
export interface FoliateSearchExcerpt {
  pre: string
  match: string
  post: string
}

/** 单条搜索命中:定位 CFI(点击跳转)+ 摘要。 */
export interface FoliateSearchMatch {
  cfi: string
  excerpt: FoliateSearchExcerpt
}

/**
 * view.search 的流式产出(全书搜索,index 缺省):
 *  - `{ progress }`   扫描进度 0..1(逐章推进);
 *  - `{ label, subitems }` 某章的命中组(label=章名,subitems=命中列表);
 *  - `'done'`         结束哨兵。
 * 命中同时被 foliate 以 Overlayer 在渲染视图内自动高亮(clearSearch 清除)。
 */
export type FoliateSearchResult =
  | { progress: number }
  | { label: string; subitems: FoliateSearchMatch[] }
  | 'done'

/** 底层渲染器(<foliate-paginator> / <foliate-fxl>)。作为 view.renderer 公开属性可及。 */
export interface FoliateRenderer extends HTMLElement {
  next(distance?: number): Promise<void>
  prev(distance?: number): Promise<void>
  goTo(target: unknown): Promise<void>
  getContents(): Array<{ doc: Document; index: number; overlayer?: unknown }>
  scrollToAnchor(anchor: unknown, select?: boolean): Promise<void>
  destroy?(): void
  /** 注入 section 文档样式:string→仅高优先 $style;[before,after]→before(低优先/书可覆盖)+after(高优先)。
   *  存于内部 #styles,每次新章载入自动重注(paginator.js:1010)。 */
  setStyles(styles: string | [string, string]): void
  // 其余排版配置经 setAttribute 施加(observedAttributes: 'flow'|'gap'|'margin'|...)。
}

/** <foliate-view> 自定义元素。import 'view.js' 副作用即 customElements.define。 */
export interface FoliateView extends HTMLElement {
  book?: FoliateBook
  renderer?: FoliateRenderer
  isFixedLayout: boolean
  lastLocation?: FoliateRelocateDetail
  open(book: File | string | FoliateBook): Promise<void>
  init(opts: { lastLocation?: unknown; showTextStart?: boolean }): Promise<void>
  close(): void
  next(distance?: number): Promise<void>
  prev(distance?: number): Promise<void>
  /** 方向感知翻页:rtl 时与 next/prev 对换(R5 竖排翻页语义复用)。 */
  goLeft(): Promise<void> | void
  goRight(): Promise<void> | void
  goTo(target: unknown): Promise<unknown>
  goToFraction(frac: number): Promise<void>
  goToTextStart(): Promise<unknown>
  getCFI(index: number, range?: Range): string
  getSectionFractions(): number[]
  /** 书内搜索(§R4):index 缺省=全书流式;命中自动高亮(见 FoliateSearchResult)。 */
  search(opts: {
    query: string
    index?: number
  }): AsyncGenerator<FoliateSearchResult, void, unknown>
  clearSearch(): void
}

/** 探测文件类型并构造 BookModel(epub 走 zip loader → EPUB;PDF 后端已 stub)。 */
export function makeBook(file: File | string): Promise<FoliateBook>

export class ResponseError extends Error {}
export class NotFoundError extends Error {}
export class UnsupportedTypeError extends Error {}
