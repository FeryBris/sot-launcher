<script setup lang="ts">
import { computed } from 'vue'
import { confirmDelete, confirmDisable, deleteAddonNow, isAddonWanted, setAddonEnabled, store } from '../store'
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

async function onToggle(e: Event) {
  const target = e.target as HTMLInputElement
  const checked = target.checked
  if (!checked && !(await confirmDisable(props.addon.name))) {
    target.checked = wanted.value // revert: nothing reactive changed, so the DOM won't re-sync on its own
    return
  }
  setAddonEnabled(props.addon.name, checked)
}

async function onDelete() {
  if (await confirmDelete(props.addon.name)) {
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
        <div v-if="addon.description" class="description">{{ addon.description }}</div>
        <div class="meta">
          v{{ addon.version }}
          <span v-if="installedVersionMismatch" class="version-diff">(installed: v{{ installed?.version }})</span>
          <span v-if="addon.changelog"> — {{ addon.changelog }}</span>
        </div>
        <div v-if="installed?.dependencies.length" class="requires">Requires: {{ installed.dependencies.join(', ') }}</div>
      </div>
    </div>
    <div class="row-actions">
      <span class="badge" :class="status">{{ statusLabel[status] }}</span>
      <button v-if="installed" class="delete-btn" title="Delete (moves to Trash)" @click.stop.prevent="onDelete">✕</button>
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

.description {
  font-size: 11px;
  color: var(--text-dim);
  margin-top: 2px;
  line-height: 1.4;
}

.meta {
  font-size: 11px;
  color: var(--text-dim);
  margin-top: 2px;
}

.version-diff {
  color: var(--accent-strong);
}

.requires {
  font-size: 10px;
  color: var(--accent-strong);
  margin-top: 3px;
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
