<template>
  <div ref="host" class="vditor-document" />
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import Vditor from 'vditor'
import 'vditor/dist/index.css'

const props = defineProps<{
  itemId: number
  value: string
}>()

const host = ref<HTMLElement | null>(null)
let editor: Vditor | null = null
let disposed = false

onMounted(() => {
  if (!host.value) return

  editor = new Vditor(host.value, {
    value: props.value,
    height: '100%',
    cdn: '/vditor',
    cache: {
      id: `scrollery-vditor-${props.itemId}`,
    },
    // Vditor 初始化会异步加载本地语言脚本；组件可能在初始化完成前卸载。
    after: () => {
      if (disposed && editor?.vditor) {
        editor.destroy()
        editor = null
      }
    },
  })
})

onBeforeUnmount(() => {
  disposed = true
  if (editor?.vditor) {
    editor.destroy()
    editor = null
  }
})
</script>

<style scoped>
.vditor-document {
  width: 100%;
  height: 100%;
  min-height: 0;
}

:deep(.vditor) {
  height: 100%;
  border: 0;
  border-radius: 0;
}
</style>
