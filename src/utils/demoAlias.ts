// 演示打码别名(2026-09-16):演示时不暴露敏感目录名/文件名的**显示层**替换。
// 只改显示——返回值不参与筛选、命中、拖拽与任何 IPC 入参(真实数据保持原样)。
//
// 稳定性契约:同一实体在一次运行内恒得同一别名。注册表按实体身份建键、首次请求时分配序号,
// 其后树行/画廊分隔头/信息浮窗/拖动浮标复用同一项,与渲染顺序无关。
// 目录键统一取「显示路径」串,三处同源(已对代码核对,非按约定推断):
//  - 树 absPath:useFolderTree.attachAbsPath → `root.path + '/' + relPath`(relPath 空则 root.path)
//  - 画廊分隔行标签:后端 DirLabel.display → `root_path + '/' + rel_path`(空则 root_path)
//  - 信息浮窗 meta.dirPath:SQL `CASE WHEN d.rel_path='' THEN r.path ELSE r.path||'/'||d.rel_path END`
// 三者都由同一个 scan_roots.path 派生、都用 '/' 连接,故原串已对齐;仍经 pathKey 归一(反斜杠
// →正斜杠、去尾斜杠、去首尾空白),防同一目录因尾斜杠/分隔符差异拿到两个序号。
// 不做大小写折叠:大小写敏感卷上 `A`/`a` 确是两个目录,折叠会让它们共用别名。
import i18n from '../i18n'
import type { LayoutRowItem, LayoutRowSeparator, MediaMeta } from '../types/layout'

/**
 * 归一路径键:统一分隔符、去尾斜杠、去首尾空白。仅用于建键,不影响真实路径的任何用途。
 */
function pathKey(path: string): string {
  return path.trim().replace(/\\/g, '/').replace(/\/+$/, '')
}

/** 文件别名类别:决定前缀词(图片/视频/文件);编号在三个类别间共用一条序列。 */
export type DemoAliasKind = 'image' | 'video' | 'file'

/** 别名序号补零宽度:「文件夹 001」「图片 001.jpg」。 */
const SEQ_WIDTH = 3

/** 目录/文件别名序号:键 = 实体身份,值 = 本运行内分配的序号。 */
const folderSeq = new Map<string, number>()
const fileSeq = new Map<string, number>()

// 已分配的完整别名文本(key → 文本):只给「拿不到扩展名、只剩一个 id」的出口回查用
// (树的拖动浮标就是这种出口——它只有 dragFileId,拼不出扩展名)。查不到返回 null,
// 由调用方回退中性通用词,绝不现场新分配一个不带扩展名的号,免得与行内别名对不上。
const fileAliasText = new Map<string, string>()

/**
 * 取序号:命中即复用(同一实体全程同一别名),未命中按注册表当前规模分配下一个。
 * 用 size + 1 而非独立计数器——清空注册表时编号自然从头开始,无需额外状态。
 */
function seqOf(map: Map<string, number>, key: string): number {
  const hit = map.get(key)
  if (hit != null) return hit
  const n = map.size + 1
  map.set(key, n)
  return n
}

function pad(n: number): string {
  return String(n).padStart(SEQ_WIDTH, '0')
}

/** 媒体类型 → 别名类别(只认 image/video,其余含未入库 null 归「文件」)。 */
export function demoAliasKindOf(mediaType: string | null | undefined): DemoAliasKind {
  if (mediaType === 'image') return 'image'
  if (mediaType === 'video') return 'video'
  return 'file'
}

/**
 * 取扩展名(不含点,小写);无扩展名返回空串。
 * 仅用于让别名保留文件类型可读性,不参与任何格式判定。
 */
export function demoExtOf(fileName: string | null | undefined): string {
  const name = fileName ?? ''
  const dot = name.lastIndexOf('.')
  // dot <= 0:隐藏文件(.gitignore)与无扩展名一律视作无扩展名。
  return dot > 0 && dot < name.length - 1 ? name.slice(dot + 1).toLowerCase() : ''
}

/** 文件夹别名:同一 display 串恒得同一别名。 */
export function demoFolderAlias(displayPath: string | null | undefined): string {
  const word = i18n.global.t('demoPrivacy.aliasFolder')
  return `${word} ${pad(seqOf(folderSeq, pathKey(displayPath ?? '')))}`
}

/**
 * 树节点的目录别名:优先按显示路径(absPath)建键——它与画廊分隔头、信息浮窗的目录显示路径
 * 同源,故同一目录在三处恒得同一个号。absPath 缺席(少数本地新建/移动后的节点)时回退
 * nodeKey:宁可三处对不上,也不能让一批节点共用同一个键而拿到同一个别名。
 */
export function demoFolderAliasOfNode(node: {
  absPath?: string | null
  nodeKey?: string | null
}): string {
  return demoFolderAlias(node.absPath ?? node.nodeKey ?? '')
}

/** 按媒体 id 回查已分配的别名文本;尚未分配过(该行没渲染过)返回 null。 */
export function demoFileAliasById(id: number): string | null {
  return fileAliasText.get(`id:${id}`) ?? null
}

/** 文件别名的实体身份:id 优先(画廊侧只有 id),未入库文件回退相对路径(只在树里出现)。 */
export interface DemoFileIdentity {
  id?: number | null
  /**
   * 路径身份 `{rootId}:{relPath}`(后端产出)。无 id 时**优先**用它:relPath 单独用会在
   * 不同扫描根下撞号(两棵根各有 `a/b.jpg` 就并成同一个文件),nodeKey 自带 rootId,不撞。
   */
  nodeKey?: string | null
  /** 相对扫描根路径(含文件名本身);连 nodeKey 都没有时的最后回退。 */
  relPath?: string | null
  /** 扩展名(不含点)。 */
  ext?: string
  kind?: DemoAliasKind
}

function kindWord(kind: DemoAliasKind): string {
  if (kind === 'image') return i18n.global.t('demoPrivacy.aliasImage')
  if (kind === 'video') return i18n.global.t('demoPrivacy.aliasVideo')
  return i18n.global.t('demoPrivacy.aliasFile')
}

/** 文件别名:图片 001.jpg / 视频 002.mp4 / 文件 003.pdf(编号跨类别共用)。 */
export function demoFileAlias(identity: DemoFileIdentity): string {
  const { id = null, nodeKey = null, relPath = null, ext = '', kind = 'file' } = identity
  // 扩展名统一小写、去前导点后再拼:调用方可能传入库格式(可能是 'JPEG')或带点扩展名,
  // 而树侧传的是真实文件名末段——同一文件在两处必须拼出同一个扩展名。
  const cleanExt = ext.toLowerCase().replace(/^\./, '')
  // 画廊格子只有 id;未入库文件(只在树里出现)先用 nodeKey(含 rootId),再不行才用 relPath。
  let key = `path:${pathKey(relPath ?? '')}`
  if (nodeKey != null) key = `node:${nodeKey}`
  if (id != null) key = `id:${id}`
  const suffix = cleanExt ? `.${cleanExt}` : ''
  const text = `${kindWord(kind)} ${pad(seqOf(fileSeq, key))}${suffix}`
  fileAliasText.set(key, text)
  return text
}

/**
 * 树文件行的别名:身份与类别都从 DirFile 的结构子集取(不 import DirFile,便于纯测试)。
 * 与画廊格子同源——两边都按「同 id → 同键」收敛。
 */
export function demoFileAliasOfFile(file: {
  id?: number | null
  /** 路径身份 `{rootId}:{relPath}`:无 id 的未入库文件用它建键,避免跨扫描根撞号。 */
  nodeKey?: string | null
  relPath?: string | null
  fileName: string
  mediaType?: string | null
}): string {
  return demoFileAlias({
    id: file.id,
    nodeKey: file.nodeKey,
    relPath: file.relPath,
    ext: demoExtOf(file.fileName),
    kind: demoAliasKindOf(file.mediaType),
  })
}

/** 信息浮窗演示文案:敏感字段全部换成示例值,存在性判定仍走真实数据(见 buildThumbInfoLines)。 */
export interface DemoInfoText {
  fileName: string
  dirPath: string
  date: string
  resolution: string
  geo: string
  camera: string
  params: string
  fileSize: string
}

export type DemoInfoFormatter = (item: LayoutRowItem, meta: MediaMeta | undefined) => DemoInfoText

// 固定示例值:刻意通用(与本机拍摄参数、坐标、时间无关),演示时读起来像正常数据。
const DEMO_DATE = '2024-01-01 12:00'
const DEMO_RESOLUTION = '1920 × 1080'
const DEMO_GEO = '25.0330, 121.5654'
const DEMO_CAMERA = 'DemoCam X100'
const DEMO_PARAMS = '35mm f/2.8 1/250s ISO200'
const DEMO_FILE_SIZE = '8.4 MB'

/**
 * 组装单格信息浮窗的演示文案:文件名/路径用与树一致的别名,其余敏感字段用固定示例值。
 * 加载状态与媒体类型角标不在本结构内——它们保持真实,便于演示功能本身。
 */
export function demoInfoTextOf(item: LayoutRowItem, meta: MediaMeta | undefined): DemoInfoText {
  return {
    fileName: demoFileAlias({
      id: item.id,
      // 扩展名取**真实文件名**的末段(与树里 file 行同源同口径),不用 item.fileFormat——
      // 后者可能被映射成 jpeg/大写等形式,会让同一文件在两处显示不同扩展名。
      ext: demoExtOf(meta?.fileName),
      kind: demoAliasKindOf(item.mediaType),
    }),
    dirPath: meta?.dirPath
      ? `${i18n.global.t('demoPrivacy.demoLibrary')}/${demoFolderAlias(meta.dirPath)}`
      : '',
    date: DEMO_DATE,
    resolution: DEMO_RESOLUTION,
    geo: DEMO_GEO,
    camera: DEMO_CAMERA,
    params: DEMO_PARAMS,
    fileSize: DEMO_FILE_SIZE,
  }
}

/** 清空注册表(仅测试用:序号是「本运行内稳定」,测试之间须隔离)。 */
export function resetDemoAliases(): void {
  folderSeq.clear()
  fileSeq.clear()
  fileAliasText.clear()
}

/**
 * 画廊分隔行的演示别名:**开启时才替换,关闭恒返回 null**(宿主原样绘制真实标签)。
 * 只接路径类行——folder 分组的标签与镜头文件夹头的标签都是目录显示路径(DirLabel.display);
 * date 分组的标签是日期、duplicateGroup 的组头是结构化数字文本,都不是敏感名称。
 */
export function demoSeparatorLabelOf(
  row: Pick<LayoutRowSeparator, 'separatorKind' | 'separatorLabel'>,
  groupBy: string,
  enabled: boolean,
): string | null {
  if (!enabled) return null
  if (row.separatorKind === 'duplicateGroup') return null
  if (row.separatorKind === 'duplicateFolder') return demoFolderAlias(row.separatorLabel)
  return groupBy === 'folder' ? demoFolderAlias(row.separatorLabel) : null
}
