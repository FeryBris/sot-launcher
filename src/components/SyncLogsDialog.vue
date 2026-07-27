<script setup lang="ts">
import { onMounted, ref } from 'vue'
import * as api from '../api'
import type { SyncLogEntry } from '../types'

const emit = defineEmits<{ close: [] }>()

const entries = ref<SyncLogEntry[]>([])
const loading = ref(true)
const loadError = ref('')

async function load() {
  loading.value = true
  loadError.value = ''
  try {
    entries.value = await api.getSyncLog()
  } catch (e) {
    loadError.value = String(e)
  } finally {
    loading.value = false
  }
}

onMounted(load)

function formatTime(at: number): string {
  return new Date(at).toLocaleString()
}

function kindLabel(kind: SyncLogEntry['kind']): string {
  return kind === 'characters' ? 'Character Data' : kind === 'guild-roster' ? 'Guild Roster' : kind
}
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div class="dialog">
      <div class="dialog-header">
        <h3>Sync Logs</h3>
        <button class="icon-btn" :disabled="loading" @click="load" title="Refresh">⟳</button>
      </div>
      <p class="hint">
        Every attempt to sync your character or guild roster data to the website — including why
        one didn't go through, if it didn't.
      </p>

      <div class="log-list">
        <p v-if="loading" class="empty">Loading…</p>
        <p v-else-if="loadError" class="empty error">{{ loadError }}</p>
        <p v-else-if="entries.length === 0" class="empty">No syncs recorded yet.</p>
        <template v-else>
          <div
            v-for="(entry, index) in entries"
            :key="index"
            class="log-row"
            :class="{ failed: !entry.success }"
          >
            <span class="log-status" :class="entry.success ? 'ok' : 'fail'">
              {{ entry.success ? 'OK' : 'Failed' }}
            </span>
            <div class="log-body">
              <div class="log-line">
                <span class="log-kind">{{ kindLabel(entry.kind) }}</span>
                <span class="log-time">{{ formatTime(entry.at) }}</span>
              </div>
              <p v-if="entry.error" class="log-error">{{ entry.error }}</p>
            </div>
          </div>
        </template>
      </div>

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
  width: 480px;
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
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  color: var(--text-dim);
  border-radius: var(--radius);
  width: 26px;
  height: 26px;
  font-size: 14px;
  line-height: 1;
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

.log-list {
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

.empty.error {
  color: var(--danger);
}

.log-row {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 8px 10px;
}

.log-row.failed {
  border-color: var(--danger);
}

.log-status {
  flex-shrink: 0;
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  border-radius: 4px;
  padding: 3px 6px;
  margin-top: 1px;
}

.log-status.ok {
  color: var(--accent-strong);
  background: color-mix(in srgb, var(--accent) 16%, transparent);
}

.log-status.fail {
  color: var(--danger);
  background: color-mix(in srgb, var(--danger) 16%, transparent);
}

.log-body {
  flex: 1;
  min-width: 0;
}

.log-line {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
}

.log-kind {
  font-size: 12px;
  font-weight: 600;
  color: var(--text);
}

.log-time {
  font-size: 11px;
  color: var(--text-dim);
  flex-shrink: 0;
}

.log-error {
  margin: 4px 0 0;
  font-size: 11px;
  color: var(--danger);
  word-break: break-word;
}

.actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 16px;
}

.btn {
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  color: var(--text);
  border-radius: var(--radius);
  padding: 8px 16px;
  font-size: 12px;
  font-weight: 600;
}

.btn:hover {
  border-color: var(--accent-dim);
}
</style>
