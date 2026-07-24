<script setup lang="ts">
import { PhCircleNotch } from '@phosphor-icons/vue'

withDefaults(defineProps<{ label?: string; size?: number }>(), { size: 20 })
</script>

<template>
  <div class="loading-spinner">
    <PhCircleNotch :size="size" weight="bold" class="spin" />
    <span v-if="label">{{ label }}</span>
  </div>
</template>

<style scoped>
.loading-spinner {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
}

.spin {
  animation: spin 0.8s linear infinite;
  flex-shrink: 0;
}

@keyframes spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}
</style>
