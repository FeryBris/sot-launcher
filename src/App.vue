<script setup lang="ts">
import { onMounted, ref, shallowRef, watch } from 'vue'
import { check as checkForUpdate, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { getVersion } from '@tauri-apps/api/app'
import HomeView from './views/Home.vue'
import AddonsView from './views/Addons.vue'
import SettingsView from './views/Settings.vue'
import PlayBar from './components/PlayBar.vue'
import ConfirmDialog from './components/ConfirmDialog.vue'
import { init as initStore, playNow, store } from './store'

function authHeaders(): Record<string, string> {
  return store.settings.apiKey ? { 'X-API-KEY': store.settings.apiKey } : {}
}

type Tab = 'home' | 'addons' | 'settings'

const tab = ref<Tab>('home')
const appVersion = ref('')

const updateAvailable = shallowRef<Update | null>(null)
const updateState = ref<'idle' | 'checking' | 'downloading' | 'ready' | 'error'>('idle')
const updateError = ref('')
const updateProgress = ref(0)

async function checkUpdate() {
  updateState.value = 'checking'
  try {
    const update = await checkForUpdate({ headers: authHeaders() })
    if (update?.available) {
      updateAvailable.value = update
      updateState.value = 'idle'
    } else {
      updateState.value = 'idle'
    }
  } catch (e) {
    console.error('[updater] check failed:', e)
    updateState.value = 'error'
    updateError.value = String(e)
  }
}

async function installUpdate() {
  if (!updateAvailable.value) return
  updateState.value = 'downloading'
  let downloaded = 0
  let total = 0
  try {
    await updateAvailable.value.downloadAndInstall((event) => {
      if (event.event === 'Started') {
        total = event.data.contentLength ?? 0
      } else if (event.event === 'Progress') {
        downloaded += event.data.chunkLength
        updateProgress.value = total > 0 ? Math.round((downloaded / total) * 100) : 0
      } else if (event.event === 'Finished') {
        updateProgress.value = 100
      }
    })
    updateState.value = 'ready'
    await relaunch()
  } catch (e) {
    console.error('[updater] install failed:', e)
    updateState.value = 'error'
    updateError.value = String(e)
  }
}

onMounted(async () => {
  appVersion.value = await getVersion()
  await initStore()
  checkUpdate()

  // Registered only after the initial load+check above has already run, so
  // it can't also fire for that same first settings load (store.settings
  // goes from its empty default to the real loaded value once here) — only
  // for genuine later changes (Settings save, Discord pairing, etc.),
  // instead of leaving a stale error/result sitting there until restart.
  watch(() => store.settings.apiKey, () => checkUpdate())

  if (store.settings.autoLaunch) {
    playNow()
  }
})
</script>

<template>
  <div class="shell">
    <aside class="sidebar">
      <div class="brand">
        <span class="brand-mark">SoT</span>
        <span class="brand-name">Launcher</span>
      </div>

      <nav class="nav">
        <button class="nav-item" :class="{ active: tab === 'home' }" @click="tab = 'home'">Home</button>
        <button class="nav-item" :class="{ active: tab === 'addons' }" @click="tab = 'addons'">Addons</button>
        <button class="nav-item" :class="{ active: tab === 'settings' }" @click="tab = 'settings'">Settings</button>
      </nav>

      <div class="sidebar-footer">
        <div v-if="updateAvailable" class="update-banner">
          <div class="update-text">
            Update {{ updateAvailable.version }} available
          </div>
          <button
            class="update-btn"
            :disabled="updateState === 'downloading'"
            @click="installUpdate"
          >
            {{ updateState === 'downloading' ? `Updating ${updateProgress}%` : 'Update & Restart' }}
          </button>
          <div v-if="updateState === 'error'" class="update-install-error" :title="updateError">
            Update failed — try again
          </div>
        </div>
        <div v-else-if="updateState === 'error'" class="update-error" :title="updateError">
          Update check failed
        </div>
        <div v-if="store.manifestError" class="manifest-error" :title="store.manifestError">
          Guild server error — check Settings
        </div>
        <div v-if="store.settings.discordUsername" class="discord-status">
          Signed in as {{ store.settings.discordUsername }}
        </div>
        <div class="version">v{{ appVersion }}</div>
      </div>
    </aside>

    <main class="content">
      <HomeView v-show="tab === 'home'" />
      <AddonsView v-show="tab === 'addons'" />
      <SettingsView v-show="tab === 'settings'" />
    </main>
  </div>
  <PlayBar />
  <ConfirmDialog />
</template>

<style scoped>
.shell {
  position: fixed;
  inset: 0;
  bottom: 64px;
  display: flex;
}

.sidebar {
  width: 200px;
  flex-shrink: 0;
  background: var(--bg-elevated);
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
}

.brand {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 20px 16px;
  border-bottom: 1px solid var(--border);
}

.brand-mark {
  width: 30px;
  height: 30px;
  border-radius: 6px;
  background: linear-gradient(135deg, var(--accent), var(--accent-dim));
  color: #062028;
  font-weight: 700;
  font-size: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.brand-name {
  font-weight: 600;
  font-size: 13px;
  letter-spacing: 0.02em;
}

.nav {
  display: flex;
  flex-direction: column;
  padding: 12px 8px;
  gap: 2px;
}

.nav-item {
  text-align: left;
  background: transparent;
  border: none;
  color: var(--text-dim);
  padding: 9px 12px;
  border-radius: var(--radius);
  font-size: 13px;
  font-weight: 500;
  transition: background 0.12s, color 0.12s;
}

.nav-item:hover {
  background: var(--bg-panel);
  color: var(--text);
}

.nav-item.active {
  background: var(--bg-panel);
  color: var(--accent-strong);
}

.sidebar-footer {
  margin-top: auto;
  padding: 12px;
  border-top: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.update-banner {
  background: var(--bg-panel);
  border: 1px solid var(--accent-dim);
  border-radius: var(--radius);
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.update-text {
  font-size: 11px;
  color: var(--accent-strong);
}

.update-btn {
  background: var(--accent);
  color: #062028;
  border: none;
  border-radius: 6px;
  padding: 6px 8px;
  font-size: 11px;
  font-weight: 600;
}

.update-btn:disabled {
  opacity: 0.7;
  cursor: default;
}

.update-error {
  font-size: 11px;
  color: var(--text-dim);
}

.update-install-error {
  font-size: 11px;
  font-weight: 600;
  color: var(--danger);
}

.manifest-error {
  background: var(--bg-panel);
  border: 1px solid var(--danger);
  border-radius: var(--radius);
  padding: 8px;
  font-size: 11px;
  font-weight: 600;
  color: var(--danger);
  text-align: center;
}

.discord-status {
  font-size: 11px;
  color: var(--accent-strong);
  text-align: center;
}

.version {
  font-size: 11px;
  color: var(--text-dim);
  text-align: center;
}

.content {
  flex: 1;
  overflow-y: auto;
  min-width: 0;
}
</style>
