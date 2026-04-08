<script setup lang="ts">
import { toRef } from 'vue'
import { getImageSrc } from '../composables/clipboardApi'
import { useEntryAnimations } from '../hooks/useEntryAnimations'
import { useRelativeTime } from '../hooks/useRelativeTime'
import { useI18n } from '../i18n'
import { useClipboardStore } from '../stores/clipboard'
import type { ClipboardEntry } from '../types'
import Icon from './Icon.vue'
import Tooltip from './Tooltip.vue'

const props = defineProps<{
  entry: ClipboardEntry
  animateIn?: boolean
}>()

const store = useClipboardStore()
const entryRef = toRef(props, 'entry')
const { t } = useI18n()
const { formatTime, formatFull } = useRelativeTime()
const {
  copied,
  pinFeedback,
  imageProcessing,
  deleting,
  actionDisabled,
  pinButtonDisabled,
  motionVars,
  handleCopy,
  handleDelete,
  handlePin,
} = useEntryAnimations(entryRef)
</script>

<template>
  <div
    class="entry-card"
    :class="{
      'entry-card--pinned': entry.is_pinned,
      'entry-card--entering': animateIn,
      'entry-card--deleting': deleting,
      'entry-card--pin-feedback-on': pinFeedback === 'on',
      'entry-card--pin-feedback-off': pinFeedback === 'off',
    }"
    :style="motionVars"
  >
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
            :class="{
              'action-btn--pin--active': entry.is_pinned,
              'action-btn--pin--feedback': pinFeedback !== null,
            }"
            :disabled="pinButtonDisabled"
            :aria-label="entry.is_pinned ? t('unpin') : t('pin')"
            @click="handlePin"
          >
            <Icon :name="entry.is_pinned ? 'pin-off' : 'pin'" :size="13" />
          </button>
        </Tooltip>
        <Tooltip :content="imageProcessing ? t('loading') : copied ? t('copied') : t('copy')">
          <button
            class="action-btn action-btn--copy"
            :disabled="actionDisabled || imageProcessing"
            :aria-label="imageProcessing ? t('loading') : copied ? t('copied') : t('copy')"
            @click="handleCopy"
          >
            <Icon :name="copied ? 'check' : 'copy'" :size="13" />
          </button>
        </Tooltip>
        <Tooltip :content="t('delete')">
          <button
            class="action-btn action-btn--delete"
            :disabled="actionDisabled"
            :aria-label="t('delete')"
            @click="handleDelete"
          >
            <Icon name="trash" :size="13" />
          </button>
        </Tooltip>
      </div>
    </div>

    <div class="entry-meta">
      <Tooltip :content="formatFull(entry.created_at)">
        <span class="entry-time">{{ formatTime(entry.created_at) }}</span>
      </Tooltip>
      <span v-if="entry.source_app" class="entry-source">{{ entry.source_app }}</span>
    </div>
  </div>
</template>

<style scoped>
.entry-card {
  position: relative;
  background: var(--color-bg-elevated);
  border: 1px solid var(--color-border);
  border-radius: var(--radius-md);
  padding: var(--space-3);
  transition: border-color 0.15s, box-shadow 0.15s;
  transform-origin: center;
}

.entry-card::before {
  content: '';
  position: absolute;
  top: var(--space-2);
  bottom: var(--space-2);
  left: 0;
  width: 3px;
  border-radius: 999px;
  background: var(--color-accent);
  opacity: 0;
  transform: scaleY(0.6);
}

.entry-card:hover {
  border-color: var(--color-accent);
  box-shadow: var(--shadow-sm);
}

.entry-card--pinned {
  background: color-mix(in srgb, var(--color-accent) 5%, var(--color-bg-elevated));
}

.entry-card--pinned::before {
  opacity: 1;
  transform: scaleY(1);
}

.entry-card--entering {
  animation: entry-fade-in var(--entry-enter-duration, 180ms) ease-out;
}

.entry-card--deleting {
  opacity: 0;
  transform: scale(0.985);
  transition:
    opacity var(--entry-exit-duration) ease,
    transform var(--entry-exit-duration) ease;
  pointer-events: none;
}

.entry-card--pin-feedback-on::before {
  animation: pin-bar-in var(--entry-pin-duration) ease-out both;
}

.entry-card--pin-feedback-off::before {
  animation: pin-bar-out var(--entry-pin-duration) ease-out both;
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

@keyframes entry-fade-in {
  from {
    opacity: 0;
    transform: translateY(6px);
  }

  to {
    opacity: 1;
    transform: translateY(0);
  }
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

.entry-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: var(--space-1);
  gap: var(--space-2);
}

.entry-time {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  flex-shrink: 0;
}

.entry-source {
  font-size: var(--font-size-xs);
  color: var(--color-text-tertiary);
  max-width: 100px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  text-align: right;
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

.action-btn--pin--feedback {
  animation: pin-btn-pop var(--entry-pin-duration) ease-out;
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

@keyframes pin-btn-pop {
  0% {
    transform: scale(1);
  }

  35% {
    transform: scale(0.9);
  }

  70% {
    transform: scale(1.08);
  }

  100% {
    transform: scale(1);
  }
}

@keyframes pin-bar-in {
  0% {
    opacity: 0;
    transform: scaleY(0.45);
  }

  65% {
    opacity: 1;
    transform: scaleY(1.15);
  }

  100% {
    opacity: 1;
    transform: scaleY(1);
  }
}

@keyframes pin-bar-out {
  0% {
    opacity: 1;
    transform: scaleY(1);
  }

  40% {
    opacity: 1;
    transform: scaleY(1.08);
  }

  100% {
    opacity: 0;
    transform: scaleY(0.4);
  }
}

@media (prefers-reduced-motion: reduce) {
  .entry-card,
  .entry-card::before,
  .entry-actions,
  .action-btn {
    animation: none !important;
    transition-duration: 0s !important;
  }
}
</style>
