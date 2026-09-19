import { watch } from 'vue'
import { createRouter, createWebHashHistory } from 'vue-router'
import i18n from '../i18n'
import { loadViewerComponent } from './viewerRouteLoader'
import GalleryRouteLayer from '../components/media/GalleryRouteLayer.vue'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: '/',
      component: GalleryRouteLayer,
      meta: { title: 'routes.allMedia' },
    },
    {
      path: '/folder/:id',
      component: GalleryRouteLayer,
      meta: { title: 'sidebar.folders' },
    },
    {
      path: '/favorites',
      component: GalleryRouteLayer,
      meta: { title: 'sidebar.favorites' },
    },
    {
      // S2-c 视图路由化:smart-album 各占独立路径(此前 live-photos/recent 连路由壳都没有,靠 store 停在 '/')。
      path: '/live-photos',
      component: GalleryRouteLayer,
      meta: { title: 'sidebar.livePhotos' },
    },
    {
      path: '/recent',
      component: GalleryRouteLayer,
      meta: { title: 'sidebar.recentlyAdded' },
    },
    {
      path: '/collections',
      component: () => import('../views/CollectionsView.vue'),
      meta: { title: 'sidebar.collections' },
    },
    {
      path: '/collections/:id',
      component: GalleryRouteLayer,
      meta: { title: 'sidebar.collections' },
    },
    {
      // 人物墙（F6）：人脸识别聚类出的人物簇，点卡片进入该人物的照片。
      path: '/persons',
      component: () => import('../views/PersonsView.vue'),
      meta: { title: 'sidebar.persons' },
    },
    {
      path: '/persons/:id',
      component: GalleryRouteLayer,
      meta: { title: 'sidebar.persons' },
    },
    {
      // 插件商店（T11）：浏览/安装 exotic 格式插件 + 激活 + 处理进度，路由级懒加载。
      path: '/plugins',
      component: () => import('../views/PluginStoreView.vue'),
      meta: { title: 'sidebar.plugins' },
    },
    {
      // 设置属于页面级信息架构，不再由全局 boolean 伪装成全屏 modal。
      path: '/settings/:section?',
      component: () => import('../views/SettingsView.vue'),
      meta: { title: 'settings.title' },
    },
    {
      // 文档浏览器（P4, §5.1）：按格式分发 pdf.js / epub.js / 文本渲染器，路由级懒加载。
      path: '/doc/:id',
      component: () => import('../views/DocumentViewer.vue'),
      meta: { title: 'routes.doc' },
    },
    {
      // 音频播放器（P3, §3.6）：封面 + 控件 + 同步歌词 + 元数据面板，路由级懒加载。
      path: '/audio/:id',
      component: () => import('../views/AudioPlayer.vue'),
      meta: { title: 'routes.audio' },
    },
    {
      // 统一查看器（顶栏重构 P4-b, §5 L2）：图/视从 body 覆盖层迁为 shell 内路由。ContentViewer 按
      // mediaType 渲染,缩放/平移/翻页/信息/exotic/人脸内核复用 useMediaDetail。?path= 为未来 OS
      // shell 深链(外部文件直达)预留。文档/音频仍走上方专用路由(本期未合并入 /view)。
      path: '/view/:id',
      component: loadViewerComponent,
      meta: { title: 'routes.view' },
    },
    {
      // H-Lab 横向画廊实验室:多种横向布局候选的真人调研载体,与 MediaGrid 完全平行
      // (独立后端缓存/滚动器;docs/designs/2026-07-02-horizontal-gallery-lab.md)。
      path: '/hgallery-lab',
      component: () => import('../views/HGalleryLabView.vue'),
      meta: { title: 'routes.hgalleryLab' },
    },
    {
      path: '/trash',
      component: GalleryRouteLayer,
      meta: { title: 'sidebar.trash' },
    },
  ],
})

// meta.title 存 i18n 键，导航与切语言两个时机都要刷新窗口标题——
// 只在 afterEach 翻译会让切语言后的标题滞留旧语言直到下次导航（R1-7 验收「en-US 全界面无中文」不允许）。
function applyDocumentTitle() {
  const titleKey = router.currentRoute.value.meta.title as string | undefined
  document.title = titleKey ? `${i18n.global.t(titleKey)} — Scrollery` : 'Scrollery'
}

router.afterEach(() => applyDocumentTitle())
watch(i18n.global.locale, () => applyDocumentTitle())

export default router
