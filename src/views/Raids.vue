<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import * as api from '../api'
import type { SyncableRaid } from '../types'

/** Parsed shape of HeadCount's own exportJSON — only the fields this view
 *  needs to show a human-readable summary; the full string is what
 *  actually gets sent, untouched, when syncing. */
interface RaidSummary {
  zone: string
  difficulty: string
  start: string
  end: string
  playerCount: number
  bossKillCount: number
}

interface RaidRow {
  raid: SyncableRaid
  summary: RaidSummary | null
  isMain: boolean
  syncing: boolean
  syncError: string
  synced: boolean
  ignoring: boolean
  ignored: boolean
}

const rows = ref<RaidRow[]>([])
const loading = ref(true)
const loadError = ref('')

function parseSummary(raidJson: string): RaidSummary | null {
  try {
    const parsed = JSON.parse(raidJson)
    return {
      zone: parsed.zone ?? 'Unknown',
      difficulty: parsed.difficulty ?? 'Unknown',
      start: parsed.start ?? '',
      end: parsed.end ?? '',
      playerCount: Array.isArray(parsed.players) ? parsed.players.length : 0,
      bossKillCount: Array.isArray(parsed.bossKills) ? parsed.bossKills.length : 0,
    }
  } catch {
    return null
  }
}

/** Short raid-group formations that never actually entered an instance
 *  (no zone, no boss kills) — HeadCount tracks these like any other raid,
 *  but they're not something a raid leader would ever want to sync. */
function isWorthShowing(summary: RaidSummary | null): boolean {
  if (!summary) return false
  return summary.zone !== 'None' && summary.bossKillCount > 0
}

async function load() {
  loading.value = true
  loadError.value = ''
  try {
    const raids = await api.getCapturedRaids()
    rows.value = raids
      .map((raid) => ({
        raid,
        summary: parseSummary(raid.raidJson),
        isMain: true,
        syncing: false,
        syncError: '',
        synced: raid.alreadySynced,
        ignoring: false,
        ignored: raid.isIgnored,
      }))
      .filter((row) => isWorthShowing(row.summary))
  } catch (e) {
    loadError.value = String(e)
  } finally {
    loading.value = false
  }
}

onMounted(load)

const hasRows = computed(() => rows.value.length > 0)

async function syncRow(row: RaidRow) {
  row.syncing = true
  row.syncError = ''
  try {
    await api.syncRaidToWeb(row.raid.raidId, row.isMain)
    row.synced = true
  } catch (e) {
    row.syncError = String(e)
  } finally {
    row.syncing = false
  }
}

async function toggleIgnore(row: RaidRow) {
  row.ignoring = true
  try {
    await api.setRaidIgnored(row.raid.raidId, !row.ignored)
    row.ignored = !row.ignored
  } catch (e) {
    row.syncError = String(e)
  } finally {
    row.ignoring = false
  }
}
</script>

<template>
  <div class="raids">
    <div class="header">
      <h1>Raids</h1>
      <button class="refresh-btn" :disabled="loading" @click="load">
        {{ loading ? 'Loading…' : 'Refresh' }}
      </button>
    </div>
    <p class="hint">
      Captured automatically by the SoTRaidLootSync addon from HeadCount (and RCLootCouncil, if
      used that night). Review and sync each raid to the website — a raid with no loot council
      data still syncs fine, just without response/vote info.
    </p>

    <p v-if="loadError" class="empty error">{{ loadError }}</p>
    <p v-else-if="!loading && !hasRows" class="empty">
      No raids to sync yet. Make sure SoTRaidLootSync is installed and a raid has been finalized
      in HeadCount.
    </p>

    <div v-else class="raid-list">
      <div
        v-for="row in rows"
        :key="row.raid.raidId"
        class="raid-card"
        :class="{ synced: row.synced, ignored: row.ignored }"
      >
        <div class="raid-main">
          <div class="raid-title">
            <span class="zone">{{ row.summary?.zone }}</span>
            <span class="difficulty">{{ row.summary?.difficulty }}</span>
            <span v-if="row.ignored" class="ignored-badge">Ignored</span>
          </div>
          <div class="raid-meta">
            {{ row.summary?.start }} – {{ row.summary?.end }}
          </div>
          <div class="raid-meta">
            {{ row.summary?.playerCount }} players · {{ row.summary?.bossKillCount }} boss kills ·
            {{ row.raid.lootJson ? 'loot council data included' : 'no loot council data' }}
          </div>
          <p v-if="row.syncError" class="raid-error">{{ row.syncError }}</p>
        </div>

        <div class="raid-actions">
          <template v-if="!row.ignored">
            <label class="main-toggle">
              <input type="checkbox" v-model="row.isMain" :disabled="row.synced || row.syncing" />
              Main raid
            </label>
            <button
              class="sync-btn"
              :disabled="row.synced || row.syncing"
              @click="syncRow(row)"
            >
              {{ row.synced ? 'Synced' : row.syncing ? 'Syncing…' : 'Sync Raid' }}
            </button>
          </template>
          <button
            v-if="!row.synced"
            class="ignore-btn"
            :disabled="row.ignoring"
            @click="toggleIgnore(row)"
          >
            {{ row.ignoring ? 'Working…' : row.ignored ? 'Unignore' : 'Ignore' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.raids {
  padding: 24px 28px;
  max-width: 720px;
}

.header {
  display: flex;
  align-items: center;
  justify-content: space-between;
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
  margin: 10px 0 20px;
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

.raid-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.raid-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 14px 16px;
}

.raid-card.synced {
  opacity: 0.6;
}

.raid-card.ignored {
  opacity: 0.5;
}

.ignored-badge {
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.03em;
  color: var(--text-dim);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 2px 6px;
}

.raid-title {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.zone {
  font-size: 14px;
  font-weight: 700;
  color: var(--text);
}

.difficulty {
  font-size: 11px;
  color: var(--accent-strong);
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.raid-meta {
  font-size: 11px;
  color: var(--text-dim);
  margin-top: 4px;
}

.raid-error {
  margin: 6px 0 0;
  font-size: 11px;
  font-weight: 600;
  color: var(--danger);
}

.raid-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 8px;
  flex-shrink: 0;
}

.main-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--text-dim);
  white-space: nowrap;
  cursor: pointer;
}

.main-toggle input {
  width: 13px;
  height: 13px;
  accent-color: var(--accent);
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
