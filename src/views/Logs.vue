<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import * as api from '../api'
import { formatDateTime } from '../format'
import type { SyncableCombatLogSession } from '../types'

interface SessionRow {
  session: SyncableCombatLogSession
  syncing: boolean
  syncError: string
  synced: boolean
  ignoring: boolean
  ignored: boolean
}

const rows = ref<SessionRow[]>([])
const loading = ref(true)
const loadError = ref('')
const reprocessing = ref(false)
const reprocessError = ref('')

function toRow(session: SyncableCombatLogSession): SessionRow {
  return {
    session,
    syncing: false,
    syncError: '',
    synced: session.alreadySynced,
    ignoring: false,
    ignored: session.isIgnored,
  }
}

async function load() {
  loading.value = true
  loadError.value = ''
  try {
    const sessions = await api.getCapturedCombatSessions()
    // Newest first - matches Raids' convention of most-recent-activity-first.
    rows.value = sessions.map(toRow).sort((a, b) => b.session.endTimeMs - a.session.endTimeMs)
  } catch (e) {
    loadError.value = String(e)
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  await load()
  // Fire-and-forget, same convention as App.vue's own listeners - this
  // view stays mounted (v-show, not v-if) for the app's whole lifetime,
  // so there's no unmount to unlisten on.
  await api.onCombatLogSessionCompleted((session) => {
    // A freshly captured session is never already-synced/ignored - the
    // watcher only just finalized it, so there's been no chance to do either.
    rows.value.unshift(toRow({ ...session, alreadySynced: false, isIgnored: false }))
  })
})

const hasRows = computed(() => rows.value.length > 0)
const activeRows = computed(() => rows.value.filter((r) => !r.ignored))
const ignoredRows = computed(() => rows.value.filter((r) => r.ignored))

/** Discards the saved checkpoint and reprocesses the whole current combat
 *  log as historical backlog - see api.reprocessCombatLog's doc comment
 *  for why this needs a manual trigger (a checkpoint saved by an older
 *  launcher build otherwise silently blocks it forever). */
async function reprocess() {
  reprocessing.value = true
  reprocessError.value = ''
  try {
    await api.reprocessCombatLog()
    // Reprocessing runs on a background thread and takes a moment for a
    // large log - give it a beat before refreshing so the first load
    // isn't just an empty in-progress state.
    await new Promise((resolve) => setTimeout(resolve, 1500))
    await load()
  } catch (e) {
    reprocessError.value = String(e)
  } finally {
    reprocessing.value = false
  }
}

function formatDuration(sec: number): string {
  const m = Math.floor(sec / 60)
  const s = Math.round(sec % 60)
  return m > 0 ? `${m}m ${s}s` : `${s}s`
}

/** startTimeMs/endTimeMs are real epoch milliseconds - the log itself has
 *  no year, so the Rust side guesses it from today's date (see
 *  combat_log_parser.rs's guess_year), accurate as long as this log isn't
 *  over a year old. */
function formatWhen(ms: number): string {
  return formatDateTime(new Date(ms))
}

async function syncRow(row: SessionRow) {
  row.syncing = true
  row.syncError = ''
  try {
    await api.syncCombatSessionToWeb(row.session.sessionId)
    row.synced = true
  } catch (e) {
    row.syncError = String(e)
  } finally {
    row.syncing = false
  }
}

async function toggleIgnore(row: SessionRow) {
  row.ignoring = true
  try {
    await api.setCombatSessionIgnored(row.session.sessionId, !row.ignored)
    row.ignored = !row.ignored
  } catch (e) {
    row.syncError = String(e)
  } finally {
    row.ignoring = false
  }
}
</script>

<template>
  <div class="logs">
    <div class="header">
      <h1>Logs</h1>
      <div class="header-actions">
        <button
          class="refresh-btn"
          :disabled="reprocessing"
          title="Discards the saved read position and reprocesses the whole current combat log from the start - use this if sessions you know are in the file aren't showing up"
          @click="reprocess"
        >
          {{ reprocessing ? 'Reprocessing…' : 'Reprocess whole log' }}
        </button>
        <button class="refresh-btn" :disabled="loading" @click="load">
          {{ loading ? 'Loading…' : 'Refresh' }}
        </button>
      </div>
    </div>
    <p class="hint">
      Captured automatically from WoWCombatLog.txt while you play — requires combat logging
      enabled in-game (<code>/combatlog</code>, or Interface Options &gt; Advanced Combat Logging).
      Every ~30 minutes of activity becomes one session here, cut at a quiet moment rather than a
      hard time limit so a pull is never split across two sessions. Nothing is analyzed
      client-side any more — boss/pull identity, players, and stats are all resolved once a
      session is synced to the website. Sync is manual, same as Raids.
    </p>
    <p v-if="reprocessError" class="empty error">{{ reprocessError }}</p>

    <p v-if="loadError" class="empty error">{{ loadError }}</p>
    <p v-else-if="!loading && !hasRows" class="empty">
      No sessions captured yet. Make sure combat logging is enabled in-game, then run a dungeon or
      raid pull.
    </p>

    <template v-else>
      <div class="log-list">
        <div
          v-for="row in activeRows"
          :key="row.session.sessionId"
          class="log-card"
          :class="{ synced: row.synced }"
        >
          <div class="log-main">
            <div class="log-title">
              <span class="npc-name">{{ formatWhen(row.session.startTimeMs) }}</span>
            </div>
            <div class="log-meta">
              {{ formatDuration((row.session.endTimeMs - row.session.startTimeMs) / 1000) }} ·
              {{ row.session.lineCount.toLocaleString() }} lines
            </div>
            <p v-if="row.syncError" class="log-error">{{ row.syncError }}</p>
          </div>

          <div class="log-actions">
            <button class="sync-btn" :disabled="row.synced || row.syncing" @click="syncRow(row)">
              {{ row.synced ? 'Synced' : row.syncing ? 'Syncing…' : 'Sync' }}
            </button>
            <button class="ignore-btn" :disabled="row.ignoring" @click="toggleIgnore(row)">
              {{ row.ignoring ? 'Working…' : 'Ignore' }}
            </button>
          </div>
        </div>
        <p v-if="activeRows.length === 0" class="empty">All captured sessions are ignored.</p>
      </div>

      <template v-if="ignoredRows.length">
        <h2 class="section-heading">Ignored ({{ ignoredRows.length }})</h2>
        <div class="log-list">
          <div
            v-for="row in ignoredRows"
            :key="row.session.sessionId"
            class="log-card ignored"
          >
            <div class="log-main">
              <div class="log-title">
                <span class="npc-name">{{ formatWhen(row.session.startTimeMs) }}</span>
              </div>
              <div class="log-meta">
                {{ formatDuration((row.session.endTimeMs - row.session.startTimeMs) / 1000) }} ·
                {{ row.session.lineCount.toLocaleString() }} lines
              </div>
            </div>

            <div class="log-actions">
              <button class="ignore-btn" :disabled="row.ignoring" @click="toggleIgnore(row)">
                {{ row.ignoring ? 'Working…' : 'Unignore' }}
              </button>
            </div>
          </div>
        </div>
      </template>
    </template>
  </div>
</template>

<style scoped>
.logs {
  padding: 24px 28px;
  max-width: 720px;
}

.header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.header-actions {
  display: flex;
  gap: 8px;
}

h1 {
  font-size: 20px;
  margin: 0;
}

.refresh-btn {
  background: var(--bg-panel);
  border: 1px solid var(--border);
  color: var(--text);
  border-radius: var(--radius);
  padding: 7px 14px;
  font-size: 12px;
  font-weight: 600;
}

.refresh-btn:hover {
  border-color: var(--accent-dim);
}

.hint {
  margin: 10px 0 16px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-dim);
}

.empty {
  margin: 20px 0;
  font-size: 13px;
  color: var(--text-dim);
  text-align: center;
}

.empty.error {
  color: var(--danger);
}

.log-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.log-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 14px 16px;
}

.log-card.synced {
  opacity: 0.6;
}

.log-card.ignored {
  opacity: 0.5;
}

.section-heading {
  font-size: 12px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--text-dim);
  margin: 20px 0 10px;
}

.log-title {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.npc-name {
  font-size: 14px;
  font-weight: 700;
  color: var(--text);
}

.log-meta {
  font-size: 11px;
  color: var(--text-dim);
  margin-top: 4px;
}

.log-error {
  margin: 6px 0 0;
  font-size: 11px;
  font-weight: 600;
  color: var(--danger);
}

.log-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 8px;
  flex-shrink: 0;
}

.sync-btn {
  background: var(--accent);
  color: #062028;
  border: none;
  border-radius: var(--radius);
  padding: 8px 16px;
  font-size: 12px;
  font-weight: 700;
  white-space: nowrap;
}

.sync-btn:disabled {
  opacity: 0.6;
  cursor: default;
}

.ignore-btn {
  background: transparent;
  color: var(--text-dim);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 8px 16px;
  font-size: 12px;
  font-weight: 600;
  white-space: nowrap;
}

.ignore-btn:hover:not(:disabled) {
  border-color: var(--accent-dim);
  color: var(--text);
}

.ignore-btn:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>
