<script setup lang="ts">
import { computed } from 'vue'
import { store } from '../store'
import AddonRow from '../components/AddonRow.vue'

const groups = computed(() => {
  const addons = store.manifest?.addons ?? []
  return {
    mandatory: addons.filter((a) => a.category === 'mandatory'),
    recommended: addons.filter((a) => a.category === 'recommended'),
    optional: addons.filter((a) => a.category === 'optional'),
  }
})
</script>

<template>
  <div class="addons">
    <div class="header">
      <h1>Addons</h1>
      <p>Mandatory addons always sync. Uncheck anything else to leave it out — unchecking something already installed removes it from your AddOns folder on the next sync.</p>
    </div>

    <div v-if="store.loadingManifest" class="empty">Loading manifest...</div>
    <div v-else-if="store.manifestError" class="empty error">
      Couldn't reach the guild server: {{ store.manifestError }}
    </div>
    <div v-else-if="!store.manifest" class="empty">No manifest loaded.</div>

    <template v-else>
      <section v-if="groups.mandatory.length" class="group">
        <h2>Mandatory</h2>
        <div class="list">
          <AddonRow v-for="a in groups.mandatory" :key="a.name" :addon="a" />
        </div>
      </section>

      <section v-if="groups.recommended.length" class="group">
        <h2>Recommended</h2>
        <div class="list">
          <AddonRow v-for="a in groups.recommended" :key="a.name" :addon="a" />
        </div>
      </section>

      <section v-if="groups.optional.length" class="group">
        <h2>Optional</h2>
        <div class="list">
          <AddonRow v-for="a in groups.optional" :key="a.name" :addon="a" />
        </div>
      </section>
    </template>
  </div>
</template>

<style scoped>
.addons {
  padding: 24px 28px;
  max-width: 760px;
}

.header h1 {
  font-size: 20px;
  margin: 0 0 4px;
}

.header p {
  font-size: 12px;
  color: var(--text-dim);
  margin: 0 0 24px;
}

.empty {
  color: var(--text-dim);
  font-size: 13px;
}

.empty.error {
  color: var(--danger);
}

.group {
  margin-bottom: 22px;
}

.group h2 {
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--text-dim);
  margin: 0 0 8px;
}

.list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
</style>
