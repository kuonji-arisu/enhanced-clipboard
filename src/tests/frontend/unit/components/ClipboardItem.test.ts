import { nextTick, ref } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import ClipboardItem from '../../../../components/ClipboardItem.vue'
import { createAppInfo, createImageListItem, createTextListItem } from '../../support/factories'
import { installTestPinia, primeAppInfoStore } from '../../support/pinia'
import { setTauriInvokeHandler } from '../../support/tauri'
import { mountWithPinia, flushPromises } from '../../support/utils'

const pinnedCount = ref(0)

vi.mock('../../../../hooks/useAsyncAction', () => ({
  useAsyncAction: () => ({
    run: async <T>(action: () => Promise<T>) => action(),
  }),
}))

vi.mock('../../../../hooks/useRelativeTime', () => ({
  useRelativeTime: () => ({
    formatTime: () => 'just now',
  }),
}))

vi.mock('../../../../hooks/useClipboardCurrentList', () => ({
  useClipboardCurrentList: () => ({
    pinnedCount,
  }),
}))

describe('ClipboardItem', () => {
  beforeEach(() => {
    installTestPinia()
    primeAppInfoStore(createAppInfo())
    pinnedCount.value = 0
  })

  it('routes copy, delete, and pin actions through the shared clipboard actions store', async () => {
    const commands: string[] = []
    setTauriInvokeHandler(async (command) => {
      commands.push(command)
      if (command === 'report_image_load_failed') {
        return true
      }
      return undefined
    })

    const { wrapper } = mountWithPinia(ClipboardItem, {
      props: {
        entry: createTextListItem(),
      },
    })

    await wrapper.find('.action-btn--copy').trigger('click')
    await wrapper.find('.action-btn--delete').trigger('click')
    await wrapper.find('.action-btn--pin').trigger('click')

    expect(commands).toEqual(['copy_entry', 'delete_entry', 'toggle_pin'])
  })

  it('disables pinning when the pinned limit is already reached', () => {
    pinnedCount.value = 3

    const { wrapper } = mountWithPinia(ClipboardItem, {
      props: {
        entry: createTextListItem({ is_pinned: false }),
      },
    })

    expect(wrapper.find('.action-btn--pin').attributes('disabled')).toBeDefined()
  })

  it('reports broken images through the shared clipboard actions path', async () => {
    const commands: string[] = []
    setTauriInvokeHandler(async (command) => {
      commands.push(command)
      if (command === 'report_image_load_failed') {
        return false
      }
      return undefined
    })

    const { wrapper } = mountWithPinia(ClipboardItem, {
      props: {
        entry: createImageListItem(),
      },
    })

    await wrapper.find('img').trigger('error')
    await flushPromises()
    await nextTick()

    expect(commands).toEqual(['report_image_load_failed'])
  })

  it('suppresses duplicate pin requests while an earlier toggle is still running', async () => {
    const commands: string[] = []
    let resolveToggle: () => void = () => {}
    setTauriInvokeHandler((command) => {
      commands.push(command)
      if (command === 'toggle_pin') {
        return new Promise<void>((resolve) => {
          resolveToggle = resolve
        })
      }
      return undefined
    })

    const { wrapper } = mountWithPinia(ClipboardItem, {
      props: {
        entry: createTextListItem(),
      },
    })

    const pinButton = wrapper.find('.action-btn--pin')
    const firstClick = pinButton.trigger('click')
    const secondClick = pinButton.trigger('click')
    await flushPromises()

    expect(commands).toEqual(['toggle_pin'])

    resolveToggle()
    await Promise.all([firstClick, secondClick])
  })

  it('shows the broken-image fallback and avoids duplicate reports after a failed removal acknowledgement', async () => {
    const commands: string[] = []
    let resolveReport: (value: boolean) => void = () => {}
    setTauriInvokeHandler((command) => {
      commands.push(command)
      if (command === 'report_image_load_failed') {
        return new Promise<boolean>((resolve) => {
          resolveReport = resolve
        })
      }
      return undefined
    })

    const { wrapper } = mountWithPinia(ClipboardItem, {
      props: {
        entry: createImageListItem(),
      },
    })

    const image = wrapper.find('img')
    const firstError = image.trigger('error')
    const secondError = image.trigger('error')
    await flushPromises()

    expect(commands).toEqual(['report_image_load_failed'])

    resolveReport(false)
    await Promise.all([firstError, secondError])
    await flushPromises()
    await nextTick()
    await nextTick()

    expect(wrapper.find('.entry-image-broken').exists()).toBe(true)
    expect(commands).toEqual(['report_image_load_failed'])
  })
})
