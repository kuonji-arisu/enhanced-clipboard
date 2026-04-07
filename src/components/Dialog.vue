<script setup lang="ts">
withDefaults(defineProps<{
  show: boolean
  title: string
  message?: string
  okLabel?: string
  cancelLabel?: string
  okVariant?: 'default' | 'danger'
}>(), {
  okVariant: 'default',
})

const emit = defineEmits<{
  'update:show': [value: boolean]
  ok: []
  cancel: []
}>()

function handleOk() {
  emit('ok')
}

function handleCancel() {
  emit('cancel')
  emit('update:show', false)
}
</script>

<template>
  <Teleport to="body">
    <div v-if="show" class="dialog-mask" @click.self="handleCancel">
      <div class="dialog-panel">
        <p class="dialog-title">{{ title }}</p>
        <p v-if="message" class="dialog-msg">{{ message }}</p>
        <div class="dialog-actions">
          <slot>
            <button v-if="cancelLabel" class="dialog-btn dialog-btn--cancel" @click="handleCancel">
              {{ cancelLabel }}
            </button>
            <button
              v-if="okLabel"
              class="dialog-btn"
              :class="okVariant === 'danger' ? 'dialog-btn--danger' : 'dialog-btn--ok'"
              @click="handleOk"
            >
              {{ okLabel }}
            </button>
          </slot>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.dialog-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  backdrop-filter: blur(2px);
  -webkit-backdrop-filter: blur(2px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.dialog-panel {
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
  padding: var(--space-5);
  width: 272px;
  box-shadow: var(--shadow-lg);
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.dialog-title {
  margin: 0;
  font-size: var(--font-size-base);
  font-weight: var(--font-weight-semibold);
  color: var(--color-text-primary);
}

.dialog-msg {
  margin: 0;
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  line-height: 1.5;
}

.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  margin-top: var(--space-1);
}

.dialog-btn {
  padding: 6px var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--font-size-sm);
  cursor: pointer;
  transition: background 0.15s;
}

.dialog-btn--cancel {
  border: 1px solid var(--color-border);
  background: transparent;
  color: var(--color-text-secondary);
}

.dialog-btn--cancel:hover {
  background: var(--color-bg-hover);
}

.dialog-btn--ok {
  border: none;
  background: var(--color-accent);
  color: var(--color-text-on-accent);
  font-weight: var(--font-weight-medium);
}

.dialog-btn--ok:hover {
  background: var(--color-accent-hover);
}

.dialog-btn--danger {
  border: none;
  background: var(--color-danger);
  color: var(--color-text-on-accent);
  font-weight: var(--font-weight-medium);
}

.dialog-btn--danger:hover {
  background: var(--color-danger-hover);
}
</style>
