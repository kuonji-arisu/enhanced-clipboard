<script setup lang="ts">
import { onMounted, ref } from 'vue'
import TitleBar from '../components/TitleBar.vue'
import SearchBar from '../components/SearchBar.vue'
import ClipboardList from '../components/ClipboardList.vue'
import Icon from '../components/Icon.vue'
import Dialog from '../components/Dialog.vue'
import { useAsyncAction } from '../composables/useAsyncAction'
import { useClipboardStore } from '../stores/clipboard'
import { useI18n } from '../i18n'
import { useRouter } from 'vue-router'

const store = useClipboardStore()
const router = useRouter()
const { t } = useI18n()
const { run } = useAsyncAction()
const showClearConfirm = ref(false)

onMounted(() => {
  void run(() => store.init(), 'loadEntriesFailed')
})

async function doClear() {
  const success = await run(() => store.clear().then(() => true), 'clearFailed')
  if (success) {
    showClearConfirm.value = false
  }
}
</script>

<template>
  <div class="home">
    <TitleBar :title="t('appTitle')">
      <template #extra-buttons>
        <!-- 全局清空按钮：在标题栏右上角，悬停/点击时显示红色警示 -->
        <button
          class="titlebar-btn titlebar-btn--danger"
          @click="showClearConfirm = true"
          :title="t('clearAll')"
        >
          <Icon name="trash" :size="14" />
        </button>
        <!-- 设置按钮 -->
        <button
          @click="router.push('/settings')"
          :title="t('settings')"
          class="titlebar-btn"
        >
          <Icon name="settings" :size="14" />
        </button>
      </template>
    </TitleBar>

    <!-- 搜索工具栏（不再包含清空按钮） -->
    <div class="home-toolbar">
      <SearchBar class="flex-1" />
    </div>

    <ClipboardList class="home-list" />

    <Dialog
      v-model:show="showClearConfirm"
      :title="t('clearConfirmTitle')"
      :message="t('clearConfirmMsg')"
      :ok-label="t('clearConfirmOk')"
      :cancel-label="t('clearConfirmCancel')"
      ok-variant="danger"
      @ok="doClear"
    />
  </div>
</template>

<style scoped>
.home {
  position: relative;
  display: flex;
  flex-direction: column;
  height: 100%;
}

.home-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--color-border);
  background: var(--color-bg-elevated);
  flex-shrink: 0;
}

.home-list {
  flex: 1;
  min-height: 0;
}
</style>

