<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  store,
  saveAutoLaunch,
  saveCloseToTray,
  saveHideOnLaunch,
  saveGamePath,
  saveLaunchCommand,
  saveWtfSyncAutoPull,
  saveWtfSyncAutoPush,
  signInWithDiscord,
  signOutDiscord,
} from '../store'
import { isWindows } from '../platform'
import * as api from '../api'
import type { LutrisGame } from '../types'
import SyncLogsDialog from '../components/SyncLogsDialog.vue'
import WtfSyncDialog from '../components/WtfSyncDialog.vue'

const showSyncLogs = ref(false)
const showWtfSync = ref(false)

const launchCmdDraft = ref(store.settings.launchCommand ?? '')
const launchCmdSaved = ref(false)

const platformTab = ref<'windows' | 'linux'>(isWindows() ? 'windows' : 'linux')

/** The folder path for whichever tab is currently selected — not
 *  necessarily the OS actually running this build, since both tabs stay
 *  browsable/editable for reference either way. */
const currentTabGamePath = computed(() =>
  platformTab.value === 'windows' ? store.settings.gamePathWindows : store.settings.gamePathLinux,
)

const lutrisGames = ref<LutrisGame[]>([])
const lutrisLoading = ref(false)
const lutrisError = ref('')

watch(
  () => store.settings.launchCommand,
  (v) => {
    launchCmdDraft.value = v ?? ''
  },
)

async function browseGameFolder() {
  const path = await api.pickGameFolder()
  if (path) {
    await saveGamePath(path, platformTab.value)
  }
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

async function onHideOnLaunchChange(e: Event) {
  await saveHideOnLaunch((e.target as HTMLInputElement).checked)
}

async function onCloseToTrayChange(e: Event) {
  await saveCloseToTray((e.target as HTMLInputElement).checked)
}

async function onAutoLaunchChange(e: Event) {
  await saveAutoLaunch((e.target as HTMLInputElement).checked)
}

async function onWtfAutoPullChange(e: Event) {
  await saveWtfSyncAutoPull((e.target as HTMLInputElement).checked)
}

async function onWtfAutoPushChange(e: Event) {
  await saveWtfSyncAutoPush((e.target as HTMLInputElement).checked)
}
</script>

<template>
  <div class="settings">
    <h1>Settings</h1>

    <section class="field">
      <label>World of Warcraft folder</label>
      <p class="hint">Location to your WoW game folder — the one containing <code>Wow.exe</code>.</p>

      <div class="platform-tabs">
        <button
          class="platform-tab"
          :class="{ active: platformTab === 'windows' }"
          @click="platformTab = 'windows'"
        >
          Windows
        </button>
        <button
          class="platform-tab"
          :class="{ active: platformTab === 'linux' }"
          @click="platformTab = 'linux'"
        >
          Linux
        </button>
      </div>

      <div class="platform-panel">
        <div class="row">
          <input class="path" type="text" readonly :value="currentTabGamePath ?? 'Not set'" />
          <button class="btn" @click="browseGameFolder">Browse...</button>
        </div>
        <p v-if="platformTab === 'windows'" class="hint">
          Usually <code>C:\Program Files (x86)\World of Warcraft</code>.
        </p>
        <p v-else class="hint">
          The folder inside your Wine prefix/Lutris install, e.g.
          <code>~/Games/world-of-warcraft/drive_c/Program Files (x86)/World of Warcraft</code>.
        </p>
        <p class="hint">Addons install into <code>Interface/AddOns</code> under this folder.</p>

        <template v-if="platformTab === 'linux'">
          <div class="panel-divider"></div>
          <label class="panel-sublabel">Custom Launch Command (optional)</label>
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
            Leave empty to auto-detect and launch <code>Wow.exe</code> directly (via Wine). If you
            play through Lutris, click "Detect Lutris games" below to pick yours instead of typing a
            command by hand. Bottles/CrossOver/custom Wine prefixes still need to be entered manually
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
        </template>
      </div>
    </section>

    <section class="field">
      <label>Behavior</label>
      <label class="checkbox-row">
        <input
          type="checkbox"
          :checked="store.settings.hideOnLaunch"
          @change="onHideOnLaunchChange"
        />
        Hide the launcher in the system tray once the game starts
      </label>
      <label class="checkbox-row">
        <input
          type="checkbox"
          :checked="store.settings.closeToTray"
          @change="onCloseToTrayChange"
        />
        Hide to the system tray when closing the launcher window
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

    <section v-if="store.settings.discordToken" class="field">
      <label>WTF Folder Sync</label>
      <p class="hint">
        Backs up account/character settings and every addon's saved data (keybinds, macros, chat
        and UI layout, ElvUI/WeakAuras/DBM profiles, ...) to the cloud — a WoW install can have
        several accounts, each synced independently.
      </p>

      <div class="row">
        <button class="btn" @click="showWtfSync = true">Manage WTF Sync</button>
      </div>

      <label class="checkbox-row">
        <input
          type="checkbox"
          :checked="store.settings.wtfSyncAutoPull"
          @change="onWtfAutoPullChange"
        />
        Automatically pull the latest settings when the launcher opens
      </label>
      <label class="checkbox-row">
        <input
          type="checkbox"
          :checked="store.settings.wtfSyncAutoPush"
          @change="onWtfAutoPushChange"
        />
        Automatically push my changes after the game closes
      </label>
      <p class="hint">
        Applies to every account already set up on this device. Both still ask for confirmation if
        the other side changed since this device's last sync for that account — nothing is ever
        overwritten silently. World of Warcraft must be closed to sync either way.
      </p>
    </section>

    <section class="field">
      <label>Sync</label>
      <div class="row">
        <button class="btn" @click="showSyncLogs = true">View Sync Logs</button>
      </div>
      <p class="hint">
        Your character and guild roster data sync to the website automatically in the background.
        See the log for a history of those attempts, including any that failed and why.
      </p>
    </section>

    <SyncLogsDialog v-if="showSyncLogs" @close="showSyncLogs = false" />
    <WtfSyncDialog v-if="showWtfSync" @close="showWtfSync = false" />
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

.platform-tabs {
  display: flex;
  gap: 2px;
  margin-top: 10px;
}

.platform-tab {
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-bottom: none;
  color: var(--text-dim);
  border-radius: 6px 6px 0 0;
  padding: 6px 14px;
  font-size: 12px;
  font-weight: 500;
  position: relative;
  top: 1px;
}

.platform-tab:hover {
  color: var(--text);
}

.platform-tab.active {
  background: color-mix(in srgb, var(--accent) 14%, var(--bg-panel));
  border-color: var(--accent-dim);
  color: var(--accent-strong);
}

/* Sits flush under the tabs, sharing a subtle accent tint with whichever
 * tab is active so the two visually read as one connected piece. */
.platform-panel {
  background: color-mix(in srgb, var(--accent) 6%, transparent);
  border: 1px solid var(--accent-dim);
  border-radius: 0 8px 8px 8px;
  padding: 14px;
}

.platform-panel .row:first-child {
  margin-top: 0;
}

.panel-divider {
  border-top: 1px solid var(--accent-dim);
  margin: 14px 0;
}

.panel-sublabel {
  display: block;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-dim);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  margin-bottom: 8px;
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
