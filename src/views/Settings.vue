<script setup lang="ts">
import { ref, watch } from 'vue'
import {
  store,
  saveApiBaseUrl,
  saveApiKey,
  saveAutoLaunch,
  saveCloseOnLaunch,
  saveGamePath,
  saveLaunchCommand,
  signInWithDiscord,
  signOutDiscord,
} from '../store'
import * as api from '../api'
import type { LutrisGame } from '../types'

const apiUrlDraft = ref(store.settings.apiBaseUrl)
const apiUrlSaved = ref(false)
const apiKeyDraft = ref(store.settings.apiKey ?? '')
const apiKeySaved = ref(false)
const launchCmdDraft = ref(store.settings.launchCommand ?? '')
const launchCmdSaved = ref(false)

const lutrisGames = ref<LutrisGame[]>([])
const lutrisLoading = ref(false)
const lutrisError = ref('')

watch(
  () => store.settings.apiBaseUrl,
  (v) => {
    apiUrlDraft.value = v
  },
)

watch(
  () => store.settings.apiKey,
  (v) => {
    apiKeyDraft.value = v ?? ''
  },
)

watch(
  () => store.settings.launchCommand,
  (v) => {
    launchCmdDraft.value = v ?? ''
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

async function saveApiKeyValue() {
  await saveApiKey(apiKeyDraft.value.trim())
  apiKeySaved.value = true
  setTimeout(() => (apiKeySaved.value = false), 1500)
}

async function saveLaunchCmd() {
  await saveLaunchCommand(launchCmdDraft.value.trim())
  launchCmdSaved.value = true
  setTimeout(() => (launchCmdSaved.value = false), 1500)
}

async function detectLutrisGames() {
  lutrisLoading.value = true
  lutrisError.value = ''
  lutrisGames.value = []
  try {
    lutrisGames.value = await api.listLutrisGames()
    if (lutrisGames.value.length === 0) {
      lutrisError.value = 'Lutris is installed, but its library is empty.'
    }
  } catch (e) {
    lutrisError.value = String(e)
  } finally {
    lutrisLoading.value = false
  }
}

function pickLutrisGame(game: LutrisGame) {
  launchCmdDraft.value = game.launchCommand
  lutrisGames.value = []
}

async function onCloseOnLaunchChange(e: Event) {
  await saveCloseOnLaunch((e.target as HTMLInputElement).checked)
}

async function onAutoLaunchChange(e: Event) {
  await saveAutoLaunch((e.target as HTMLInputElement).checked)
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
      <label>Custom Launch Command (optional)</label>
      <div class="row">
        <input
          class="path"
          type="text"
          v-model="launchCmdDraft"
          placeholder='e.g. lutris lutris:rungame/world-of-warcraft'
        />
        <button class="btn" @click="saveLaunchCmd">{{ launchCmdSaved ? 'Saved' : 'Save' }}</button>
      </div>
      <p class="hint">
        Leave empty to auto-detect and launch <code>Wow.exe</code> directly (via Wine on Linux/macOS).
        If you play through Lutris, click "Detect Lutris games" below to pick yours instead of typing
        a command by hand. Bottles/CrossOver/custom Wine prefixes still need to be entered manually
        &mdash; <code>{game_path}</code> is replaced with the folder above.
      </p>

      <div class="row lutris-row">
        <button class="btn" :disabled="lutrisLoading" @click="detectLutrisGames">
          {{ lutrisLoading ? 'Looking for Lutris…' : 'Detect Lutris games' }}
        </button>
      </div>
      <p v-if="lutrisError" class="hint error">{{ lutrisError }}</p>
      <ul v-if="lutrisGames.length" class="lutris-list">
        <li v-for="game in lutrisGames" :key="game.id">
          <button class="lutris-game" @click="pickLutrisGame(game)">
            {{ game.name }}
            <span class="lutris-runner">{{ game.runner }}</span>
          </button>
        </li>
      </ul>
    </section>

    <section class="field">
      <label>Behavior</label>
      <label class="checkbox-row">
        <input
          type="checkbox"
          :checked="store.settings.closeOnLaunch"
          @change="onCloseOnLaunchChange"
        />
        Close the launcher once the game starts
      </label>
      <label class="checkbox-row">
        <input
          type="checkbox"
          :checked="store.settings.autoLaunch"
          @change="onAutoLaunchChange"
        />
        Automatically sync and launch when the launcher opens
      </label>
    </section>

    <section class="field">
      <label>Guild server URL</label>
      <div class="row">
        <input class="path" type="text" v-model="apiUrlDraft" placeholder="https://launcher-api.example.com/api" />
        <button class="btn" @click="saveApiUrl">{{ apiUrlSaved ? 'Saved' : 'Save' }}</button>
      </div>
      <p class="hint">Where the launcher fetches the addon manifest and guild news.</p>
    </section>

    <section class="field">
      <label>Guild API Key</label>
      <div class="row">
        <input
          class="path"
          type="password"
          v-model="apiKeyDraft"
          placeholder="Ask an officer for the guild launcher key"
        />
        <button class="btn" @click="saveApiKeyValue">{{ apiKeySaved ? 'Saved' : 'Save' }}</button>
      </div>
      <p class="hint">Required to fetch the manifest and launcher updates. Sent as the X-API-KEY header.</p>
    </section>

    <section class="field">
      <label>Discord Account</label>
      <div v-if="store.settings.discordToken" class="row discord-row">
        <img v-if="store.settings.discordAvatar" class="avatar" :src="store.settings.discordAvatar" alt="" />
        <span class="discord-name">Connected as {{ store.settings.discordUsername }}</span>
        <button class="btn" @click="signOutDiscord">Disconnect</button>
      </div>
      <div v-else class="row">
        <button class="btn accent" @click="signInWithDiscord">Sign in with Discord</button>
      </div>
      <p v-if="store.discordError" class="hint error">{{ store.discordError }}</p>
      <p class="hint">Links your Discord identity so your WTF folder can sync across devices.</p>
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

.hint.error {
  color: var(--danger);
}

.lutris-row {
  margin-top: 10px;
}

.lutris-list {
  list-style: none;
  margin: 8px 0 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 200px;
  overflow-y: auto;
}

.lutris-game {
  width: 100%;
  text-align: left;
  background: var(--bg-panel);
  border: 1px solid var(--border);
  color: var(--text);
  border-radius: var(--radius);
  padding: 8px 12px;
  font-size: 13px;
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.lutris-game:hover {
  border-color: var(--accent-dim);
}

.lutris-runner {
  font-size: 11px;
  color: var(--text-dim);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.btn.accent {
  background: var(--accent);
  color: #062028;
  border-color: var(--accent);
}

.btn.accent:hover {
  border-color: var(--accent-strong);
}

.discord-row {
  align-items: center;
}

.avatar {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  flex-shrink: 0;
}

.discord-name {
  flex: 1;
  font-size: 13px;
  color: var(--text);
}

.checkbox-row {
  display: flex;
  align-items: center;
  gap: 8px;
  text-transform: none;
  letter-spacing: normal;
  font-weight: 400;
  font-size: 13px;
  color: var(--text);
  margin-bottom: 10px;
  cursor: pointer;
}

.checkbox-row input[type='checkbox'] {
  width: 15px;
  height: 15px;
  accent-color: var(--accent);
  flex-shrink: 0;
}
</style>
