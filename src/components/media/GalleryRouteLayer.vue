<template>
  <MediaGrid />
</template>

<script lang="ts">
import { defineAsyncComponent, defineComponent } from 'vue'

// 路由壳本身极薄且可由 App 持续复用；高成本画廊仍保留独立 chunk，不能为了 `/view` 覆盖层
// 语义把 MediaGrid 静态拉进入口包。
const MediaGrid = defineAsyncComponent(() => import('./MediaGrid.vue'))

export default defineComponent({
  name: 'GalleryRouteLayer',
  components: { MediaGrid },
})
</script>
