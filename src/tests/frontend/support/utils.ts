import { mount, type MountingOptions } from '@vue/test-utils'
import { getActivePinia } from 'pinia'
import { installTestPinia } from './pinia'

export async function flushPromises(): Promise<void> {
  await Promise.resolve()
  await Promise.resolve()
}

export function mountWithPinia(
  component: unknown,
  options: MountingOptions<any> = {},
) {
  const pinia = getActivePinia() ?? installTestPinia()
  const globalOptions = options.global ?? {}

  return {
    pinia,
    wrapper: mount(component as any, {
      ...options,
      global: {
        ...globalOptions,
        plugins: [...(globalOptions.plugins ?? []), pinia],
        directives: {
          clickOutside: {
            mounted() {},
            unmounted() {},
          },
          ...globalOptions.directives,
        },
        stubs: {
          teleport: true,
          ...globalOptions.stubs,
        },
      },
    } as any),
  }
}
