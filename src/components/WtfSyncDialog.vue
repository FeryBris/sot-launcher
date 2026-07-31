<script setup lang="ts">
import { onMounted } from 'vue'
import { PhArrowsClockwise, PhCloudArrowDown, PhCloudArrowUp, PhCircleNotch } from '@phosphor-icons/vue'
import {
  store,
  refreshWtfAccountRows,
  saveWtfAccountAutoSyncEnabled,
  wtfPullWithConfirm,
  wtfPushWithConfirm,
} from '../store'

const emit = defineEmits<{ close: [] }>()

onMounted(() => {
  void refreshWtfAccountRows()
})

// No entry yet (never synced) still counts as enabled - matches
// WtfAccountSyncState.autoSyncEnabled's own true default.
function isAutoSyncEnabled(account: string): boolean {
  return store.settings.wtfAccountSync[account]?.autoSyncEnabled !== false
}

function onAutoSyncToggle(account: string, e: Event) {
  void saveWtfAccountAutoSyncEnabled(account, (e.target as HTMLInputElement).checked)
}

// updatedAt comes straight from the API's Storage::lastModified() (see
// WtfSyncController::statusPayload) - a Unix timestamp in SECONDS, like
// PHP's filemtime(), not the milliseconds the Date constructor expects.
// Passing it through unconverted landed every date around 21 Jan 1970.
function formatTime(at: number | null): string {
  return at ? new Date(at * 1000).toLocaleString() : 'Never'
}

function formatBytes(size: number | null): string {
  if (size == null) return ''
  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`
  return `${(size / 1024 / 1024).toFixed(1)} MB`
}
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div class="dialog">
      <div class="dialog-header">
        <h3>Settings Backup</h3>
        <button
          class="icon-btn"
          :disabled="store.wtfAccountRowsLoading"
          @click="refreshWtfAccountRows"
          title="Refresh"
        >
          <PhArrowsClockwise :size="15" weight="bold" :class="{ spin: store.wtfAccountRowsLoading }" />
        </button>
      </div>
      <p class="hint">
        Backs up your account/character settings and every addon's saved data (keybinds, macros,
        chat and UI layout, ElvUI/WeakAuras/DBM profiles, ...) to the cloud, per WoW account — one
        install can have several, each backed up independently. Back Up and Restore both ask for
        confirmation first if the other side changed since this device's last sync — nothing is
        ever overwritten silently. World of Warcraft must be closed to back up or restore.
      </p>

      <div class="account-list">
        <p v-if="store.wtfAccountRowsLoading" class="empty">Loading…</p>
        <p v-else-if="store.wtfAccountRows.length === 0" class="empty">
          No accounts found yet — log into the game at least once, or back up from another device
          first.
        </p>
        <template v-else>
          <div v-for="row in store.wtfAccountRows" :key="row.account" class="account-row">
            <div class="account-main">
              <div class="account-name">
                {{ row.account }}
                <span v-if="!row.existsLocally" class="badge cloud-only">Cloud only</span>
              </div>
              <div class="account-meta">
                Cloud:
                <template v-if="row.status.exists">
                  {{ formatTime(row.status.updatedAt) }} ({{ formatBytes(row.status.size) }})
                </template>
                <template v-else>none yet</template>
                <template v-if="row.localChanged"> — local changes pending</template>
              </div>
            </div>
            <div class="account-actions">
              <label class="auto-sync-toggle" title="Include this account in automatic backup/restore (Settings > Automatically back up/restore). Doesn't affect Back Up/Restore below.">
                <input
                  type="checkbox"
                  :checked="isAutoSyncEnabled(row.account)"
                  @change="onAutoSyncToggle(row.account, $event)"
                />
                Auto
              </label>
              <button
                class="btn"
                :disabled="!row.existsLocally || store.wtfSyncingAccount === row.account"
                :title="row.existsLocally ? 'Upload this device\'s settings to the cloud' : 'No local folder for this account on this device yet'"
                @click="wtfPushWithConfirm(row.account)"
              >
                <template v-if="store.wtfSyncingAccount === row.account && store.wtfSyncingDirection === 'backup'">
                  <PhCircleNotch :size="14" weight="bold" class="spin" />
                  Backing up…
                </template>
                <template v-else>
                  <PhCloudArrowUp :size="14" weight="bold" />
                  Back Up
                </template>
              </button>
              <button
                class="btn"
                :disabled="!row.status.exists || store.wtfSyncingAccount === row.account"
                :title="row.status.exists ? 'Download the cloud backup to this device' : 'No cloud backup for this account yet'"
                @click="wtfPullWithConfirm(row.account)"
              >
                <template v-if="store.wtfSyncingAccount === row.account && store.wtfSyncingDirection === 'restore'">
                  <PhCircleNotch :size="14" weight="bold" class="spin" />
                  Restoring…
                </template>
                <template v-else>
                  <PhCloudArrowDown :size="14" weight="bold" />
                  Restore
                </template>
              </button>
            </div>
          </div>
        </template>
      </div>
      <p v-if="store.wtfSyncError" class="hint error">{{ store.wtfSyncError }}</p>

      <div class="actions">
        <button class="btn" @click="emit('close')">Close</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.dialog {
  width: 560px;
  max-width: calc(100% - 48px);
  max-height: calc(100% - 96px);
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 20px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.4);
  display: flex;
  flex-direction: column;
}

.dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.dialog-header h3 {
  margin: 0;
  font-size: 15px;
  color: var(--text);
}

.icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  color: var(--text-dim);
  border-radius: var(--radius);
  width: 26px;
  height: 26px;
  flex-shrink: 0;
}

.icon-btn:hover {
  color: var(--text);
  border-color: var(--accent-dim);
}

.icon-btn:disabled {
  opacity: 0.6;
}

.hint {
  margin: 8px 0 14px;
  font-size: 11px;
  line-height: 1.5;
  color: var(--text-dim);
}

.hint.error {
  color: var(--danger);
}

.account-list {
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-height: 60px;
}

.empty {
  margin: 12px 0;
  font-size: 12px;
  color: var(--text-dim);
  text-align: center;
}

.account-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 8px 10px;
}

.account-main {
  min-width: 0;
}

.account-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
}

.badge {
  font-size: 10px;
  padding: 2px 7px;
  border-radius: 10px;
  background: var(--bg-panel);
  color: var(--text-dim);
  margin-left: 8px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.account-meta {
  font-size: 11px;
  color: var(--text-dim);
  margin-top: 3px;
}

.account-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.auto-sync-toggle {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  color: var(--text-dim);
  white-space: nowrap;
}

.actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 16px;
}

.btn {
  display: flex;
  align-items: center;
  gap: 6px;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  color: var(--text);
  border-radius: var(--radius);
  padding: 8px 16px;
  font-size: 12px;
  font-weight: 600;
}

.btn:hover:not(:disabled) {
  border-color: var(--accent-dim);
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

.btn:disabled {
  opacity: 0.5;
}
</style>
