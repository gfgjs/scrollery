<template>
  <AccordionSection id="library" :order="order" :title="$t('sidebar.library')">
    <ul class="nav-list">
      <li v-for="album in smartAlbums" :key="album.id">
        <button
          class="nav-item"
          :class="{
            active:
              viewStore.activeSmartAlbum === album.id &&
              !viewStore.activeDirectoryId &&
              !viewStore.activeCollection &&
              !viewStore.activePersonId,
          }"
          @click="onAlbumClick(album.id)"
        >
          <span class="nav-item__icon"><component :is="album.icon" :size="18" /></span>
          <span class="nav-item__label">{{ album.label }}</span>
          <span v-if="album.count != null" class="nav-item__count">{{
            formatCount(album.count)
          }}</span>
        </button>
      </li>

      <!-- 收藏夹总览（需求7）：进入卡片列表，可建自定义夹 -->
      <li>
        <button
          class="nav-item"
          :class="{ active: route.path.startsWith('/collections') }"
          @click="onCollectionsClick"
        >
          <span class="nav-item__icon"><FolderHeart :size="18" /></span>
          <span class="nav-item__label">{{ $t('sidebar.collections') }}</span>
        </button>
      </li>

      <!-- 人物墙（F6）：人脸识别聚类出的人物 -->
      <li>
        <button
          class="nav-item"
          :class="{ active: route.path.startsWith('/persons') }"
          @click="onPersonsClick"
        >
          <span class="nav-item__icon"><Users :size="18" /></span>
          <span class="nav-item__label">{{ $t('sidebar.persons') }}</span>
        </button>
      </li>

      <!-- 插件商店（T11）：exotic 格式插件的浏览/安装/激活入口 -->
      <li>
        <button
          class="nav-item"
          :class="{ active: route.path === '/plugins' }"
          @click="onPluginsClick"
        >
          <span class="nav-item__icon"><Puzzle :size="18" /></span>
          <span class="nav-item__label">{{ $t('sidebar.plugins') }}</span>
        </button>
      </li>

      <!-- 重复项：进入主画廊重复镜头（browse-only 浏览，方案 §4.3） -->
      <li>
        <button
          class="nav-item"
          :class="{ active: route.query.duplicates != null }"
          @click="onDuplicatesClick"
        >
          <span class="nav-item__icon"><CopyCheck :size="18" /></span>
          <span class="nav-item__label">{{ $t('sidebar.duplicates') }}</span>
        </button>
      </li>
    </ul>
  </AccordionSection>
</template>

<script setup lang="ts">
import { computed, markRaw } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  ImageIcon,
  Heart,
  Sparkles,
  Clock,
  Trash2,
  FolderHeart,
  Users,
  Puzzle,
  CopyCheck,
} from '@lucide/vue'
import AccordionSection from '../AccordionSection.vue'
import { useViewStore } from '../../../stores/viewStore'
import { useMediaStore } from '../../../stores/mediaStore'
import type { SmartAlbum } from '../../../types/ui'
import { smartAlbumToPath } from '../../../utils/viewRoute'

defineProps<{ order: number }>()

const viewStore = useViewStore()
const media = useMediaStore()
const router = useRouter()
const route = useRoute()
const { t } = useI18n()

// 智能相册——计数来自媒体统计（null = 不显示计数）。
const smartAlbums = computed(() => [
  {
    id: 'all' as const,
    icon: markRaw(ImageIcon),
    label: t('sidebar.allPhotos'),
    count: media.stats?.totalItems,
  },
  {
    id: 'favorites' as const,
    icon: markRaw(Heart),
    label: t('sidebar.favorites'),
    count: media.stats?.totalFavorited,
  },
  {
    id: 'live-photos' as const,
    icon: markRaw(Sparkles),
    label: t('sidebar.livePhotos'),
    count: media.stats?.totalLivePhotos,
  },
  { id: 'recent' as const, icon: markRaw(Clock), label: t('sidebar.recentlyAdded'), count: null },
  {
    id: 'trash' as const,
    icon: markRaw(Trash2),
    label: t('sidebar.trash'),
    count: media.stats?.totalDeleted,
  },
])

function formatCount(n: number | undefined | null): string {
  if (n == null) return ''
  if (n >= 1000) return (n / 1000).toFixed(1) + 'k'
  return String(n)
}

function onAlbumClick(albumId: SmartAlbum) {
  // 同步设 viewStore(保 MediaGrid getViewKey 读取时序)+ 导航到该 album 的可寻址路径(S2-c;深链/刷新可恢复)。
  // watcher 会因相等守卫跳过重复设值。
  viewStore.setSmartAlbum(albumId)
  const target = smartAlbumToPath(albumId)
  if (route.path !== target) router.push(target)
}

function onCollectionsClick() {
  if (route.path !== '/collections') router.push('/collections')
}

function onPersonsClick() {
  if (route.path !== '/persons') router.push('/persons')
}

function onPluginsClick() {
  if (route.path !== '/plugins') router.push('/plugins')
}

function onDuplicatesClick() {
  // 深链语义直推 URL（2026-09-02 方案 §4.3）：不走 duplicateLensStore.enterLens——它会把当前
  // fullPath 拍成返回快照，从侧栏（常已在「/」）点击时快照自指；这里只落
  // ?duplicates=groups，由 useGalleryQuerySync 的 route.query watcher 水合镜头 store。
  // 无返回快照 = 深链语义，退出镜头即回普通「/」。已在目标位置时 push 幂等（duplicated nav 直接 resolve）。
  void router.push({ path: '/', query: { duplicates: 'groups' } })
}
</script>

<style scoped>
.nav-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 0 var(--spacing-sm);
}
.nav-item {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  width: 100%;
  /* 图标列左缘对齐 --sidebar-indent(标题文字起点),子菜单嵌套在群组标签之下;
     减去 .nav-list 的横向 padding 得行内左缩进。 */
  padding: 6px var(--spacing-sm) 6px calc(var(--sidebar-indent, 30px) - var(--spacing-sm));
  border-radius: var(--radius-sm);
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  transition:
    background-color var(--transition-fast),
    color var(--transition-fast);
  text-align: left;
}
.nav-item:hover {
  background: var(--color-sidebar-hover-bg);
  color: var(--color-text-primary);
}
.nav-item.active {
  background: var(--color-sidebar-active-bg);
  color: var(--color-sidebar-active-text);
  font-weight: 500;
}
.nav-item__icon {
  width: 20px;
  display: inline-flex;
  justify-content: center;
}
.nav-item__label {
  flex: 1;
}
.nav-item__count {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  font-variant-numeric: tabular-nums;
}
</style>
