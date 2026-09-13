// src/types/reader.ts
// 阅读器文本管线类型（阅读器完善方案 R1，§6.3）。对应 Rust doc_commands 的 DTO。

/** 编码检测置信度来源。`lossy` = U+FFFD 替换率超阈,UI 据此提示「编码可能误判」。 */
export type ReaderEncodingConfidence = 'bom' | 'detected' | 'manual' | 'lossy'

/** 一章的前端可见元数据(字节偏移留在 Rust,前端只见标题 + 字符数)。 */
export interface TextChapterMeta {
  title: string
  /** 章内解码字符数(展示提示,如章长)。 */
  charLen: number
}

/** get_text_book_index 返回:检出编码 + 置信度 + 章节列表。 */
export interface TextBookIndex {
  /** encoding_rs 规范名(如 "GBK" / "UTF-8" / "Big5")。 */
  encoding: string
  confidence: ReaderEncodingConfidence
  chapters: TextChapterMeta[]
}

/** get_text_chapter 返回:章标题 + 段落数组(已剥离作为标题的首行,避免与标题重复渲染)。 */
export interface TextChapterContent {
  title: string
  /** 已分段的段落文本(缩进/段间距由渲染层 CSS 施加,不在文本内)。 */
  paragraphs: string[]
}

/**
 * 简繁转换档位(convert_chinese 的 config 参数,§5.10)。首发档位:关(前端不调) / t2s / s2t / s2tw / s2twp;
 * 其余为 ferrous-opencc 内置项,备扩展。前端在应用替换规则之后按章批量调用。
 */
export type ZhConvertConfig =
  | 's2t' // 简 → 繁
  | 't2s' // 繁 → 简
  | 's2tw' // 简 → 繁(台湾正体)
  | 's2twp' // 简 → 繁(台湾正体 + 词汇:软件→軟體)
  | 'tw2s'
  | 's2hk'
  | 'hk2s'
  | 'tw2sp'
  | 't2tw'
  | 'tw2t'
  | 't2hk'
  | 'hk2t'

/**
 * 每书阅读偏好(reader_book_prefs.prefs 的版本化 JSON,§6.1)。只存与全局默认的 diff,读时 merge。
 * R1 阶段仅用到 `encoding`(手动编码覆盖,§5.1);R3 起扩充竖排/主题/字号等,故字段多为可选 + 版本号。
 */
export interface ReaderBookPrefs {
  /** JSON 结构版本(迁移用)。 */
  v: number
  /** 手动编码覆盖(WHATWG label,如 'gb18030' / 'big5');缺省=自动检测。后端 stored_encoding_override 读取。 */
  encoding?: string
  /** 二级智能重排(硬换行断句黏合,§5.2 R2-6c)。前端读取后经 get_text_chapter 的 reflow 参传入(不进 src_key)。 */
  reflow?: boolean
  /** 简繁转换档位(R4,§5.10)。纯显示层变换,前端读取后透传 BookReader→convert_chinese;不改后端 canonical(不进 src_key,后端忽略此字段)。缺省=关。 */
  zhConvert?: ZhConvertConfig
  /** 每书阅读主题 id(R3)。纯显示层(只改注入 CSS 的正文/背景色),后端忽略;缺省/'' = 跟随全局槽。 */
  theme?: string
  // R3+ 续扩:vertical?/… —— 加字段即可,后端只读取需要的字段。
}

/**
 * 复合定位器(仿 Readium Locator,§6.2)。序列化为 `reading_progress.position` /
 * `reader_bookmarks.locator` 的 `loc1:<json>` 值。
 *
 * **偏移的锚定空间 = canonical 文本**:解码 + 分章 + 分段/重排之后、替换规则/简繁转换之前的
 * 章内字符流。替换/简繁是纯显示层变换(不改 canonical、不改偏移);text-context 重锚也在
 * canonical 空间搜索。推论:重排开关/版本切换会变 canonical → 走 `t` 重锚;替换/简繁切换不影响定位。
 */
export interface ReaderLocator {
  /** 格式判别:txt/md 主键用 c+o,epub 主键用 cfi。 */
  k: 'txt' | 'md' | 'epub'
  /** 章 index(txt/md)。 */
  c?: number
  /** 章内字符偏移(txt/md,canonical 空间)。 */
  o?: number
  /** epub CFI(epub 主键,foliate 解析)。 */
  cfi?: string
  /** 章内 progression 0..1(重锚兜底用)。 */
  p?: number
  /** 全书 progression 0..1(页脚百分比 / 跨章兜底)。 */
  tp?: number
  /** 上下文三元组(前文 / 锚点原文 / 后文,各约 40 字):布局与文本双无关的最后重锚防线。 */
  t?: { b: string; h: string; a: string }
}

/**
 * 一条阅读书签(§6.2,R4),对应 Rust `ReaderBookmark` DTO(camelCase)。`locator` 现用 foliate
 * CFI("cfi:<epubcfi>",与阅读进度同源);loc1 落地后可存 "loc1:<json>",前端形状不变。
 */
export interface ReaderBookmark {
  id: number
  locator: string
  /** 展示标签:当前章名(回退百分比)。 */
  label: string
  /** 全书 progression 0..1:列表排序 + 百分比展示。 */
  fraction: number
  createdAt: number
}
