<script setup lang="ts">
import { computed } from 'vue'
import { refreshManifest, store } from '../store'
import AddonRow from '../components/AddonRow.vue'

const groups = computed(() => {
  const addons = store.manifest?.addons ?? []
  return {
    mandatory: addons.filter((a) => a.category === 'mandatory'),
    recommended: addons.filter((a) => a.category === 'recommended'),
    optional: addons.filter((a) => a.category === 'optional'),
  }
})

const customAddons = computed(() =>
  Object.values(store.installed?.addons ?? {})
    .filter((a) => a.type === 'custom')
    .sort((a, b) => a.name.localeCompare(b.name)),
)
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
      <p>Mandatory addons always sync. Uncheck anything else to leave it out — unchecking something already installed removes it from your AddOns folder on the next sync.</p>
    </div>

    <div v-if="store.loadingManifest" class="empty">Loading manifest...</div>
    <div v-else-if="store.manifestError" class="empty error">
      Couldn't reach the guild server: {{ store.manifestError }}
    </div>
    <div v-else-if="!store.manifest" class="empty">No manifest loaded.</div>

    <template v-else>
      <section v-if="groups.mandatory.length" class="group">
        <h2>Mandatory</h2>
        <div class="list">
          <AddonRow v-for="a in groups.mandatory" :key="a.name" :addon="a" />
        </div>
      </section>

      <section v-if="groups.recommended.length" class="group">
        <h2>Recommended</h2>
        <div class="list">
          <AddonRow v-for="a in groups.recommended" :key="a.name" :addon="a" />
        </div>
      </section>

      <section v-if="groups.optional.length" class="group">
        <h2>Optional</h2>
        <div class="list">
          <AddonRow v-for="a in groups.optional" :key="a.name" :addon="a" />
        </div>
      </section>

      <section v-if="customAddons.length" class="group">
        <h2>Custom Addons</h2>
        <p class="custom-hint">Installed outside the guild launcher — not synced or managed here.</p>
        <div class="list">
          <div v-for="c in customAddons" :key="c.files[0]" class="custom-row">
            <div class="custom-main">
              <div class="name">{{ c.name }}<span class="version">v{{ c.version }}</span></div>
              <div v-if="c.notes" class="notes">{{ c.notes }}</div>
              <div class="folder">{{ c.files[0] }}</div>
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
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 10px 14px;
}

.custom-main .name {
  font-size: 13px;
  font-weight: 500;
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

.custom-main .folder {
  font-size: 10px;
  color: var(--text-dim);
  opacity: 0.7;
  margin-top: 4px;
  font-family: monospace;
}
</style>
