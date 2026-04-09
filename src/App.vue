<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import Dialog from './components/Dialog.vue'
import { useRuntimeNotice } from './hooks/useRuntimeNotice'
import { useI18n } from './i18n'
import { useAppInfoStore } from './stores/appInfo'
import { useNoticeStore } from './stores/notice'
import { usePersistedStateStore } from './stores/persistedState'
import { useRuntimeStore } from './stores/runtime'
import { useSettingsStore } from './stores/settings'
import { getErrorMessage } from './utils/errors'

const appInfoStore = useAppInfoStore()
const persistedStateStore = usePersistedStateStore()
const runtimeStore = useRuntimeStore()
const settingsStore = useSettingsStore()
const noticeStore = useNoticeStore()
const { t } = useI18n()
const bootstrapped = ref(false)

useRuntimeNotice()

onMounted(async () => {
  try {
    await appInfoStore.load()
    await Promise.all([settingsStore.load(), persistedStateStore.load(), runtimeStore.start()])
  } catch (e) {
    noticeStore.openError(t('actionErrorTitle'), getErrorMessage(e, t('appInitFailed')))
  } finally {
    bootstrapped.value = true
  }
})

onUnmounted(() => {
  runtimeStore.stop()
})
</script>

<template>
  <div class="app-shell">
    <router-view v-if="bootstrapped" />
    <div v-else class="app-loading" />

    <div v-if="!runtimeStore.runtime.clipboard_capture_available" class="runtime-banner">
      {{ t('captureUnavailableBanner') }}
    </div>

    <Dialog
      v-model:show="noticeStore.showDialog"
      :title="noticeStore.dialogTitle"
      :message="noticeStore.dialogMessage"
      :ok-label="t('ok')"
      @ok="noticeStore.closeDialog"
    />
  </div>
</template>

<style scoped>
.app-shell {
  height: 100%;
}

.app-loading {
  height: 100%;
}

.runtime-banner {
  position: fixed;
  top: var(--titlebar-height);
  left: 0;
  right: 0;
  z-index: 200;
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid color-mix(in srgb, var(--color-danger) 22%, var(--color-border));
  background: color-mix(in srgb, var(--color-danger) 12%, var(--color-bg));
  color: var(--color-danger);
  font-size: var(--font-size-xs);
  font-weight: var(--font-weight-medium);
  text-align: center;
  pointer-events: none;
}
</style>

