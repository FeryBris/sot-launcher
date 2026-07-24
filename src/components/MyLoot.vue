<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { store } from '../store'
import type { MyLootInstance } from '../types'
import LoadingSpinner from './LoadingSpinner.vue'

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
const activeInstanceIndex = ref(0)

// Whenever the character tab changes, jump back to that character's most
// recently-looted instance (instances already arrive sorted newest-first).
watch(activeCharacterIndex, () => {
  activeInstanceIndex.value = 0
})

const activeCharacter = computed(() => characters.value[activeCharacterIndex.value] ?? null)
const activeInstance = computed<MyLootInstance | null>(
  () => activeCharacter.value?.instances[activeInstanceIndex.value] ?? null,
)
</script>

<template>
  <div class="my-loot">
    <div v-if="store.myLootLoading && !store.myLoot" class="empty">
      <LoadingSpinner label="Loading…" />
    </div>
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
        <ul class="instance-list">
          <li v-for="(instance, index) in activeCharacter?.instances ?? []" :key="instance.id ?? index">
            <button
              class="instance-item"
              :class="{ active: index === activeInstanceIndex }"
              @click="activeInstanceIndex = index"
            >
              <span class="instance-name">{{ instance.name }}</span>
              <span v-if="instance.lastLootedAt?.stringDate" class="instance-date">
                {{ instance.lastLootedAt.stringDate }}
              </span>
            </button>
          </li>
        </ul>

        <div class="loot-list">
          <div v-if="!activeInstance || !activeInstance.loots.length" class="empty">
            No loot from this instance.
          </div>
          <div v-for="loot in activeInstance?.loots ?? []" :key="loot.id" class="loot-row">
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
              </span>
            </div>
            <div v-if="loot.count > 1" class="loot-count" title="Times looted">×{{ loot.count }}</div>
            <div v-if="loot.givenAt?.day" class="loot-date" :title="loot.givenAt.stringDate">
              <span class="loot-date-day">{{ loot.givenAt.day }}.{{ loot.givenAt.month }}.</span>
              <span class="loot-date-year">{{ loot.givenAt.year }}</span>
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

.instance-list {
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

.instance-item {
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

.instance-item.active {
  border-color: var(--accent-strong);
  background: var(--bg-elevated);
}

.instance-name {
  font-size: 12px;
  font-weight: 600;
}

.instance-date {
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
  flex: 1;
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

.loot-count {
  flex-shrink: 0;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 4px 8px;
  font-size: 11px;
  font-weight: 700;
  color: var(--accent-strong);
}

.loot-date {
  flex-shrink: 0;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 4px 8px;
  display: flex;
  flex-direction: column;
  align-items: center;
  line-height: 1.3;
}

.loot-date-day {
  font-size: 11px;
  font-weight: 600;
  color: var(--text);
}

.loot-date-year {
  font-size: 10px;
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
