<script setup lang="ts">
import { computed } from 'vue';
import { PhPlay } from '@phosphor-icons/vue';
import {
  currentGamePath,
  pendingCount as computePendingCount,
  pendingUpdateCount as computePendingUpdateCount,
  playNow,
  store,
  syncNow,
} from '../store';

const pendingCount = computed(computePendingCount);
const pendingUpdateCount = computed(computePendingUpdateCount);

const canPlay = computed(() => !!currentGamePath() && pendingCount.value === 0 && !store.syncing);
const canSync = computed(() => !!currentGamePath() && !!store.manifest && !store.syncing);

async function play() {
  await playNow();
}

function statusLabel() {
  if (!currentGamePath()) return 'Set your game folder in Settings first';
  if (store.syncing && store.syncProgress) {
    return `${store.syncProgress.stage} ${store.syncProgress.item} (${store.syncProgress.current}/${store.syncProgress.total})`;
  }
  if (store.syncing) return 'Syncing...';
  if (store.guildSyncing) {
    return store.guildSyncData.length === 1
      ? `Syncing ${store.guildSyncData[0].name}...`
      : `Syncing ${store.guildSyncData.length} characters...`;
  }
  if (store.guildRosterSyncing) return 'Syncing guild roster...';
  if (store.manifestError) return `Guild server error: ${store.manifestError}`;
  if (pendingUpdateCount.value > 0) return `${pendingUpdateCount.value} item(s) need updating`;
  if (pendingCount.value > 0) return 'Some addons were unchecked and need to be disabled';
  return 'Up to date';
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
          :style="{
            width: store.syncProgress
              ? `${(store.syncProgress.current / Math.max(store.syncProgress.total, 1)) * 100}%`
              : '10%',
          }"
        />
      </div>
    </div>

    <div class="actions">
      <button class="sync-btn" :disabled="!canSync" @click="syncNow">
        {{ pendingUpdateCount > 0 ? `Update (${pendingUpdateCount})` : 'Sync' }}
      </button>
      <button class="play-btn" :disabled="!canPlay" @click="play">
        Play
        <PhPlay :size="16" weight="duotone" />
      </button>
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
  color: #062028;
  border: none;
  border-radius: var(--radius);
  padding: 10px 28px;
  font-size: 14px;
  font-weight: 700;
  letter-spacing: 0.02em;
  display: flex;
  align-items: center;
  gap: 8px;
}

.play-btn:disabled {
  opacity: 0.4;
  cursor: default;
}
</style>
