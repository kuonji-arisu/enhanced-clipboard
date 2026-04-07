<script setup lang="ts">
import { ref, computed, onUnmounted } from 'vue'
import { useI18n } from '../i18n'
import { useSettingsStore } from '../stores/settings'

const props = defineProps<{ modelValue: string }>()
const emit = defineEmits<{ 'update:modelValue': [v: string] }>()

const { t } = useI18n()
const store = useSettingsStore()
const recording = ref(false)
const errorMessage = ref('')

const KEY_MAP: Record<string, string> = {
  ' ': 'Space',
  ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right',
  Backspace: 'Backspace', Delete: 'Delete', Tab: 'Tab',
  Enter: 'Return', Escape: 'Escape', Home: 'Home', End: 'End',
  PageUp: 'PageUp', PageDown: 'PageDown',
  F1: 'F1', F2: 'F2', F3: 'F3', F4: 'F4', F5: 'F5', F6: 'F6',
  F7: 'F7', F8: 'F8', F9: 'F9', F10: 'F10', F11: 'F11', F12: 'F12',
}

async function startRecording() {
  if (recording.value) return
  errorMessage.value = ''
  try {
    await store.pauseHotkey()
  } catch (error) {
    console.error('[hotkey] pause failed', error)
    errorMessage.value = t('hotkeyPauseFailed')
    return
  }
  recording.value = true
  document.addEventListener('keydown', captureKey, true)
}

async function stopRecording() {
  if (!recording.value) return
  recording.value = false
  document.removeEventListener('keydown', captureKey, true)
  try {
    await store.resumeHotkey()
  } catch (error) {
    console.error('[hotkey] resume failed', error)
    errorMessage.value = t('hotkeyResumeFailed')
  }
}

async function captureKey(e: KeyboardEvent) {
  e.preventDefault()
  e.stopPropagation()
  if (e.key === 'Escape') { await stopRecording(); return }
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return

  const parts: string[] = []
  // Ctrl 和 Meta 互斥：macOS Command 映射为 CmdOrCtrl，Windows Meta(Win 键) 映射为 Super
  // 两者同时按下时优先取 Ctrl(CmdOrCtrl)，避免 parts 中出现重复
  if (e.ctrlKey) {
    parts.push('CmdOrCtrl')
  } else if (e.metaKey) {
    parts.push(navigator.platform.startsWith('Win') ? 'Super' : 'CmdOrCtrl')
  }
  if (e.altKey) parts.push('Alt')
  if (e.shiftKey) parts.push('Shift')
  const keyName = KEY_MAP[e.key] ?? (e.key.length === 1 ? e.key.toUpperCase() : e.key)
  parts.push(keyName)
  emit('update:modelValue', parts.join('+'))
  await stopRecording()
}

onUnmounted(() => {
  if (recording.value) void stopRecording()
})

// "CmdOrCtrl+Shift+V" 解析为 ["Ctrl", "Shift", "V"]
const badges = computed(() =>
  props.modelValue
    ? props.modelValue.split('+').map((k) => (k === 'CmdOrCtrl' ? 'Ctrl' : k))
    : []
)
</script>

<template>
  <div
    class="hotkey-input-wrap"
  >
    <div
      class="hotkey-input"
    :class="{ 'hotkey-input--recording': recording }"
    tabindex="0"
      @click="startRecording"
      @blur.capture="stopRecording"
    >
      <template v-if="recording">
        <span class="hotkey-hint">{{ t('hotkeyRecording') }}</span>
      </template>
      <template v-else-if="badges.length">
        <kbd v-for="badge in badges" :key="badge" class="hotkey-badge">{{ badge }}</kbd>
      </template>
      <template v-else>
        <span class="hotkey-hint">—</span>
      </template>
    </div>
    <p v-if="errorMessage" class="hotkey-error">{{ errorMessage }}</p>
  </div>
</template>

<style scoped>
.hotkey-input-wrap {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.hotkey-input {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-wrap: wrap;
  min-height: 34px;
  padding: 6px var(--space-3);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  cursor: pointer;
  outline: none;
  transition: border-color 0.15s, box-shadow 0.15s;
}

.hotkey-input:hover {
  border-color: var(--color-border-strong);
}

.hotkey-input:focus,
.hotkey-input--recording {
  border-color: var(--color-accent);
  box-shadow: 0 0 0 3px var(--color-accent-subtle);
}

.hotkey-badge {
  display: inline-flex;
  align-items: center;
  padding: 1px 6px;
  font-size: var(--font-size-xs);
  font-family: inherit;
  background: var(--color-bg-hover);
  border: 1px solid var(--color-border-strong);
  border-radius: var(--radius-sm);
  color: var(--color-text-primary);
  line-height: 1.4;
}

.hotkey-hint {
  font-size: var(--font-size-sm);
  color: var(--color-text-tertiary);
}

.hotkey-error {
  margin: 0;
  font-size: var(--font-size-xs);
  color: var(--color-danger);
}
</style>
