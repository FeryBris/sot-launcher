<script setup lang="ts">
import { computed } from 'vue'
import { confirmDependentImpact, deleteAddonNow, isAddonWanted, setAddonEnabled, store } from '../store'
import type { AddonEntry } from '../types'

const props = defineProps<{ addon: AddonEntry }>()

const locked = computed(() => props.addon.category === 'mandatory')
const wanted = computed(() => isAddonWanted(props.addon))

const installed = computed(() => store.installed?.addons[props.addon.name])

// sha256 is what actually decides whether a sync would re-download this addon
// (see sync_addons in Rust); the .toc "version" is separate, author-controlled
// text that doesn't always get bumped, so it's shown but not used for this check.
const status = computed<'synced' | 'pending' | 'removing' | 'off'>(() => {
  if (wanted.value) {
    const upToDate = !!installed.value && installed.value.filesPresent && installed.value.sha256 === props.addon.sha256
    return upToDate ? 'synced' : 'pending'
  }
  return installed.value ? 'removing' : 'off'
})

// Only worth calling out when they actually disagree — most addons keep these in sync.
const installedVersionMismatch = computed(
  () => !!installed.value && installed.value.version !== props.addon.version,
)

const statusLabel: Record<typeof status.value, string> = {
  synced: 'Synced',
  pending: 'Pending',
  removing: 'Will disable',
  off: 'Off',
}

function onToggle(e: Event) {
  const target = e.target as HTMLInputElement
  if (!target.checked && !confirmDependentImpact(props.addon.name, 'disable')) {
    target.checked = wanted.value // revert: nothing reactive changed, so the DOM won't re-sync on its own
    return
  }
  setAddonEnabled(props.addon.name, target.checked)
}

function onDelete() {
  if (!confirmDependentImpact(props.addon.name, 'delete')) return
  if (confirm(`Delete ${props.addon.name} completely? This can't be undone — you'd need to sync it again to get it back.`)) {
    deleteAddonNow(props.addon.name)
  }
}
</script>

<template>
  <label class="row" :class="{ dim: status === 'off' }">
    <div class="row-main">
      <input type="checkbox" :checked="wanted" :disabled="locked" @change="onToggle" />
      <div>
        <div class="name">{{ addon.name }}</div>
        <div class="meta">
          v{{ addon.version }}
          <span v-if="installedVersionMismatch" class="version-diff">(installed: v{{ installed?.version }})</span>
          <span v-if="addon.changelog"> — {{ addon.changelog }}</span>
        </div>
      </div>
    </div>
    <div class="row-actions">
      <span class="badge" :class="status">{{ statusLabel[status] }}</span>
      <button v-if="installed" class="delete-btn" title="Delete completely" @click.stop.prevent="onDelete">✕</button>
    </div>
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

.version-diff {
  color: var(--accent-strong);
}

.row-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.badge {
  font-size: 10px;
  padding: 3px 8px;
  border-radius: 10px;
  background: var(--bg-elevated);
  color: var(--text-dim);
  flex-shrink: 0;
}

.delete-btn {
  background: transparent;
  border: 1px solid var(--border);
  color: var(--text-dim);
  border-radius: var(--radius);
  width: 22px;
  height: 22px;
  font-size: 11px;
  line-height: 1;
  padding: 0;
}

.delete-btn:hover {
  border-color: var(--danger);
  color: var(--danger);
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
