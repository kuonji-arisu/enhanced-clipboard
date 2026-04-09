<script setup lang="ts">
import { computed, ref, watch, onUnmounted } from 'vue'
import TitleBar from '../components/TitleBar.vue'
import HotkeyInput from '../components/HotkeyInput.vue'
import Icon from '../components/Icon.vue'
import Dialog from '../components/Dialog.vue'
import { useAppInfoStore } from '../stores/appInfo'
import { useSettingsStore } from '../stores/settings'
import { useI18n } from '../i18n'
import { useRouter } from 'vue-router'
import type { SaveSettingsResult } from '../types'

const appInfoStore = useAppInfoStore()
const store = useSettingsStore()
const router = useRouter()
const { t } = useI18n()
const LOG_LEVEL_LABELS = {
  silent: 'logLevelSilent',
  error: 'logLevelError',
  warning: 'logLevelWarning',
  info: 'logLevelInfo',
  debug: 'logLevelDebug',
} as const

const historyLimits = computed(() => {
  const { min_history_limit, max_history_limit } = appInfoStore.requireAppInfo()
  return { min: min_history_limit, max: max_history_limit }
})

function formatExpiryOption(seconds: number) {
  switch (seconds) {
    case 0:
      return t('expiryOff')
    case 10 * 60:
      return t('durationMinutes', { count: 10 })
    case 30 * 60:
      return t('durationMinutes', { count: 30 })
    case 60 * 60:
      return t('durationHours', { count: 1 })
    case 24 * 60 * 60:
      return t('durationDays', { count: 1 })
    case 7 * 24 * 60 * 60:
      return t('durationWeeks', { count: 1 })
    default:
      return `${seconds}`
  }
}

const expiryOptions = computed(() =>
  appInfoStore.requireAppInfo().expiry_presets.map((seconds) => ({
    seconds,
    label: formatExpiryOption(seconds),
  })),
)

const logLevelOptions = computed(() =>
  appInfoStore.requireAppInfo().log_level_options.map((level) => ({
    value: level,
    label: LOG_LEVEL_LABELS[level],
  })),
)

watch(
  () => store.draftSettings.theme_mode,
  (themeMode) => {
    store.setThemePreview(themeMode)
  },
  { immediate: true },
)

onUnmounted(() => {
  store.clearThemePreview()
  store.resetDraft()
})

const maxHistoryError = computed(() =>
  store.draftSettings.max_history < historyLimits.value.min ||
  store.draftSettings.max_history > historyLimits.value.max
    ? `${t('min')} ${historyLimits.value.min} ~ ${t('max')} ${historyLimits.value.max}`
    : ''
)
const canSave = computed(() => store.isDirty && !maxHistoryError.value)
const showSaveConfirm = ref(false)
const showSaveError = ref(false)
const saveErrorMsg = ref('')
const saveWarnings = ref<string[]>([])

function expiryRank(seconds: number) {
  return seconds === 0 ? Number.POSITIVE_INFINITY : seconds
}

const destructiveChangeLabels = computed(() => {
  const labels: string[] = []
  if (store.draftSettings.max_history < store.savedSettings.max_history) {
    labels.push(t('maxHistory'))
  }
  if (expiryRank(store.draftSettings.expiry_seconds) < expiryRank(store.savedSettings.expiry_seconds)) {
    labels.push(t('autoExpiry'))
  }
  if (store.savedSettings.capture_images && !store.draftSettings.capture_images) {
    labels.push(t('captureImages'))
  }
  return labels
})

const destructiveConfirmMessage = computed(() => {
  if (destructiveChangeLabels.value.length === 0) return ''
  return t('settingsDeleteWarnMessage', {
    list: destructiveChangeLabels.value.join(t('listSeparator')),
  })
})

function collectEffectWarnings(result: SaveSettingsResult): string[] {
  return Object.values(result.effects)
    .filter((effect) => effect && !effect.ok && effect.error)
    .map((effect) => effect!.error as string)
}

async function persistSave() {
  showSaveConfirm.value = false
  try {
    const result = await store.save()
    saveWarnings.value = collectEffectWarnings(result)
  } catch (e) {
    saveWarnings.value = []
    saveErrorMsg.value = String(e)
    showSaveError.value = true
  }
}

async function handleSave() {
  if (destructiveChangeLabels.value.length > 0) {
    showSaveConfirm.value = true
    return
  }
  await persistSave()
}
</script>

<template>
  <div class="settings">
    <TitleBar :title="t('settingsTitle')">
      <template #extra-buttons>
        <button @click="router.push('/')" class="titlebar-btn">
          <Icon name="back" :size="14" />
        </button>
      </template>
    </TitleBar>

    <div class="settings-body">
      <div class="settings-form">
        <div v-if="saveWarnings.length" class="save-warning">
          <div class="field-label">{{ t('settingsApplyWarningTitle') }}</div>
          <p v-for="warning in saveWarnings" :key="warning" class="save-warning__item">
            {{ warning }}
          </p>
        </div>

        <div class="field">
          <label class="field-label">{{ t('globalHotkey') }}</label>
          <HotkeyInput v-model="store.draftSettings.hotkey" />
          <p class="field-hint">{{ t('hotkeyHint') }}</p>
        </div>

        <div class="field-row">
          <div>
            <div class="field-label">{{ t('appearance') }}</div>
            <p class="field-hint">{{ t('appearanceHint') }}</p>
          </div>
          <div class="theme-toggle">
            <button
              :class="['theme-option', { 'theme-option--active': store.draftSettings.theme_mode === 'light' }]"
              @click="store.draftSettings.theme_mode = 'light'"
            >
              <Icon name="sun" :size="13" />
              {{ t('light') }}
            </button>
            <button
              :class="['theme-option', { 'theme-option--active': store.draftSettings.theme_mode === 'dark' }]"
              @click="store.draftSettings.theme_mode = 'dark'"
            >
              <Icon name="moon" :size="13" />
              {{ t('dark') }}
            </button>
            <button
              :class="['theme-option', { 'theme-option--active': store.draftSettings.theme_mode === 'system' }]"
              @click="store.draftSettings.theme_mode = 'system'"
            >
              {{ t('system') }}
            </button>
          </div>
        </div>

        <div class="field-row">
          <div>
            <div class="field-label">{{ t('autostart') }}</div>
            <p class="field-hint">{{ t('autostartHint') }}</p>
          </div>
          <button
            class="toggle-switch"
            :class="{ 'toggle-switch--on': store.draftSettings.autostart }"
            @click="store.draftSettings.autostart = !store.draftSettings.autostart"
          >
            <span class="toggle-thumb" />
          </button>
        </div>

        <div class="field">
          <label class="field-label">{{ t('maxHistory') }}</label>
          <input
            v-model.number="store.draftSettings.max_history"
            type="number"
            :min="historyLimits.min"
            :max="historyLimits.max"
            :class="['field-input', { 'field-input--error': maxHistoryError }]"
          />
          <p v-if="maxHistoryError" class="field-error">{{ maxHistoryError }}</p>
          <p v-else class="field-hint">
            {{ t('min') }} {{ historyLimits.min }} ~ {{ t('max') }} {{ historyLimits.max }}
          </p>
        </div>

        <div class="field">
          <label class="field-label">{{ t('autoExpiry') }}</label>
          <select v-model.number="store.draftSettings.expiry_seconds" class="field-select">
            <option
              v-for="opt in expiryOptions"
              :key="opt.seconds"
              :value="opt.seconds"
            >{{ opt.label }}</option>
          </select>
          <p class="field-hint">{{ t('autoExpiryHint') }}</p>
        </div>

        <div class="field-row">
          <div>
            <div class="field-label">{{ t('captureImages') }}</div>
            <p class="field-hint">{{ t('captureImagesHint') }}</p>
          </div>
          <button
            class="toggle-switch"
            :class="{ 'toggle-switch--on': store.draftSettings.capture_images }"
            @click="store.draftSettings.capture_images = !store.draftSettings.capture_images"
          >
            <span class="toggle-thumb" />
          </button>
        </div>

        <div class="field">
          <label class="field-label">{{ t('logLevel') }}</label>
          <select v-model="store.draftSettings.log_level" class="field-select">
            <option
              v-for="level in logLevelOptions"
              :key="level.value"
              :value="level.value"
            >{{ t(level.label) }}</option>
          </select>
          <p class="field-hint">{{ t('logLevelHint') }}</p>
        </div>
      </div>
    </div>

    <div class="settings-footer">
      <button class="save-btn" :disabled="store.saving || !canSave" @click="handleSave">
        {{ store.saving ? t('saving') : store.saved ? t('savedOk') : t('save') }}
      </button>
      <a
        href="https://github.com/kuonji-arisu/enhanced-clipboard"
        target="_blank"
        rel="noopener noreferrer"
        class="github-link"
      >{{ t('github') }}</a>
    </div>

    <Dialog
      v-model:show="showSaveConfirm"
      :title="t('settingsDeleteWarnTitle')"
      :message="destructiveConfirmMessage"
      :ok-label="t('ok')"
      :cancel-label="t('cancel')"
      ok-variant="danger"
      @ok="persistSave"
    />
    <Dialog
      v-model:show="showSaveError"
      :title="t('saveErrorTitle')"
      :message="saveErrorMsg"
      :ok-label="t('ok')"
      @ok="showSaveError = false"
    />
  </div>
</template>

<style scoped>
.settings {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.settings-body {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-5);
  min-height: 0;
}

.settings-form {
  max-width: 420px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.field {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.field-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
}

.field-label {
  font-size: var(--font-size-sm);
  font-weight: var(--font-weight-medium);
  color: var(--color-text-primary);
}

.field-hint {
  margin: 0;
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}

.field-input {
  padding: 7px var(--space-3);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
  outline: none;
  transition: border-color 0.15s, box-shadow 0.15s;
}

.field-select {
  padding: 7px var(--space-3);
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
  outline: none;
  cursor: pointer;
  transition: border-color 0.15s, box-shadow 0.15s;
}

.field-select:focus {
  border-color: var(--color-accent);
  box-shadow: 0 0 0 3px var(--color-accent-subtle);
}

.field-input:focus {
  border-color: var(--color-accent);
  box-shadow: 0 0 0 3px var(--color-accent-subtle);
}

.field-input--error {
  border-color: var(--color-danger, #e05252);
}

.field-input--error:focus {
  border-color: var(--color-danger, #e05252);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-danger, #e05252) 20%, transparent);
}

.field-error {
  margin: 0;
  font-size: var(--font-size-xs);
  color: var(--color-danger, #e05252);
}

.save-warning {
  padding: var(--space-3);
  border: 1px solid color-mix(in srgb, var(--color-warning, #d88a21) 28%, var(--color-border));
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--color-warning, #d88a21) 10%, var(--color-bg));
}

.save-warning__item {
  margin: 4px 0 0;
  font-size: var(--font-size-xs);
  color: var(--color-text-secondary);
}

.theme-toggle {
  display: flex;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  overflow: hidden;
  flex-shrink: 0;
}

.theme-option {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 5px var(--space-3);
  font-size: var(--font-size-xs);
  border: none;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
}

.theme-option + .theme-option {
  border-left: 1px solid var(--color-border);
}

.theme-option:hover {
  background: var(--color-bg-hover);
  color: var(--color-text-primary);
}

.theme-option--active {
  background: var(--color-accent);
  color: var(--color-text-on-accent);
  font-weight: var(--font-weight-medium);
}

.theme-option--active:hover {
  background: var(--color-accent-hover);
  color: var(--color-text-on-accent);
}

.toggle-switch {
  position: relative;
  width: 44px;
  height: 24px;
  border-radius: 12px;
  border: none;
  background: var(--color-border-strong);
  cursor: pointer;
  flex-shrink: 0;
  transition: background 0.2s;
}

.toggle-switch--on {
  background: var(--color-accent);
}

.toggle-thumb {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 20px;
  height: 20px;
  border-radius: 50%;
  background: var(--color-text-on-accent);
  box-shadow: var(--shadow-sm);
  transition: transform 0.2s;
}

.toggle-switch--on .toggle-thumb {
  transform: translateX(20px);
}

.save-btn {
  padding: 8px var(--space-4);
  background: var(--color-accent);
  color: var(--color-text-on-accent);
  border: none;
  border-radius: var(--radius-md);
  font-size: var(--font-size-sm);
  font-weight: var(--font-weight-medium);
  cursor: pointer;
  transition: background 0.15s;
}

.save-btn:hover:not(:disabled) {
  background: var(--color-accent-hover);
}

.save-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.settings-footer {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px var(--space-5) 8px;
  border-top: 1px solid var(--color-border);
  background: var(--color-bg);
}

.github-link {
  display: block;
  text-align: center;
  padding: 2px 0 0;
  font-size: var(--font-size-base);
  color: var(--color-text-tertiary);
  text-decoration: none;
  transition: color 0.15s;
}

.github-link:hover {
  color: var(--color-accent);
}
</style>
