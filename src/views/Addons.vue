<script setup lang="ts">
import { computed } from 'vue'
import { store, toggleOptional } from '../store'
import type { AddonEntry } from '../types'

const groups = computed(() => {
  const addons = store.manifest?.addons ?? []
  return {
    mandatory: addons.filter((a) => a.category === 'mandatory'),
    recommended: addons.filter((a) => a.category === 'recommended'),
    optional: addons.filter((a) => a.category === 'optional'),
  }
})

function isInstalled(addon: AddonEntry): boolean {
  const installed = store.lock?.addons[addon.name]
  return !!installed && installed.version === addon.version && installed.sha256 === addon.sha256
}
</script>

<template>
  <div class="addons">
    <div class="header">
      <h1>Addons</h1>
      <p>Mandatory addons sync automatically. Pick any optional ones you also want.</p>
    </div>

    <div v-if="store.loadingManifest" class="empty">Loading manifest...</div>
    <div v-else-if="store.manifestError" class="empty error">
      Couldn't reach the guild server. Check the API URL in Settings.
    </div>
    <div v-else-if="!store.manifest" class="empty">No manifest loaded.</div>

    <template v-else>
      <section v-if="groups.mandatory.length" class="group">
        <h2>Mandatory</h2>
        <div class="list">
          <div v-for="a in groups.mandatory" :key="a.name" class="row">
            <div class="row-main">
              <span class="lock">🔒</span>
              <div>
                <div class="name">{{ a.name }}</div>
                <div class="meta">v{{ a.version }}<span v-if="a.changelog"> — {{ a.changelog }}</span></div>
              </div>
            </div>
            <span class="badge" :class="{ ok: isInstalled(a) }">{{ isInstalled(a) ? 'Synced' : 'Pending' }}</span>
          </div>
        </div>
      </section>

      <section v-if="groups.recommended.length" class="group">
        <h2>Recommended</h2>
        <div class="list">
          <div v-for="a in groups.recommended" :key="a.name" class="row">
            <div class="row-main">
              <span class="lock">★</span>
              <div>
                <div class="name">{{ a.name }}</div>
                <div class="meta">v{{ a.version }}<span v-if="a.changelog"> — {{ a.changelog }}</span></div>
              </div>
            </div>
            <span class="badge" :class="{ ok: isInstalled(a) }">{{ isInstalled(a) ? 'Synced' : 'Pending' }}</span>
          </div>
        </div>
      </section>

      <section v-if="groups.optional.length" class="group">
        <h2>Optional</h2>
        <div class="list">
          <label v-for="a in groups.optional" :key="a.name" class="row row-checkable">
            <div class="row-main">
              <input
                type="checkbox"
                :checked="store.selectedOptional.has(a.name)"
                @change="toggleOptional(a.name)"
              />
              <div>
                <div class="name">{{ a.name }}</div>
                <div class="meta">v{{ a.version }}<span v-if="a.changelog"> — {{ a.changelog }}</span></div>
              </div>
            </div>
            <span
              v-if="store.selectedOptional.has(a.name)"
              class="badge"
              :class="{ ok: isInstalled(a) }"
            >
              {{ isInstalled(a) ? 'Synced' : 'Pending' }}
            </span>
          </label>
        </div>
      </section>
    </template>
  </div>
</template>

<style scoped>
.addons {
  padding: 24px 28px;
  max-width: 760px;
}

.header h1 {
  font-size: 20px;
  margin: 0 0 4px;
}

.header p {
  font-size: 12px;
  color: var(--text-dim);
  margin: 0 0 24px;
}

.empty {
  color: var(--text-dim);
  font-size: 13px;
}

.empty.error {
  color: var(--danger);
}

.group {
  margin-bottom: 22px;
}

.group h2 {
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--text-dim);
  margin: 0 0 8px;
}

.list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 10px 14px;
}

.row-checkable {
  cursor: pointer;
}

.row-main {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}

.lock {
  font-size: 12px;
  color: var(--accent);
  flex-shrink: 0;
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

.badge.ok {
  color: var(--success);
}

input[type='checkbox'] {
  width: 15px;
  height: 15px;
  accent-color: var(--accent);
}
</style>
