<script setup lang="ts">
// 统一 SVG 图标组件，使用 vite-plugin-svg-icons 注入的 sprite
defineProps<{
  name: string        // 图标名称，对应 assets/icons/{name}.svg
  size?: number       // 图标尺寸（px），默认 14
}>()
</script>

<template>
  <svg
    :width="size ?? 14"
    :height="size ?? 14"
    aria-hidden="true"
    class="icon"
  >
    <use :href="`#icon-${name}`" />
  </svg>
</template>

<style scoped>
.icon {
  /* 继承父元素颜色，使 stroke/fill 随主题变化 */
  color: inherit;
  flex-shrink: 0;
}
</style>
