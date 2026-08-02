<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import * as api from '../api'
import { formatDateTime } from '../format'
import { saveCombatLogAutoSync, store } from '../store'
import type { SyncableEncounter } from '../types'

interface EncounterRow {
  encounter: SyncableEncounter
  syncing: boolean
  syncError: string
  synced: boolean
  ignoring: boolean
  ignored: boolean
}

const rows = ref<EncounterRow[]>([])
const loading = ref(true)
const loadError = ref('')
const reprocessing = ref(false)
const reprocessError = ref('')

function toRow(encounter: SyncableEncounter): EncounterRow {
  return {
    encounter,
    syncing: false,
    syncError: '',
    synced: encounter.alreadySynced,
    ignoring: false,
    ignored: encounter.isIgnored,
  }
}

async function load() {
  loading.value = true
  loadError.value = ''
  try {
    const encounters = await api.getCapturedCombatSessions()
    // Newest first - matches Raids' convention of most-recent-activity-first.
    rows.value = encounters.map(toRow).sort((a, b) => b.encounter.endTimeMs - a.encounter.endTimeMs)
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
  await api.onCombatEncounterCompleted((encounter) => {
    // A freshly captured encounter is never already-synced/ignored - the
    // watcher only just finalized it, so there's been no chance to do either.
    rows.value.unshift(toRow({ ...encounter, alreadySynced: false, isIgnored: false }))
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

/** endTimeMs is real epoch milliseconds - the log itself has no year, so
 *  the Rust side guesses it from today's date (see combat_log_parser.rs's
 *  guess_year), accurate as long as this log isn't over a year old. */
function formatWhen(ms: number): string {
  return formatDateTime(new Date(ms))
}

function formatNumber(n: number): string {
  return Math.round(n).toLocaleString()
}

/** Best-effort label - the first (most-damaged) NPC name plus a count of
 *  any others involved (adds, trash before the boss). No attempt to guess
 *  which one is "the boss" beyond that - see CombatEncounterReport's doc
 *  comment for why that's left to the backend. */
function encounterTitle(encounter: SyncableEncounter): string {
  if (encounter.npcNames.length === 0) return 'Unknown encounter'
  const [primary, ...rest] = encounter.npcNames
  return rest.length > 0 ? `${primary} (+${rest.length} more)` : primary
}

function topPlayers(encounter: SyncableEncounter, count = 3): string {
  return encounter.players
    .slice(0, count)
    .map((p) => `${p.name} (${formatNumber(p.dps)} dps)`)
    .join(', ')
}

async function syncRow(row: EncounterRow) {
  row.syncing = true
  row.syncError = ''
  try {
    await api.syncCombatSessionToWeb(row.encounter.sessionId)
    row.synced = true
  } catch (e) {
    row.syncError = String(e)
  } finally {
    row.syncing = false
  }
}

async function toggleIgnore(row: EncounterRow) {
  row.ignoring = true
  try {
    await api.setCombatSessionIgnored(row.encounter.sessionId, !row.ignored)
    row.ignored = !row.ignored
  } catch (e) {
    row.syncError = String(e)
  } finally {
    row.ignoring = false
  }
}

function onAutoSyncToggle(e: Event) {
  void saveCombatLogAutoSync((e.target as HTMLInputElement).checked)
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
      Every pull of at least a few seconds shows up here, including solo/world content — the
      "type" shown per session is just a best-effort guess, not a filter, so ignore whatever isn't
      worth keeping. Boss/instance identity isn't resolved yet — that happens once a session is
      synced to the website.
    </p>
    <p v-if="reprocessError" class="empty error">{{ reprocessError }}</p>

    <label class="auto-sync-toggle">
      <input
        type="checkbox"
        :checked="store.settings.combatLogAutoSync"
        @change="onAutoSyncToggle"
      />
      Automatically sync new sessions
    </label>

    <p v-if="loadError" class="empty error">{{ loadError }}</p>
    <p v-else-if="!loading && !hasRows" class="empty">
      No sessions captured yet. Make sure combat logging is enabled in-game, then run a dungeon or
      raid pull.
    </p>

    <template v-else>
      <div class="log-list">
        <div
          v-for="row in activeRows"
          :key="row.encounter.sessionId"
          class="log-card"
          :class="{ synced: row.synced }"
        >
          <div class="log-main">
            <div class="log-title">
              <span class="npc-name">{{ encounterTitle(row.encounter) }}</span>
              <span class="content-type">{{ row.encounter.contentType }}</span>
            </div>
            <div class="log-meta">{{ formatWhen(row.encounter.endTimeMs) }}</div>
            <div class="log-meta">
              {{ formatDuration(row.encounter.durationSec) }} · {{ row.encounter.players.length }}
              players
            </div>
            <div v-if="row.encounter.players.length" class="log-meta">
              {{ topPlayers(row.encounter) }}
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
            :key="row.encounter.sessionId"
            class="log-card ignored"
          >
            <div class="log-main">
              <div class="log-title">
                <span class="npc-name">{{ encounterTitle(row.encounter) }}</span>
                <span class="content-type">{{ row.encounter.contentType }}</span>
              </div>
              <div class="log-meta">{{ formatWhen(row.encounter.endTimeMs) }}</div>
              <div class="log-meta">
                {{ formatDuration(row.encounter.durationSec) }} · {{ row.encounter.players.length }}
                players
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

.auto-sync-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-dim);
  margin-bottom: 16px;
  cursor: pointer;
}

.auto-sync-toggle input {
  width: 13px;
  height: 13px;
  accent-color: var(--accent);
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

.content-type {
  font-size: 11px;
  color: var(--accent-strong);
  text-transform: uppercase;
  letter-spacing: 0.03em;
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
