<script setup lang="ts">
import { ref } from 'vue'
import { chooseClient, GAME_NAMES, store } from '../store'
import type { GameClient } from '../types'
// Each game's own icon, taken from its Wow.exe.
import wotlkIcon from '../assets/game_wotlk.png'
import foreverIcon from '../assets/game_forever.png'

const GAMES: { client: GameClient; icon: string }[] = [
  { client: 'wotlk', icon: wotlkIcon },
  { client: 'forever', icon: foreverIcon },
]

const choosing = ref<GameClient | null>(null)
const error = ref('')

async function choose(client: GameClient) {
  if (choosing.value) return
  choosing.value = client
  error.value = ''
  try {
    await chooseClient(client)
  } catch (e) {
    error.value = String(e)
  } finally {
    choosing.value = null
  }
}
</script>

<template>
  <div class="picker" role="dialog" aria-modal="true" aria-labelledby="picker-title">
    <h2 id="picker-title">Which game do you play the most?</h2>
    <p class="hint">
      The launcher will open with this game. Play both? No problem: switch between them any time
      with the WotLK | Forever switch in the sidebar, and change your choice whenever you like.
    </p>
    <div class="games">
      <button
        v-for="game in GAMES"
        :key="game.client"
        class="game"
        :disabled="choosing !== null || store.syncing"
        @click="choose(game.client)"
      >
        <img class="icon" :src="game.icon" alt="" />
        <span class="name">{{ GAME_NAMES[game.client] }}</span>
      </button>
    </div>
    <p v-if="error" class="error">{{ error }}</p>
  </div>
</template>

<style scoped>
.picker {
  position: fixed;
  inset: 0;
  z-index: 900; /* under ConfirmDialog (1000) */
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 24px;
  background: rgba(0, 0, 0, 0.88);
}

h2 {
  margin: 0;
  font-size: 20px;
  color: var(--text);
}

.hint {
  max-width: 440px;
  margin: 0 0 20px;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-dim);
  text-align: center;
}

.games {
  display: flex;
  gap: 24px;
}

.game {
  width: 200px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 14px;
  padding: 28px 16px 22px;
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: 12px;
  color: var(--text);
  transition:
    border-color 0.12s,
    transform 0.12s,
    box-shadow 0.12s;
}

.game:hover:not(:disabled),
.game:focus-visible {
  border-color: var(--highlight);
  box-shadow: 0 0 0 1px var(--highlight), 0 8px 24px rgba(0, 0, 0, 0.5);
  transform: translateY(-2px);
  outline: none;
}

.game:disabled {
  cursor: default;
  opacity: 0.6;
}

.icon {
  width: 96px;
  height: 96px;
  object-fit: contain;
}

.name {
  font-size: 18px;
  font-weight: 700;
  letter-spacing: 0.03em;
}

.error {
  margin: 12px 0 0;
  font-size: 12px;
  color: var(--danger);
}
</style>
