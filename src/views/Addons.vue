<script setup lang="ts">
import { computed, ref } from 'vue'
import { confirmDelete, confirmDisable, deleteAddonNow, isAddonWanted, refreshManifest, setAddonDisabledNow, store } from '../store'
import AddonRow from '../components/AddonRow.vue'

const search = ref('')
const hideDisabled = ref(false)

const byName = (a: { name: string }, b: { name: string }) => a.name.localeCompare(b.name)

const groups = computed(() => {
  const query = search.value.trim().toLowerCase()
  let addons = (store.manifest?.addons ?? []).filter((a) => a.name.toLowerCase().includes(query))
  if (hideDisabled.value) {
    addons = addons.filter((a) => isAddonWanted(a))
  }
  return {
    mandatory: addons.filter((a) => a.category === 'mandatory').sort(byName),
    recommended: addons.filter((a) => a.category === 'recommended').sort(byName),
    optional: addons.filter((a) => a.category === 'optional').sort(byName),
  }
})

const customAddons = computed(() => {
  const query = search.value.trim().toLowerCase()
  return Object.values(store.installed?.addons ?? {})
    .filter((a) => a.type === 'custom' && a.name.toLowerCase().includes(query))
    .filter((a) => !hideDisabled.value || !a.disabled)
    .sort(byName)
})

const hasResults = computed(
  () => groups.value.mandatory.length + groups.value.recommended.length + groups.value.optional.length + customAddons.value.length > 0,
)

async function onDeleteCustom(name: string) {
  if (await confirmDelete(name)) {
    deleteAddonNow(name)
  }
}

async function onToggleCustom(name: string, currentlyDisabled: boolean) {
  const disabling = !currentlyDisabled
  if (disabling) {
    if (await confirmDisable(name)) setAddonDisabledNow(name, true)
  } else {
    setAddonDisabledNow(name, false) // re-enabling isn't destructive, no confirm needed
  }
}
</script>

<template>
  <div class="addons">
    <div class="header">
      <div class="header-top">
        <h1>Addons</h1>
        <button class="refresh-btn" :disabled="store.loadingManifest" @click="refreshManifest">
          {{ store.loadingManifest ? 'Refreshing...' : 'Refresh' }}
        </button>
      </div>
      <p>Mandatory addons always sync. Uncheck anything else to leave it out — unchecking something already installed moves it to Interface/.DisabledAddons on the next sync (recoverable by re-checking, or use the ✕ to delete it for good).</p>
      <div v-if="store.manifest" class="filters">
        <input v-model="search" type="text" class="search-input" placeholder="Search addons..." />
        <label class="hide-disabled">
          <input type="checkbox" v-model="hideDisabled" />
          Hide disabled
        </label>
      </div>
    </div>

    <div v-if="store.loadingManifest" class="empty">Loading manifest...</div>
    <div v-else-if="store.manifestError" class="empty error">
      Couldn't reach the guild server: {{ store.manifestError }}
    </div>
    <div v-else-if="!store.manifest" class="empty">No manifest loaded.</div>
    <div v-else-if="!hasResults" class="empty">
      {{ search ? `No addons match "${search}".` : 'No addons to show — try unchecking "Hide disabled".' }}
    </div>

    <template v-else>
      <section v-if="groups.mandatory.length" class="group">
        <h2>Mandatory ({{ groups.mandatory.length }})</h2>
        <div class="list">
          <AddonRow v-for="a in groups.mandatory" :key="a.name" :addon="a" />
        </div>
      </section>

      <section v-if="groups.recommended.length" class="group">
        <h2>Recommended ({{ groups.recommended.length }})</h2>
        <div class="list">
          <AddonRow v-for="a in groups.recommended" :key="a.name" :addon="a" />
        </div>
      </section>

      <section v-if="groups.optional.length" class="group">
        <h2>Optional ({{ groups.optional.length }})</h2>
        <div class="list">
          <AddonRow v-for="a in groups.optional" :key="a.name" :addon="a" />
        </div>
      </section>

      <section v-if="customAddons.length" class="group">
        <h2>Custom Addons ({{ customAddons.length }})</h2>
        <p class="custom-hint">Installed outside the guild launcher — not synced or managed here.</p>
        <div class="list">
          <div v-for="c in customAddons" :key="c.files[0]" class="custom-row" :class="{ dim: c.disabled }">
            <div class="custom-main">
              <div class="name">
                {{ c.name }}<span class="version">v{{ c.version }}</span>
                <span v-if="c.disabled" class="badge off">Off</span>
              </div>
              <div v-if="c.notes" class="notes">{{ c.notes }}</div>
              <div v-if="c.dependencies.length" class="requires">Requires: {{ c.dependencies.join(', ') }}</div>
              <div class="folder">{{ c.files[0] }}</div>
            </div>
            <div class="custom-actions">
              <button class="action-btn" @click="onToggleCustom(c.name, c.disabled)">
                {{ c.disabled ? 'Enable' : 'Disable' }}
              </button>
              <button class="delete-btn" title="Delete completely" @click="onDeleteCustom(c.name)">✕</button>
            </div>
          </div>
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

.header-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
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

.filters {
  display: flex;
  align-items: center;
  gap: 14px;
  margin-bottom: 20px;
}

.search-input {
  flex: 1;
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 8px 12px;
  color: var(--text);
  font-size: 13px;
}

.search-input:focus {
  outline: none;
  border-color: var(--accent-dim);
}

.hide-disabled {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-dim);
  white-space: nowrap;
  cursor: pointer;
}

.hide-disabled input[type='checkbox'] {
  accent-color: var(--accent);
}

.refresh-btn {
  background: var(--bg-panel);
  border: 1px solid var(--border);
  color: var(--text);
  border-radius: var(--radius);
  padding: 7px 14px;
  font-size: 12px;
  font-weight: 500;
  flex-shrink: 0;
}

.refresh-btn:hover:not(:disabled) {
  border-color: var(--accent-dim);
}

.refresh-btn:disabled {
  opacity: 0.6;
  cursor: default;
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

.custom-hint {
  font-size: 11px;
  color: var(--text-dim);
  margin: 0 0 8px;
}

.custom-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 10px 14px;
}

.custom-row.dim {
  opacity: 0.6;
}

.custom-main {
  min-width: 0;
}

.custom-main .name {
  font-size: 13px;
  font-weight: 500;
}

.custom-main .badge {
  font-size: 10px;
  padding: 2px 7px;
  border-radius: 10px;
  background: var(--bg-elevated);
  color: var(--text-dim);
  margin-left: 8px;
}

.custom-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.action-btn {
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  color: var(--text);
  border-radius: var(--radius);
  padding: 6px 12px;
  font-size: 11px;
  font-weight: 500;
}

.action-btn:hover {
  border-color: var(--accent-dim);
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
  flex-shrink: 0;
}

.delete-btn:hover {
  border-color: var(--danger);
  color: var(--danger);
}

.custom-main .version {
  font-size: 11px;
  color: var(--text-dim);
  font-weight: 400;
  margin-left: 6px;
}

.custom-main .notes {
  font-size: 11px;
  color: var(--text-dim);
  margin-top: 3px;
  line-height: 1.4;
}

.custom-main .requires {
  font-size: 10px;
  color: var(--accent-strong);
  margin-top: 3px;
}

.custom-main .folder {
  font-size: 10px;
  color: var(--text-dim);
  opacity: 0.7;
  margin-top: 4px;
  font-family: monospace;
}
</style>
