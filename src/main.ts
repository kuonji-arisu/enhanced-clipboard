import { createApp } from 'vue'
import { createPinia } from 'pinia'
import { createRouter, createWebHashHistory } from 'vue-router'
import type { Directive } from 'vue'
import App from './App.vue'
import HomeView from './views/HomeView.vue'
import SettingsView from './views/SettingsView.vue'
import './style.css'
// 注入 vite-plugin-svg-icons 生成的 SVG sprite
import 'virtual:svg-icons-register'

// 生产环境禁用浏览器右键菜单
if (!import.meta.env.DEV) {
  document.addEventListener('contextmenu', (e) => e.preventDefault())
}

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', component: HomeView },
    { path: '/settings', component: SettingsView },
  ],
})

// 点击元素外部时触发 binding.value 回调
const clickOutside: Directive = {
  beforeMount(el, binding) {
    el._clickOutsideHandler = (event: MouseEvent) => {
      if (!el.contains(event.target as Node)) {
        binding.value(event)
      }
    }
    document.addEventListener('mousedown', el._clickOutsideHandler)
  },
  unmounted(el) {
    document.removeEventListener('mousedown', el._clickOutsideHandler)
  },
}

const app = createApp(App)
app.use(createPinia())
app.use(router)
app.directive('click-outside', clickOutside)
app.mount('#app')
