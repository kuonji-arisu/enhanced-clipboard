import { describe, expect, it } from 'vitest'
import HighlightedText from '../../../../components/HighlightedText.vue'
import { mountWithPinia } from '../../support/utils'

describe('HighlightedText', () => {
  it('renders highlighted segments from character ranges', () => {
    const { wrapper } = mountWithPinia(HighlightedText, {
      props: {
        text: 'A你B好',
        ranges: [
          { start: 1, end: 2 },
          { start: 3, end: 4 },
        ],
      },
    })

    const marks = wrapper.findAll('.highlighted-text__mark')
    expect(marks).toHaveLength(2)
    expect(marks[0].text()).toBe('你')
    expect(marks[1].text()).toBe('好')
    expect(wrapper.text()).toBe('A你B好')
  })
})
