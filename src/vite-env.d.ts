/// <reference types="vite/client" />
/// <reference types="vite-plugin-svg-icons/client" />

declare module 'virtual:svg-icons-register' {
  const register: () => void
  export default register
}

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}
