<script setup lang="ts">
import { computed } from 'vue'
import { isAddonWanted, setAddonEnabled, store } from '../store'
import type { AddonEntry } from '../types'

const props = defineProps<{ addon: AddonEntry }>()

const locked = computed(() => props.addon.category === 'mandatory')
const wanted = computed(() => isAddonWanted(props.addon))

const status = computed<'synced' | 'pending' | 'removing' | 'off'>(() => {
  const installed = store.installed?.addons[props.addon.name]
  if (wanted.value) {
    const upToDate =
      !!installed && installed.filesPresent && installed.version === props.addon.version && installed.sha256 === props.addon.sha256
    return upToDate ? 'synced' : 'pending'
  }
  return installed ? 'removing' : 'off'
})

const statusLabel: Record<typeof status.value, string> = {
  synced: 'Synced',
  pending: 'Pending',
  removing: 'Will remove',
  off: 'Off',
}

function onToggle(e: Event) {
  setAddonEnabled(props.addon.name, (e.target as HTMLInputElement).checked)
}
</script>

<template>
  <label class="row" :class="{ dim: status === 'off' }">
    <div class="row-main">
      <input type="checkbox" :checked="wanted" :disabled="locked" @change="onToggle" />
      <div>
        <div class="name">{{ addon.name }}</div>
        <div class="meta">v{{ addon.version }}<span v-if="addon.changelog"> — {{ addon.changelog }}</span></div>
      </div>
    </div>
    <span class="badge" :class="status">{{ statusLabel[status] }}</span>
  </label>
</template>

<style scoped>
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 10px 14px;
  cursor: pointer;
}

.row.dim {
  opacity: 0.6;
}

.row-main {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}

.name {
  font-size: 13px;
  font-weight: 500;
}

.meta {
  font-size: 11px;
  color: var(--text-dim);
  margin-top: 2px;
}

.badge {
  font-size: 10px;
  padding: 3px 8px;
  border-radius: 10px;
  background: var(--bg-elevated);
  color: var(--text-dim);
  flex-shrink: 0;
}

.badge.synced {
  color: var(--success);
}

.badge.pending {
  color: var(--accent-strong);
}

.badge.removing {
  color: var(--danger);
}

input[type='checkbox'] {
  width: 15px;
  height: 15px;
  accent-color: var(--accent);
  flex-shrink: 0;
}

input[type='checkbox']:disabled {
  opacity: 0.6;
}
</style>
