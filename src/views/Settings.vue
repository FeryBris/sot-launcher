<script setup lang="ts">
import { ref, watch } from 'vue'
import { store, saveApiBaseUrl, saveGamePath } from '../store'
import * as api from '../api'

const apiUrlDraft = ref(store.settings.apiBaseUrl)
const apiUrlSaved = ref(false)

watch(
  () => store.settings.apiBaseUrl,
  (v) => {
    apiUrlDraft.value = v
  },
)

async function browseGameFolder() {
  const path = await api.pickGameFolder()
  if (path) {
    await saveGamePath(path)
  }
}

async function saveApiUrl() {
  await saveApiBaseUrl(apiUrlDraft.value.trim())
  apiUrlSaved.value = true
  setTimeout(() => (apiUrlSaved.value = false), 1500)
}
</script>

<template>
  <div class="settings">
    <h1>Settings</h1>

    <section class="field">
      <label>World of Warcraft folder</label>
      <div class="row">
        <input class="path" type="text" readonly :value="store.settings.gamePath ?? 'Not set'" />
        <button class="btn" @click="browseGameFolder">Browse...</button>
      </div>
      <p class="hint">Addons install into <code>Interface/AddOns</code> under this folder.</p>
    </section>

    <section class="field">
      <label>Guild server URL</label>
      <div class="row">
        <input class="path" type="text" v-model="apiUrlDraft" placeholder="https://launcher-api.example.com/api" />
        <button class="btn" @click="saveApiUrl">{{ apiUrlSaved ? 'Saved' : 'Save' }}</button>
      </div>
      <p class="hint">Where the launcher fetches the addon manifest and guild news.</p>
    </section>
  </div>
</template>

<style scoped>
.settings {
  padding: 24px 28px;
  max-width: 640px;
}

h1 {
  font-size: 20px;
  margin: 0 0 24px;
}

.field {
  margin-bottom: 22px;
}

label {
  display: block;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-dim);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  margin-bottom: 8px;
}

.row {
  display: flex;
  gap: 8px;
}

.path {
  flex: 1;
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 9px 12px;
  color: var(--text);
  font-size: 13px;
  min-width: 0;
}

.path[readonly] {
  color: var(--text-dim);
}

.btn {
  background: var(--bg-panel);
  border: 1px solid var(--border);
  color: var(--text);
  border-radius: var(--radius);
  padding: 9px 16px;
  font-size: 13px;
  font-weight: 500;
  flex-shrink: 0;
}

.btn:hover {
  border-color: var(--accent-dim);
}

.hint {
  margin: 8px 0 0;
  font-size: 11px;
  color: var(--text-dim);
}

.hint code {
  background: var(--bg-panel);
  padding: 1px 5px;
  border-radius: 4px;
}
</style>
