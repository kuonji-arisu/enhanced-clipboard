<script setup lang="ts">
import { computed, ref } from 'vue'
import { getImageSrc } from '../composables/clipboardApi'
import { useAsyncAction } from '../hooks/useAsyncAction'
import { COPY_FEEDBACK_MS } from '../constants'
import { useRelativeTime } from '../hooks/useRelativeTime'
import { useI18n } from '../i18n'
import { useAppInfoStore } from '../stores/appInfo'
import { useClipboardStore } from '../stores/clipboard'
import type { ClipboardEntry } from '../types'
import EntryTagChip from './EntryTagChip.vue'
import Icon from './Icon.vue'
import Tooltip from './Tooltip.vue'

const props = defineProps<{
  entry: ClipboardEntry
}>()

const appInfoStore = useAppInfoStore()
const store = useClipboardStore()
const { t } = useI18n()
const { formatTime } = useRelativeTime()
const { run } = useAsyncAction()
const copied = ref(false)
const pinning = ref(false)
const maxPinnedEntries = computed(
  () => appInfoStore.requireAppInfo().max_pinned_entries,
)
const imageProcessing = computed(
  () => props.entry.content_type === 'image' && !props.entry.thumbnail_path,
)
const visibleTags = computed(() =>
  props.entry.tags.filter((tag) => tag.trim().length > 0),
)

async function handleCopy() {
  const copiedOk = await run(() => store.copy(props.entry.id).then(() => true), 'copyFailed')
  if (copiedOk) {
    copied.value = true
    setTimeout(() => (copied.value = false), COPY_FEEDBACK_MS)
  }
}

async function handleDelete() {
  await run(() => store.remove(props.entry.id), 'deleteFailed')
}

async function handlePin() {
  if (pinning.value) return
  pinning.value = true
  try {
    await run(() => store.togglePin(props.entry.id), 'pinFailed')
  } finally {
    pinning.value = false
  }
}
</script>

<template>
  <div class="entry-card" :class="{ 'entry-card--pinned': entry.is_pinned }">
    <div class="entry-body">
      <div class="entry-content">
        <div v-if="entry.content_type === 'text'" class="entry-text">
          {{ entry.content }}
        </div>
        <div v-else-if="entry.content_type === 'image'" class="entry-image-wrap">
          <!-- thumbnail_path 是唯一展示源，null 表示处理中，始终显示 shimmer -->
          <!-- 原图（image_path）仅供 copy_entry 命令使用，永远不在浏览器中加载 -->
          <img
            v-if="entry.thumbnail_path"
            :src="getImageSrc(entry.thumbnail_path)"
            class="entry-image"
            :alt="t('clipboardImageAlt')"
            loading="lazy"
            @error="store.remove(entry.id)"
          />
          <div v-else class="entry-image-loading"></div>
        </div>
      </div>

      <div class="entry-actions">
        <Tooltip :content="entry.is_pinned ? t('unpin') : t('pin')">
          <button
            class="action-btn action-btn--pin"
            :class="{ 'action-btn--pin--active': entry.is_pinned }"
            :disabled="!entry.is_pinned && store.pinnedCount >= maxPinnedEntries"
            @click="handlePin"
          >
            <Icon :name="entry.is_pinned ? 'pin-off' : 'pin'" :size="13" />
          </button>
        </Tooltip>
        <Tooltip :content="imageProcessing ? t('loading') : copied ? t('copied') : t('copy')">
          <button
            class="action-btn action-btn--copy"
            :disabled="imageProcessing"
            @click="handleCopy"
          >
            <Icon :name="copied ? 'check' : 'copy'" :size="13" />
          </button>
        </Tooltip>
        <Tooltip :content="t('delete')">
          <button class="action-btn action-btn--delete" @click="handleDelete">
            <Icon name="trash" :size="13" />
          </button>
        </Tooltip>
      </div>
    </div>

    <div class="entry-supporting">
      <div class="entry-supporting__tags">
        <div v-if="visibleTags.length > 0" class="entry-tags">
          <EntryTagChip v-for="tag in visibleTags" :key="tag" :tag="tag" />
        </div>
      </div>

      <div class="entry-meta">
        <span class="entry-time">{{ formatTime(entry.created_at) }}</span>
        <span v-if="entry.source_app" class="entry-meta__separator" aria-hidden="true"></span>
        <span v-if="entry.source_app" class="entry-source">{{ entry.source_app }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.entry-card {
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  padding: var(--space-3);
  transition: border-color 0.15s, box-shadow 0.15s;
}

.entry-card:hover {
  border-color: var(--color-accent);
  box-shadow: var(--shadow-sm);
}

.entry-card--pinned {
  border-left: 3px solid var(--color-accent);
  background: color-mix(in srgb, var(--color-accent) 5%, var(--color-bg-elevated));
}

.entry-card:hover .entry-actions {
  opacity: 1;
}

.entry-card--pinned .entry-actions {
  opacity: 1;
}

.entry-body {
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
}

.entry-content {
  flex: 1;
  min-width: 0;
}

.entry-text {
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
  white-space: pre-wrap;
  overflow-wrap: break-word;
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  line-clamp: 3;
  overflow: hidden;
}

.entry-image-loading {
  width: 100%;
  height: 48px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
  background: linear-gradient(
    90deg,
    var(--color-bg-elevated) 25%,
    var(--color-border) 50%,
    var(--color-bg-elevated) 75%
  );
  background-size: 200% 100%;
  animation: shimmer 1.4s infinite;
}

@keyframes shimmer {
  0% {
    background-position: 200% 0;
  }

  100% {
    background-position: -200% 0;
  }
}

.entry-image-wrap {
  display: flex;
}

.entry-image {
  display: block;
  width: auto;
  max-width: 100%;
  max-height: 96px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--color-border);
  object-fit: contain;
}

.entry-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.entry-supporting {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  margin-top: var(--space-2);
  min-height: 20px;
}

.entry-supporting__tags {
  flex: 1;
  min-width: 0;
}

.entry-meta {
  display: inline-flex;
  align-items: center;
  justify-content: flex-end;
  gap: 6px;
  min-width: 0;
  flex-shrink: 0;
}

.entry-time {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  flex-shrink: 0;
}

.entry-meta__separator {
  width: 3px;
  height: 3px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--color-text-tertiary) 78%, transparent);
  flex-shrink: 0;
}

.entry-source {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  max-width: 112px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.entry-actions {
  display: flex;
  gap: var(--space-1);
  opacity: 0;
  transition: opacity 0.15s;
  flex-shrink: 0;
}

.action-btn {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-sm);
  border: none;
  cursor: pointer;
  transition: background 0.15s, color 0.15s;
  background: transparent;
}

.action-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.action-btn--pin {
  color: var(--color-text-tertiary);
}

.action-btn--pin:hover {
  background: var(--color-accent-subtle);
  color: var(--color-accent);
}

.action-btn--pin--active {
  color: var(--color-accent);
}

.action-btn--pin--active:hover {
  background: var(--color-accent-subtle);
}

.action-btn--copy {
  color: var(--color-accent);
}

.action-btn--copy:hover {
  background: var(--color-accent-subtle);
}

.action-btn--delete {
  color: var(--color-danger);
}

.action-btn--delete:hover {
  background: color-mix(in srgb, var(--color-danger) 12%, transparent);
}
</style>
