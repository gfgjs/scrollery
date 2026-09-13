<template>
  <span
    v-if="progress"
    class="scan-progress"
    :title="progress.currentDir || undefined"
  >
    <span class="spinner" />
    <span class="scan-progress__text">{{ statusText }}</span>
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useScanStore } from '../../stores/scanStore'
import { formatFileSize } from '../../utils/format'

const scan = useScanStore()
const { t } = useI18n()

const progress = computed(() => scan.aggregateProgress)

const statusText = computed(() => {
  const current = progress.value
  if (!current) return ''

  const phase =
    current.phase === 'enriching'
      ? t('statusbar.scanPhaseMetadata')
      : current.phase === 'scanning'
        ? t('statusbar.scanPhaseScanning')
        : t('statusbar.scanPhaseDiscovering')
  const processed = formatFileSize(current.processedBytes)
  const files =
    current.totalFiles === null
      ? t('statusbar.scanFilesProcessed', { count: current.processedFiles.toLocaleString() })
      : t('statusbar.scanFilesProgress', {
          processed: current.processedFiles.toLocaleString(),
          total: current.totalFiles.toLocaleString(),
        })
  const roots = current.rootCount > 1 ? ` · ${current.rootCount} ${t('statusbar.scanRoots')}` : ''

  if (current.totalBytes === null) {
    return t('statusbar.scanProgressUnknown', { phase, processed, files }) + roots
  }
  return (
    t('statusbar.scanProgressKnown', {
      phase,
      processed,
      total: formatFileSize(current.totalBytes),
      files,
    }) + roots
  )
})
</script>

<style scoped>
.scan-progress {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  min-width: 0;
  max-width: min(48vw, 420px);
  color: var(--color-accent);
  white-space: nowrap;
}

.scan-progress__text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
