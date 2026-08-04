<script setup lang="ts">
import { onMounted, onUnmounted, ref, shallowRef, watch } from 'vue';
import { check as checkForUpdate, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { getVersion } from '@tauri-apps/api/app';
import HomeView from './views/Home.vue';
import AddonsView from './views/Addons.vue';
import LogsView from './views/Logs.vue';
import RaidsView from './views/Raids.vue';
import SettingsView from './views/Settings.vue';
import PlayBar from './components/PlayBar.vue';
import ConfirmDialog from './components/ConfirmDialog.vue';
import logo from './assets/logo.png';
import { init as initStore, playNow, signInWithDiscord, store } from './store';
import {
  isSelfUpdateSupported,
  logClientEvent,
  onTrayPrimaryClicked,
  openLauncherDownloadPage,
  setTrayPrimaryLabel,
} from './api';

type Tab = 'home' | 'addons' | 'logs' | 'raids' | 'settings';

const tab = ref<Tab>('home');
const appVersion = ref('');

// False for a Linux install that isn't an AppImage (.deb/.rpm/pacman) —
// downloadAndInstall() can't replace a binary under /usr/bin there, so
// those users get a link to the download page instead of an install button.
const selfUpdateSupported = ref(true);

const updateAvailable = shallowRef<Update | null>(null);
const updateState = ref<'idle' | 'checking' | 'downloading' | 'ready' | 'error'>('idle');
const updateError = ref('');
const updateProgress = ref(0);

// Without this, a launcher left open across a release only learns about it
// on next restart — most players just leave it running in the tray. 30min
// catches a new release within a session without hammering the update server.
const UPDATE_CHECK_INTERVAL_MS = 30 * 60 * 1000;
let updateCheckTimer: ReturnType<typeof setInterval> | undefined;

/** Best-effort breadcrumb into debug.json — logClientEvent's own Rust side
 *  already swallows its errors, but the IPC call itself could still reject
 *  (e.g. mid-teardown), and a logging hiccup must never break the update
 *  flow it's trying to observe. */
function logUpdateEvent(level: 'error' | 'info', message: string) {
  logClientEvent(level, 'launcher_update', message).catch(() => {});
}

async function checkUpdate() {
  updateState.value = 'checking';
  try {
    const update = await checkForUpdate();
    if (update?.available) {
      updateAvailable.value = update;
      updateState.value = 'idle';
    } else {
      updateState.value = 'idle';
    }
  } catch (e) {
    console.error('[updater] check failed:', e);
    logUpdateEvent('error', `check failed: ${e}`);
    updateState.value = 'error';
    updateError.value = String(e);
  }
}

async function installUpdate() {
  if (!updateAvailable.value) return;
  if (!selfUpdateSupported.value) {
    void openLauncherDownloadPage(store.apiBaseUrl);
    return;
  }
  updateState.value = 'downloading';
  let downloaded = 0;
  let total = 0;
  try {
    await updateAvailable.value.downloadAndInstall((event) => {
      if (event.event === 'Started') {
        total = event.data.contentLength ?? 0;
      } else if (event.event === 'Progress') {
        downloaded += event.data.chunkLength;
        updateProgress.value = total > 0 ? Math.round((downloaded / total) * 100) : 0;
      } else if (event.event === 'Finished') {
        updateProgress.value = 100;
      }
    });
    updateState.value = 'ready';
    // Windows' update handoff exits this process so the installer can
    // replace its files, whether or not that installer is actually allowed
    // to run under strict AppLocker/WDAC policies — if this is the last
    // entry debug.json ever gets, that silence is itself the diagnosis.
    await logClientEvent(
      'info',
      'launcher_update',
      'install downloaded, relaunching to hand off to installer'
    ).catch(() => {});
    await relaunch();
  } catch (e) {
    console.error('[updater] install failed:', e);
    logUpdateEvent('error', `install failed: ${e}`);
    updateState.value = 'error';
    updateError.value = String(e);
  }
}

// Mirrors update-availability onto the native tray menu's first item —
// the tray itself has no idea whether an update exists, it just relays
// whatever this side decides.
watch(
  updateAvailable,
  (val) => {
    void setTrayPrimaryLabel(val ? 'Update' : 'Play');
  },
  { immediate: true }
);

// The Raids tab (and its nav button) only render for officers — if that
// stops being true mid-session (e.g. sign-out), fall back off a tab that
// no longer has anything to show instead of leaving the content area blank.
watch(
  () => store.settings.isOfficer,
  (isOfficer) => {
    if (!isOfficer && tab.value === 'raids') {
      tab.value = 'home';
    }
  }
);

onMounted(async () => {
  appVersion.value = await getVersion();
  await initStore();
  selfUpdateSupported.value = await isSelfUpdateSupported();
  checkUpdate();

  await onTrayPrimaryClicked(() => {
    if (updateAvailable.value) {
      installUpdate();
    } else {
      playNow();
    }
  });

  if (store.settings.autoLaunch) {
    playNow();
  }

  updateCheckTimer = setInterval(() => {
    // Don't let a periodic recheck interrupt a download already in flight
    // or one that's finished and waiting to relaunch.
    if (updateState.value === 'downloading' || updateState.value === 'ready') return;
    checkUpdate();
  }, UPDATE_CHECK_INTERVAL_MS);
});

onUnmounted(() => {
  clearInterval(updateCheckTimer);
});
</script>

<template>
  <div class="shell">
    <aside class="sidebar">
      <div class="brand">
        <div class="brand-row">
          <img class="brand-mark" :src="logo" alt="" />
          <span class="brand-name">Launcher</span>
        </div>
        <div class="version">v{{ appVersion }}</div>
      </div>

      <nav class="nav">
        <button class="nav-item" :class="{ active: tab === 'home' }" @click="tab = 'home'">
          Home
        </button>
        <button class="nav-item" :class="{ active: tab === 'addons' }" @click="tab = 'addons'">
          Addons
        </button>
        <button class="nav-item" :class="{ active: tab === 'logs' }" @click="tab = 'logs'">
          Logs
        </button>
        <button
          v-if="store.settings.isOfficer"
          class="nav-item"
          :class="{ active: tab === 'raids' }"
          @click="tab = 'raids'"
        >
          Raids
        </button>
        <button class="nav-item" :class="{ active: tab === 'settings' }" @click="tab = 'settings'">
          Settings
        </button>
      </nav>

      <div class="sidebar-footer">
        <div v-if="updateAvailable" class="update-banner">
          <div class="update-text">Update {{ updateAvailable.version }} available</div>
          <button
            class="update-btn"
            :disabled="updateState === 'downloading'"
            @click="installUpdate"
          >
            <template v-if="!selfUpdateSupported">Download update</template>
            <template v-else>{{
              updateState === 'downloading' ? `Updating ${updateProgress}%` : 'Update & Restart'
            }}</template>
          </button>
          <div v-if="!selfUpdateSupported" class="update-notice">
            This install can't auto-update — opens sot.sk/sot-launcher to download it manually.
          </div>
          <div
            v-if="selfUpdateSupported && updateState === 'error'"
            class="update-install-error"
            :title="updateError"
          >
            Update failed — try again
          </div>
        </div>
        <div v-else-if="updateState === 'error'" class="update-error" :title="updateError">
          Update check failed
        </div>
        <div v-if="store.manifestError" class="manifest-error" :title="store.manifestError">
          Guild server error — check Settings
        </div>
        <div v-if="store.settings.discordSignedIn" class="discord-status">
          <img
            v-if="store.settings.discordAvatar"
            class="discord-avatar"
            :src="store.settings.discordAvatar"
            alt=""
          />
          <span class="discord-name">{{ store.settings.discordUsername }}</span>
        </div>
        <button v-else class="discord-signin-btn" @click="signInWithDiscord">Sign in</button>
        <div v-if="store.discordError" class="discord-error" :title="store.discordError">
          Sign-in failed — try again
        </div>
      </div>
    </aside>

    <main class="content">
      <HomeView v-show="tab === 'home'" />
      <AddonsView v-show="tab === 'addons'" />
      <LogsView v-show="tab === 'logs'" />
      <RaidsView v-if="store.settings.isOfficer" v-show="tab === 'raids'" />
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
  padding: 20px 16px;
  border-bottom: 1px solid var(--border);
}

.brand-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.brand-mark {
  width: 30px;
  height: 30px;
  border-radius: 6px;
  object-fit: cover;
  flex-shrink: 0;
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
  transition:
    background 0.12s,
    color 0.12s;
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

.update-notice {
  font-size: 11px;
  color: var(--text-dim);
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

.discord-error {
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
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
}

.discord-avatar {
  width: 22px;
  height: 22px;
  border-radius: 50%;
  flex-shrink: 0;
}

.discord-name {
  font-size: 11px;
  color: var(--accent-strong);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.discord-signin-btn {
  background: var(--accent);
  color: #062028;
  border: none;
  border-radius: 6px;
  padding: 6px 8px;
  font-size: 11px;
  font-weight: 600;
}

.version {
  font-size: 11px;
  color: var(--text-dim);
  margin-top: 4px;
}

.content {
  flex: 1;
  overflow-y: auto;
  min-width: 0;
}
</style>
