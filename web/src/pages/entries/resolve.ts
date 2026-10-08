import type { Picked } from '@/components/Picker'
import { api } from '@/lib/api'
import type { Flow } from '@/lib/types'

/*
 * Turning what a form picked into ids, creating what was only named.
 *
 * Done when the form is saved, not when a name is typed: a form that is
 * cancelled must leave no counterparty or category behind, and one typed
 * twice in the same form must be created once.
 */

export interface Resolver {
  category: (picked: Picked, flow: Flow) => Promise<string>
  counterparty: (picked: Picked | null, defaultCategory?: string) => Promise<string | null>
}

export function resolver(): Resolver {
  const categories = new Map<string, Promise<string>>()
  return {
    category(picked, flow) {
      if ('id' in picked) return Promise.resolve(picked.id)
      const key = `${flow}:${picked.name.toLocaleLowerCase()}`
      let created = categories.get(key)
      if (!created) {
        created = api.createCategory(picked.name, flow).then((category) => category.id)
        categories.set(key, created)
      }
      return created
    },
    async counterparty(picked, defaultCategory) {
      if (picked === null) return null
      if ('id' in picked) return picked.id
      // A new counterparty takes the category it was first paid for as its
      // usual one - the same rule a one-line entry follows - so the next
      // receipt from there can leave the category out.
      const counterparty = await api.createCounterparty(picked.name, defaultCategory)
      return counterparty.id
    },
  }
}
