<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import Dialog from './components/Dialog.vue'
import { fetchRuntimeStatus } from './composables/runtimeApi'
import { useI18n } from './i18n'
import { useAppInfoStore } from './stores/appInfo'
import { useNoticeStore } from './stores/notice'
import { useSettingsStore } from './stores/settings'
import { getErrorMessage } from './utils/errors'
import type { RuntimeStatus } from './types'

const appInfoStore = useAppInfoStore()
const settingsStore = useSettingsStore()
const noticeStore = useNoticeStore()
const { t } = useI18n()
const bootstrapped = ref(false)

let unlistenRuntimeStatus: UnlistenFn | null = null

function applyRuntimeStatus(status: RuntimeStatus) {
  noticeStore.setCaptureRuntimeStatus(
    status.clipboard_capture_available,
    t('captureUnavailableTitle'),
    t('captureUnavailableMessage'),
  )
}

onMounted(async () => {
  try {
    await appInfoStore.load()
    await settingsStore.load()
    unlistenRuntimeStatus = await listen<RuntimeStatus>('runtime_status_changed', (event) => {
      applyRuntimeStatus(event.payload)
    })
    applyRuntimeStatus(await fetchRuntimeStatus())
    bootstrapped.value = true
  } catch (e) {
    noticeStore.openError(t('actionErrorTitle'), getErrorMessage(e, t('appInitFailed')))
  }
})

onUnmounted(() => {
  unlistenRuntimeStatus?.()
})
</script>

<template>
  <div class="app-shell">
    <router-view v-if="bootstrapped" />
    <div v-else class="app-loading" />

    <div v-if="!noticeStore.clipboardCaptureAvailable" class="runtime-banner">
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

