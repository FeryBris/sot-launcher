<script setup lang="ts">
import { store } from '../store';
import heroImg from '../assets/hero.png';
</script>

<template>
  <div class="home">
    <div class="banner" :style="{ backgroundImage: `url(${heroImg})` }">
      <div class="banner-overlay">
        <h1>Welcome back{{ store.settings.gamePath ? '' : ', adventurer' }}</h1>
        <p>Keep your addons in sync and jump straight into raid.</p>
      </div>
    </div>

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
