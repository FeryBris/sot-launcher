<script setup lang="ts">
import { computed } from 'vue';
import { PhArrowSquareOut } from '@phosphor-icons/vue';
import { currentGamePath, store } from '../store';
import { openCharacterPage } from '../api';
import heroImg from '../assets/hero.png';
import type { GuildSyncData } from '../types';

// Same filenames the guild website uses for portraits (gender-race-class.webp,
// snake_case, e.g. "male-night_elf-death_knight.webp") — copied locally so
// the launcher doesn't need a network round-trip just to show them.
const portraitModules = import.meta.glob('../assets/portraits/*.webp', {
  eager: true,
  import: 'default',
}) as Record<string, string>;
const portraitsByFile: Record<string, string> = {};
for (const path in portraitModules) {
  const file = path.split('/').pop()!;
  portraitsByFile[file] = portraitModules[path];
}

// Fallback when a specific gender/race/class combo has no portrait (not
// every race/class pairing exists in WotLK) — the generic per-class icon,
// same one ClassPortrait.vue uses on the website.
const classIconModules = import.meta.glob('../assets/classes/*.webp', {
  eager: true,
  import: 'default',
}) as Record<string, string>;
const classIconsByFile: Record<string, string> = {};
for (const path in classIconModules) {
  const file = path.split('/').pop()!;
  classIconsByFile[file] = classIconModules[path];
}

// The addon reports WoW's raw, non-localized API tokens (e.g. class
// "WARLOCK", race "NightElf", "Scourge" for Undead) — map those to the
// portrait filenames' snake_case pieces, same normalization the website's
// CharacterSyncService does server-side for display.
const CLASS_KEY: Record<string, string> = {
  WARRIOR: 'warrior',
  PALADIN: 'paladin',
  HUNTER: 'hunter',
  ROGUE: 'rogue',
  PRIEST: 'priest',
  DEATHKNIGHT: 'death_knight',
  SHAMAN: 'shaman',
  MAGE: 'mage',
  WARLOCK: 'warlock',
  DRUID: 'druid',
};
const RACE_KEY: Record<string, string> = {
  Human: 'human',
  Dwarf: 'dwarf',
  NightElf: 'night_elf',
  Gnome: 'gnome',
  Draenei: 'draenei',
  Orc: 'orc',
  Scourge: 'undead',
  Tauren: 'tauren',
  Troll: 'troll',
  BloodElf: 'blood_elf',
};

function portraitSrc(character: GuildSyncData): string {
  const gender = (character.gender ?? 'Male').toLowerCase();
  const race = RACE_KEY[character.race] ?? character.race.toLowerCase();
  const cls = CLASS_KEY[character.class.toUpperCase()] ?? character.class.toLowerCase();
  const file = `${gender}-${race}-${cls}.webp`;
  const classIconFile = `${cls.replace(/_/g, '')}.webp`;
  return portraitsByFile[file] ?? classIconsByFile[classIconFile] ?? portraitsByFile['blank.webp'];
}

function classIconSrc(rawClass: string): string {
  const cls = CLASS_KEY[rawClass.toUpperCase()] ?? rawClass.toLowerCase();
  const classIconFile = `${cls.replace(/_/g, '')}.webp`;
  return classIconsByFile[classIconFile] ?? portraitsByFile['blank.webp'];
}

// Standard WoW class colors — same convention as the website's own class
// badges (see App/Character/Index.vue's ".Badge--<Class>" backgrounds).
const CLASS_COLOR: Record<string, string> = {
  WARRIOR: '#C79C6E',
  PALADIN: '#F58CBA',
  HUNTER: '#ABD473',
  ROGUE: '#FFF569',
  PRIEST: '#FFFFFF',
  DEATHKNIGHT: '#C41F3B',
  SHAMAN: '#0070DE',
  MAGE: '#69CCF0',
  WARLOCK: '#9482C9',
  DRUID: '#FF7D0A',
};
function classColor(rawClass: string): string {
  return CLASS_COLOR[rawClass.toUpperCase()] ?? '#c9cdd6';
}

// Highest level first; within the same level, higher guild rank first
// (lower guildRankIndex = higher rank, matching WoW's own 0-based
// convention where 0 is Guild Master). Characters with no rank yet known
// sort after ones that do.
const sortedCharacters = computed<GuildSyncData[]>(() => {
  return [...store.guildSyncData].sort((a, b) => {
    if (a.level !== b.level) return b.level - a.level;
    const rankA = a.guildRankIndex ?? Infinity;
    const rankB = b.guildRankIndex ?? Infinity;
    return rankA - rankB;
  });
});

const syncStatusLabel = computed(() => {
  if (store.guildSyncing) return 'Syncing…';
  if (store.guildLastSyncedAt) {
    const time = new Date(store.guildLastSyncedAt).toLocaleTimeString([], {
      hour: '2-digit',
      minute: '2-digit',
    });
    return `Synced at ${time}`;
  }
  return null;
});

function viewCharacterOnWeb(character: GuildSyncData) {
  void openCharacterPage(store.apiBaseUrl, character.name);
}
</script>

<template>
  <div class="home">
    <div class="banner" :style="{ backgroundImage: `url(${heroImg})` }">
      <div class="banner-overlay">
        <h1>Welcome back{{ currentGamePath() ? '' : ', adventurer' }}</h1>
        <p>Keep your addons in sync and jump straight into raid.</p>
      </div>
    </div>

    <section class="character">
      <div class="character-header">
        <h2>My Characters</h2>
        <span v-if="syncStatusLabel" class="sync-status">{{ syncStatusLabel }}</span>
      </div>

      <div v-if="!store.guildSyncData.length" class="empty">
        No character data yet — log in as a Stalkers of Time member with the SoTGuildSync addon
        installed.
      </div>

      <template v-else>
        <div class="character-grid">
          <div
            v-for="character in sortedCharacters"
            :key="character.name"
            class="character-card"
            :style="{ '--class-color': classColor(character.class) }"
          >
            <img class="card-portrait" :src="portraitSrc(character)" alt="" />

            <div class="card-level" title="Level">{{ character.level }}</div>
            <img class="card-class-icon" :src="classIconSrc(character.class)" alt="" />

            <div class="card-footer">
              <span class="card-name">{{ character.name }}</span>
              <span v-if="character.guildRank" class="card-rank">{{ character.guildRank }}</span>
            </div>

            <button
              class="card-web-link"
              title="View on website"
              @click="viewCharacterOnWeb(character)"
            >
              <PhArrowSquareOut :size="16" weight="duotone" />
            </button>
          </div>
        </div>
        <p v-if="!store.settings.discordToken" class="hint">
          Sign in with Discord (sidebar) to sync these to the website.
        </p>
        <p v-if="store.guildSyncError" class="hint error">{{ store.guildSyncError }}</p>
      </template>
    </section>

    <section class="news">
      <h2>Guild News</h2>

      <div v-if="store.loadingManifest" class="empty">Loading...</div>
      <div v-else-if="store.manifestError" class="empty error">
        Couldn't reach the guild server: {{ store.manifestError }}
      </div>
      <div v-else-if="!store.manifest?.news.length" class="empty">No news yet.</div>

      <article v-for="item in store.manifest?.news" :key="item.title + item.date" class="news-item">
        <div class="news-header">
          <h3>{{ item.title }}</h3>
          <span class="news-date">{{ item.date }}</span>
        </div>
        <p>{{ item.body }}</p>
      </article>
    </section>
  </div>
</template>

<style scoped>
.home {
  padding: 24px 28px;
  max-width: 760px;
}

.banner {
  height: 160px;
  border-radius: 12px;
  background-size: cover;
  background-position: center;
  overflow: hidden;
  position: relative;
  margin-bottom: 28px;
  border: 1px solid var(--border);
}

.banner-overlay {
  position: absolute;
  inset: 0;
  background: linear-gradient(0deg, rgba(14, 17, 22, 0.92), rgba(14, 17, 22, 0.35));
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  padding: 16px 20px;
}

.banner-overlay h1 {
  margin: 0;
  font-size: 20px;
}

.banner-overlay p {
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--text-dim);
}

.character {
  margin-bottom: 28px;
}

.character h2 {
  font-size: 14px;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--text-dim);
  margin: 0;
}

.character-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
}

.character-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 14px;
}

.character-card {
  position: relative;
  width: 140px;
  height: 186px;
  border-radius: 10px;
  border: 2px solid var(--class-color);
  overflow: hidden;
  background: var(--bg-panel);
  box-shadow:
    0 0 0 1px rgba(0, 0, 0, 0.35),
    inset 0 0 24px 4px rgba(0, 0, 0, 0.7);
}

.card-portrait {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.card-level {
  position: absolute;
  top: 6px;
  right: 6px;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: rgba(14, 17, 22, 0.85);
  border: 2px solid var(--class-color);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
  font-weight: 700;
  color: var(--text);
}

.card-class-icon {
  position: absolute;
  top: 6px;
  left: 6px;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  border: 2px solid var(--class-color);
  object-fit: cover;
  background: rgba(14, 17, 22, 0.85);
}

.card-footer {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  /* Extra right padding keeps long names/ranks from running under the
     web-link button sitting in the bottom-right corner. */
  padding: 20px 32px 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  background: linear-gradient(0deg, rgba(10, 12, 16, 0.95) 15%, rgba(10, 12, 16, 0));
}

.card-name {
  font-size: 13px;
  font-weight: 700;
  color: var(--class-color);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.card-rank {
  font-size: 11px;
  color: var(--text-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.card-web-link {
  position: absolute;
  right: 6px;
  bottom: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: rgba(14, 17, 22, 0.85);
  color: var(--text-dim);
}

.card-web-link:hover {
  color: var(--accent-strong);
  background: rgba(14, 17, 22, 1);
}

.sync-status {
  font-size: 11px;
  color: var(--text-dim);
  flex-shrink: 0;
}

.hint {
  margin: 8px 0 0;
  font-size: 11px;
  color: var(--text-dim);
}

.hint.error {
  color: var(--danger);
}

.news h2 {
  font-size: 14px;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--text-dim);
  margin: 0 0 12px;
}

.empty {
  color: var(--text-dim);
  font-size: 13px;
  padding: 16px 0;
}

.empty.error {
  color: var(--danger);
}

.news-item {
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 14px 16px;
  margin-bottom: 10px;
}

.news-header {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 12px;
}

.news-header h3 {
  margin: 0;
  font-size: 14px;
}

.news-date {
  font-size: 11px;
  color: var(--text-dim);
  flex-shrink: 0;
}

.news-item p {
  margin: 6px 0 0;
  font-size: 12px;
  color: var(--text-dim);
  line-height: 1.5;
}
</style>
