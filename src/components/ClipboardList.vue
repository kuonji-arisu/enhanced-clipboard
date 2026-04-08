<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useVirtualizer } from '@tanstack/vue-virtual'
import ClipboardItem from './ClipboardItem.vue'
import { useClipboardStore } from '../stores/clipboard'
import { useI18n } from '../i18n'
import { getErrorMessage } from '../utils/errors'
import {
  LOAD_MORE_THRESHOLD,
  VIRTUAL_ITEM_ESTIMATE_SIZE,
  VIRTUAL_LIST_GAP,
  VIRTUAL_LIST_OVERSCAN,
  VIRTUAL_LIST_PADDING,
} from '../constants'

const store = useClipboardStore()
const { t } = useI18n()
const loadMoreError = ref('')

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
  count: store.entries.length,
  getScrollElement: () => scrollRef.value,
  estimateSize: () => VIRTUAL_ITEM_ESTIMATE_SIZE,
  gap: VIRTUAL_LIST_GAP,
  paddingStart: VIRTUAL_LIST_PADDING,
  paddingEnd: VIRTUAL_LIST_PADDING,
  overscan: VIRTUAL_LIST_OVERSCAN,
})))

const virtualItems = computed(() => virtualizer.value.getVirtualItems())
const totalSize = computed(() => virtualizer.value.getTotalSize())

async function tryLoadMore() {
  try {
    await store.loadMore()
    loadMoreError.value = ''
  } catch (error) {
    loadMoreError.value = getErrorMessage(error, t('loadEntriesFailed'))
  }
}

/**
 * 当最后一个可见条目的索引接近数组末尾时触发加载更多。
 * 提前 8 条预加载，减少用户等待感。
 */
watch(virtualItems, (items) => {
  if (!items.length || !store.hasMore || store.loadingMore || loadMoreError.value) return
  const lastVisible = items[items.length - 1]
  if (lastVisible.index >= store.entries.length - LOAD_MORE_THRESHOLD) {
    void tryLoadMore()
  }
})
</script>

<template>
  <div ref="scrollRef" class="list-container">
    <div v-if="store.loading" class="list-state">{{ t('loading') }}</div>
    <div v-else-if="store.entries.length === 0" class="list-state list-state--empty">
      {{ t('noEntries') }}
    </div>
    <template v-else>
      <!-- 虚拟滚动内容区：高度由虚拟化器维护，items 绝对定位于其中 -->
      <div class="virtual-content" :style="{ height: `${totalSize}px` }">
        <div
          v-for="item in virtualItems"
          :key="store.entries[item.index]?.id ?? item.index"
          :data-index="item.index"
          :ref="(el) => el && virtualizer.measureElement(el as Element)"
          class="virtual-item"
          :style="{ transform: `translateY(${item.start}px)` }"
        >
          <ClipboardItem :entry="store.entries[item.index]" />
        </div>
      </div>
      <!-- 加载更多指示器：显示在虚拟内容区下方 -->
      <div v-if="store.loadingMore" class="list-state list-state--more">
        {{ t('loading') }}
      </div>
      <div v-else-if="loadMoreError" class="list-state list-state--more list-state--error">
        <span>{{ loadMoreError }}</span>
        <button class="list-retry-btn" @click="tryLoadMore">{{ t('retry') }}</button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.list-container {
  height: 100%;
  overflow-y: auto;
  /* 左右 padding 保留，上下由虚拟化器的 paddingStart/paddingEnd 控制 */
  padding: 0 var(--space-3);
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

.list-retry-btn {
  border: 1px solid var(--color-border);
  border-radius: var(--radius-sm);
  padding: 2px 8px;
  background: var(--color-bg-elevated);
  color: inherit;
  font-size: inherit;
  cursor: pointer;
}
</style>

