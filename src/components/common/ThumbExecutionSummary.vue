<template>
  <details v-if="scan.thumbGenExecutionDetails.length" class="thumb-execution">
    <summary>{{ scan.thumbGenHardwareLabel }}</summary>
    <ul>
      <li v-for="(detail, index) in scan.thumbGenExecutionDetails" :key="index">{{ detail }}</li>
    </ul>
  </details>
  <span v-else-if="scan.thumbGenHardwareLabel" class="thumb-execution">{{ scan.thumbGenHardwareLabel }}</span>
</template>

<script setup lang="ts">
import { useScanStore } from '../../stores/scanStore'

const scan = useScanStore()
</script>

<style scoped>
.thumb-execution {
  font-size: var(--font-size-xs);
  color: var(--text-secondary);
  overflow-wrap: anywhere;
}
summary { cursor: pointer; }
ul { margin: 4px 0; padding-left: 18px; }
</style>
