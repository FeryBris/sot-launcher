<script setup lang="ts">
import { computed } from 'vue'
import { pendingCount as computePendingCount, playNow, store, syncNow } from '../store'

const pendingCount = computed(computePendingCount)

const canPlay = computed(() => !!store.settings.gamePath && pendingCount.value === 0 && !store.syncing)
const canSync = computed(() => !!store.settings.gamePath && !!store.manifest && !store.syncing)

async function play() {
  await playNow()
}

function statusLabel() {
  if (!store.settings.gamePath) return 'Set your game folder in Settings first'
  if (store.syncing && store.syncProgress) {
    return `${store.syncProgress.stage} ${store.syncProgress.item} (${store.syncProgress.current}/${store.syncProgress.total})`
  }
  if (store.syncing) return 'Syncing...'
  if (store.manifestError) return 'Could not reach the guild server'
  if (pendingCount.value > 0) return `${pendingCount.value} item(s) need updating`
  return 'Up to date'
}
</script>

<template>
  <footer class="playbar">
    <div class="status">
      <div class="status-label">{{ statusLabel() }}</div>
      <div v-if="store.syncError" class="status-error">{{ store.syncError }}</div>
      <div v-if="store.syncing" class="progress-track">
        <div
          class="progress-fill"
          :style="{ width: store.syncProgress ? `${(store.syncProgress.current / Math.max(store.syncProgress.total, 1)) * 100}%` : '10%' }"
        />
      </div>
    </div>

    <div class="actions">
      <button class="sync-btn" :disabled="!canSync" @click="syncNow">
        {{ pendingCount > 0 ? `Update (${pendingCount})` : 'Sync' }}
      </button>
      <button class="play-btn" :disabled="!canPlay" @click="play">Play</button>
    </div>
  </footer>
</template>

<style scoped>
.playbar {
  position: fixed;
  left: 0;
  right: 0;
  bottom: 0;
  height: 64px;
  background: var(--bg-elevated);
  border-top: 1px solid var(--border);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 20px;
  gap: 16px;
}

.status {
  flex: 1;
  min-width: 0;
}

.status-label {
  font-size: 12px;
  color: var(--text-dim);
}

.status-error {
  font-size: 11px;
  color: var(--danger);
  margin-top: 2px;
}

.progress-track {
  margin-top: 6px;
  height: 4px;
  width: 240px;
  max-width: 100%;
  background: var(--bg-panel);
  border-radius: 2px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: var(--accent);
  transition: width 0.2s ease;
}

.actions {
  display: flex;
  gap: 10px;
  flex-shrink: 0;
}

.sync-btn {
  background: var(--bg-panel);
  border: 1px solid var(--border);
  color: var(--text);
  border-radius: var(--radius);
  padding: 10px 16px;
  font-size: 13px;
  font-weight: 500;
}

.sync-btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.play-btn {
  background: linear-gradient(135deg, var(--accent-strong), var(--accent));
  color: #1a1408;
  border: none;
  border-radius: var(--radius);
  padding: 10px 28px;
  font-size: 14px;
  font-weight: 700;
  letter-spacing: 0.02em;
}

.play-btn:disabled {
  opacity: 0.4;
  cursor: default;
}
</style>
