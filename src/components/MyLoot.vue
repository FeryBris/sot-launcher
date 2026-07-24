<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { store } from '../store'
import type { MyLootRaid } from '../types'

const QUALITY_COLOR: Record<string, string> = {
  Poor: '#9d9d9d',
  Common: '#ffffff',
  Uncommon: '#1eff00',
  Rare: '#0070dd',
  Epic: '#a335ee',
  Legendary: '#ff8000',
}
function qualityColor(quality: string | null): string {
  return (quality && QUALITY_COLOR[quality]) || '#c9cdd6'
}

const characters = computed(() => store.myLoot?.characters ?? [])

const activeCharacterIndex = ref(0)
const activeRaidIndex = ref(0)

// Whenever the character tab changes, jump back to that character's most
// recent raid (raids already arrive sorted newest-first).
watch(activeCharacterIndex, () => {
  activeRaidIndex.value = 0
})

const activeCharacter = computed(() => characters.value[activeCharacterIndex.value] ?? null)
const activeRaid = computed<MyLootRaid | null>(
  () => activeCharacter.value?.raids[activeRaidIndex.value] ?? null,
)
</script>

<template>
  <div class="my-loot">
    <div v-if="store.myLootLoading && !store.myLoot" class="empty">Loading…</div>
    <div v-else-if="store.myLootError" class="empty error">{{ store.myLootError }}</div>
    <div v-else-if="!characters.length" class="empty">No loot recorded yet.</div>

    <template v-else>
      <div class="char-tabs">
        <button
          v-for="(character, index) in characters"
          :key="character.name"
          class="char-tab"
          :class="{ active: index === activeCharacterIndex }"
          @click="activeCharacterIndex = index"
        >
          {{ character.name }}
        </button>
      </div>

      <div class="loot-body">
        <ul class="raid-list">
          <li v-for="(raid, index) in activeCharacter?.raids ?? []" :key="raid.id ?? index">
            <button
              class="raid-item"
              :class="{ active: index === activeRaidIndex }"
              @click="activeRaidIndex = index"
            >
              <span class="raid-name">{{ raid.name }}</span>
              <span v-if="raid.startedAt?.stringDate" class="raid-date">{{ raid.startedAt.stringDate }}</span>
            </button>
          </li>
        </ul>

        <div class="loot-list">
          <div v-if="!activeRaid || !activeRaid.loots.length" class="empty">No loot in this raid.</div>
          <div v-for="loot in activeRaid?.loots ?? []" :key="loot.id" class="loot-row">
            <img
              v-if="loot.item.media?.full_path"
              class="loot-icon"
              :src="loot.item.media.full_path"
              alt=""
              :style="{ borderColor: qualityColor(loot.item.quality) }"
            />
            <div class="loot-info">
              <span class="loot-name" :style="{ color: qualityColor(loot.item.quality) }">
                {{ loot.item.name }}
              </span>
              <span class="loot-sub">
                <template v-if="loot.item.level">ilvl {{ loot.item.level }}</template>
                <template v-if="loot.response?.name"> · {{ loot.response.name }}</template>
                <template v-if="loot.givenAt?.stringDate"> · {{ loot.givenAt.stringDate }}</template>
              </span>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.my-loot {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.char-tabs {
  display: flex;
  gap: 4px;
  border-bottom: 1px solid var(--border);
}

.char-tab {
  background: transparent;
  border: none;
  border-bottom: 2px solid transparent;
  color: var(--text-dim);
  padding: 8px 14px;
  font-size: 13px;
  font-weight: 600;
}

.char-tab:hover {
  color: var(--text);
}

.char-tab.active {
  color: var(--accent-strong);
  border-bottom-color: var(--accent-strong);
}

.loot-body {
  display: flex;
  gap: 14px;
  min-height: 0;
}

.raid-list {
  list-style: none;
  margin: 0;
  padding: 0;
  width: 200px;
  flex-shrink: 0;
  max-height: 320px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.raid-item {
  width: 100%;
  text-align: left;
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.raid-item.active {
  border-color: var(--accent-strong);
  background: var(--bg-elevated);
}

.raid-name {
  font-size: 12px;
  font-weight: 600;
}

.raid-date {
  font-size: 11px;
  color: var(--text-dim);
}

.loot-list {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.loot-row {
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 8px 12px;
  display: flex;
  align-items: center;
  gap: 10px;
}

.loot-icon {
  width: 32px;
  height: 32px;
  border-radius: 6px;
  border: 1px solid var(--border);
  object-fit: cover;
  flex-shrink: 0;
}

.loot-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.loot-name {
  font-size: 13px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.loot-sub {
  font-size: 11px;
  color: var(--text-dim);
}

.empty {
  color: var(--text-dim);
  font-size: 13px;
  padding: 16px 0;
}

.empty.error {
  color: var(--danger);
}
</style>
