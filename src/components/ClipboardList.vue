<script setup lang="ts">
import { ref, computed, watch, nextTick } from 'vue'
import { useVirtualizer } from '@tanstack/vue-virtual'
import ClipboardItem from './ClipboardItem.vue'
import Icon from './Icon.vue'
import Tooltip from './Tooltip.vue'
import { useClipboardView } from '../hooks/useClipboardView'
import { useI18n } from '../i18n'
import { getErrorMessage } from '../utils/errors'
import {
  LOAD_MORE_THRESHOLD,
  VIRTUAL_ITEM_ESTIMATE_SIZE,
  VIRTUAL_LIST_GAP,
  VIRTUAL_LIST_OVERSCAN,
  VIRTUAL_LIST_PADDING,
} from '../constants'

const clipboardView = useClipboardView()
const queryStore = clipboardView.queryStore
const { t } = useI18n()
const loadMoreError = ref('')
const showScrollTopButton = ref(false)
const entries = clipboardView.entries
const loading = clipboardView.loading
const loadingMore = clipboardView.loadingMore
const hasMore = clipboardView.hasMore

/** 滚动容器 ref */
const scrollRef = ref<HTMLElement | null>(null)

/**
 * 虚拟化器：
 * - estimateSize: 文字条目约 80px，图片约 130px，取中间值 90px 作估算
 * - gap: 8px = --space-2，与原 flex gap 一致
 * - paddingStart/paddingEnd: 12px = --space-3，与原容器 padding 一致
 * - overscan: 前后各多渲染 5 个，避免快速滚动时白屏
 */
const virtualizer = useVirtualizer(computed(() => ({
  count: entries.value.length,
  getScrollElement: () => scrollRef.value,
  estimateSize: () => VIRTUAL_ITEM_ESTIMATE_SIZE,
  gap: VIRTUAL_LIST_GAP,
  paddingStart: VIRTUAL_LIST_PADDING,
  paddingEnd: VIRTUAL_LIST_PADDING,
  overscan: VIRTUAL_LIST_OVERSCAN,
})))

const virtualItems = computed(() => virtualizer.value.getVirtualItems())
const totalSize = computed(() => virtualizer.value.getTotalSize())

function updateScrollTopButton() {
  const el = scrollRef.value
  if (!el || loading.value || entries.value.length === 0) {
    showScrollTopButton.value = false
    return
  }

  showScrollTopButton.value = el.scrollTop > el.clientHeight
}

async function tryLoadMore() {
  try {
    await clipboardView.loadMore()
    loadMoreError.value = ''
  } catch (error) {
    loadMoreError.value = getErrorMessage(error, t('loadEntriesFailed'))
  }
}

async function refreshStaleSnapshot() {
  try {
    await clipboardView.refreshStaleSnapshot()
    loadMoreError.value = ''
  } catch (error) {
    loadMoreError.value = getErrorMessage(error, t('loadEntriesFailed'))
  }
}

function handleScroll() {
  updateScrollTopButton()
}

function scrollToTop() {
  virtualizer.value.scrollToOffset(0, {
    behavior: 'auto',
  })
}

/**
 * 当最后一个可见条目的索引接近数组末尾时触发加载更多。
 * 提前 8 条预加载，减少用户等待感。
 */
watch(virtualItems, (items) => {
  if (!items.length || !hasMore.value || loadingMore.value || loadMoreError.value) return
  const lastVisible = items[items.length - 1]
  if (lastVisible.index >= entries.value.length - LOAD_MORE_THRESHOLD) {
    void tryLoadMore()
  }
})

watch(
  () => [entries.value.length, loading.value] as const,
  async () => {
    await nextTick()
    updateScrollTopButton()
  },
  { immediate: true },
)
</script>

<template>
  <div class="list-shell">
    <div ref="scrollRef" class="list-container" @scroll="handleScroll">
      <div v-if="loading" class="list-state">{{ t('loading') }}</div>
      <template v-else>
        <div v-if="queryStore.stale" class="list-state list-state--stale">
          <span>{{ t('snapshotStale') }}</span>
          <button class="list-retry-btn" @click="refreshStaleSnapshot">{{ t('refresh') }}</button>
        </div>
        <div v-if="entries.length === 0" class="list-state list-state--empty">
          {{ t('noEntries') }}
        </div>
        <!-- 虚拟滚动内容区：高度由虚拟化器维护，items 绝对定位于其中 -->
        <div v-else class="virtual-content" :style="{ height: `${totalSize}px` }">
          <div
            v-for="item in virtualItems"
            :key="entries[item.index]?.id ?? item.index"
            :data-index="item.index"
            :ref="(el) => el && virtualizer.measureElement(el as Element)"
            class="virtual-item"
            :style="{ transform: `translateY(${item.start}px)` }"
          >
            <ClipboardItem :entry="entries[item.index]" />
          </div>
        </div>
        <!-- 加载更多指示器：显示在虚拟内容区下方 -->
        <div v-if="loadingMore" class="list-state list-state--more">
          {{ t('loading') }}
        </div>
        <div v-else-if="loadMoreError" class="list-state list-state--more list-state--error">
          <span>{{ loadMoreError }}</span>
          <button class="list-retry-btn" @click="tryLoadMore">{{ t('retry') }}</button>
        </div>
      </template>
    </div>

    <Transition name="scroll-top-fab">
      <div v-if="showScrollTopButton" class="scroll-top-fab">
        <Tooltip :content="t('backToTop')" :delay="500">
          <button
            class="scroll-top-btn"
            type="button"
            :aria-label="t('backToTop')"
            @click="scrollToTop"
          >
            <Icon name="back" :size="14" class="scroll-top-btn__icon" />
          </button>
        </Tooltip>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.list-shell {
  position: relative;
  height: 100%;
}

.list-container {
  height: 100%;
  overflow-y: auto;
  /* 左右 padding 保留，上下由虚拟化器的 paddingStart/paddingEnd 控制 */
  padding: 0 var(--space-3);
  scrollbar-width: none;
  -ms-overflow-style: none;
}

.list-container::-webkit-scrollbar {
  width: 0;
  height: 0;
}

.virtual-content {
  position: relative;
  width: 100%;
}

.virtual-item {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
}

.list-state {
  text-align: center;
  padding: var(--space-6) 0;
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
}

.list-state--empty {
  color: var(--color-text-tertiary);
}

.list-state--more {
  padding: var(--space-2) 0;
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
}

.list-state--error {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  color: var(--color-danger);
}

.list-state--stale {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  padding: var(--space-2) 0;
  color: var(--color-text-tertiary);
}

.list-retry-btn {
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  padding: 2px 8px;
  background: var(--color-bg-elevated);
  color: inherit;
  font-size: inherit;
  cursor: pointer;
}

.scroll-top-fab {
  position: absolute;
  right: var(--space-4);
  bottom: var(--space-4);
  z-index: 2;
}

.scroll-top-btn {
  width: 34px;
  height: 34px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid color-mix(in srgb, var(--color-accent) 12%, var(--color-border));
  border-radius: 999px;
  background: color-mix(in srgb, var(--color-bg-overlay) 94%, var(--color-bg-elevated));
  color: var(--color-text-secondary);
  box-shadow: var(--shadow-md);
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
  cursor: pointer;
  transition: transform 0.16s ease, background 0.16s ease, color 0.16s ease, border-color 0.16s ease;
}

.scroll-top-btn:hover {
  transform: translateY(-1px);
  background: color-mix(in srgb, var(--color-accent-subtle) 65%, var(--color-bg-elevated));
  border-color: color-mix(in srgb, var(--color-accent) 32%, var(--color-border));
  color: var(--color-accent);
}

.scroll-top-btn__icon {
  transform: rotate(90deg);
}

.scroll-top-fab-enter-active,
.scroll-top-fab-leave-active {
  transition: opacity 0.16s ease, transform 0.16s ease;
}

.scroll-top-fab-enter-from,
.scroll-top-fab-leave-to {
  opacity: 0;
  transform: translateY(6px);
}
</style>

