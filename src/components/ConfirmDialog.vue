<script setup lang="ts">
import { answerConfirm, confirmState } from '../store'
</script>

<template>
  <div v-if="confirmState.request" class="overlay" @click.self="answerConfirm(false)">
    <div class="dialog" :class="{ danger: confirmState.request.danger }">
      <h3>{{ confirmState.request.title }}</h3>
      <p class="message">{{ confirmState.request.message }}</p>
      <div class="actions">
        <button class="btn cancel" @click="answerConfirm(false)">Cancel</button>
        <button class="btn confirm" @click="answerConfirm(true)">{{ confirmState.request.confirmLabel }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.dialog {
  width: 380px;
  max-width: calc(100% - 48px);
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 20px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.4);
}

.dialog h3 {
  margin: 0 0 10px;
  font-size: 15px;
  color: var(--text);
}

.dialog.danger h3 {
  color: var(--danger);
}

.message {
  margin: 0 0 18px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-dim);
  white-space: pre-line;
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.btn {
  border-radius: var(--radius);
  padding: 8px 16px;
  font-size: 12px;
  font-weight: 600;
  border: 1px solid transparent;
}

.cancel {
  background: var(--bg-elevated);
  border-color: var(--border);
  color: var(--text);
}

.cancel:hover {
  border-color: var(--accent-dim);
}

.confirm {
  background: var(--accent);
  color: #062028;
}

.dialog.danger .confirm {
  background: var(--danger);
  color: #260a0a;
}
</style>
