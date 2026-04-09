<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import Icon from './Icon.vue'
import Tooltip from './Tooltip.vue'
import { usePinState } from '../hooks/usePinState'
import { useI18n } from '../i18n'

defineProps<{
  title: string
}>()

const { pinned, togglePin } = usePinState()
const { t } = useI18n()

const win = getCurrentWindow()

// 双 rAF 确保浏览器完成一次 paint 后 :hover 状态被清除，再触发关闭。
function close(e: MouseEvent) {
  ;(e.currentTarget as HTMLElement).blur()
  document.documentElement.style.pointerEvents = 'none'
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      document.documentElement.style.pointerEvents = ''
      win.close()
    })
  })
}
</script>

<template>
  <header data-tauri-drag-region class="titlebar">
    <span data-tauri-drag-region class="titlebar-title">{{ title }}</span>

    <div class="titlebar-actions" @mousedown.stop>
      <!-- 置顶按钮：激活时使用 accent 背景 -->
      <Tooltip :content="pinned ? t('unpinWindow') : t('pinWindow')">
        <button
          @click="togglePin"
          class="titlebar-btn"
          :class="{ 'titlebar-btn--active': pinned }"
        >
          <Icon :name="pinned ? 'pin-off' : 'pin'" :size="14" />
        </button>
      </Tooltip>

      <!-- 页面自定义按钮插槽（设置、返回等） -->
      <slot name="extra-buttons" />

      <!-- 关闭按钮（最小化到托盘） -->
      <Tooltip :content="t('minimizeToTray')">
        <button @click="close" class="titlebar-btn titlebar-btn--close">
          <Icon name="close" :size="12" />
        </button>
      </Tooltip>
    </div>
  </header>
</template>

<style scoped>
.titlebar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: var(--titlebar-height);
  padding: 0 var(--space-3);
  background: var(--color-bg-titlebar);
  border-bottom: 1px solid var(--color-border);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  user-select: none;
  flex-shrink: 0;
}

.titlebar-title {
  font-size: var(--font-size-sm);
  font-weight: var(--font-weight-semibold);
  color: var(--color-text-primary);
  pointer-events: none;
}

.titlebar-actions {
  display: flex;
  align-items: center;
  gap: var(--space-1);
}
</style>

