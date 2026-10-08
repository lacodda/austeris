import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocale } from 'dowel-ui'
import { Picker, type Picked } from '@/components/Picker'
import { Button } from '@/components/ui/button'
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from '@/components/ui/select'
import { TagInput } from '@/components/ui/tag-input'
import { countryName, placeLabel, useCounterparties, usePlaces } from '@/lib/ledger'
import type { Account, PlaceBody } from '@/lib/types'

/*
 * The fields an entry, an exchange and an entry being edited share: who, where
 * and which labels. One copy, so the three forms cannot drift into three ways
 * of saying where a payment happened.
 */

/** Who the money went to or came from: one of theirs, or a new name. */
export function CounterpartyField({ value, onValueChange }: { value: Picked | null; onValueChange: (value: Picked | null) => void }) {
  const { t } = useTranslation()
  const counterparties = useCounterparties()
  const options = useMemo(
    () => (counterparties.data ?? []).map((counterparty) => ({ value: counterparty.id, label: counterparty.name })),
    [counterparties.data],
  )
  return (
    <Field label={t('entry.counterparty')} help={t('entry.counterpartyHelp')}>
      <Picker
        options={options}
        value={value}
        onValueChange={onValueChange}
        placeholder={t('entry.counterpartyPlaceholder')}
        emptyLabel={t('picker.empty')}
        createLabel={(name) => t('picker.create', { name })}
      />
    </Field>
  )
}

/** A tag is one word starting with a letter, as the ledger takes it (and as a
 * `#tag` is typed in a one-line entry). */
const TAG = /^\p{L}[\p{L}\p{N}_-]*$/u

export function isTag(text: string): boolean {
  return TAG.test(text)
}

export function TagsField({ value, onValueChange }: { value: string[]; onValueChange: (value: string[]) => void }) {
  const { t } = useTranslation()
  const bad = value.filter((tag) => !isTag(tag))
  return (
    <Field label={t('entry.tags')} help={t('entry.tagsHelp')} error={bad.length > 0 ? t('entry.badTags', { tags: bad.join(', ') }) : undefined}>
      <TagInput
        value={value}
        onValueChange={(tags) => onValueChange(tags.map((tag) => tag.replace(/^#/, '').trim()).filter(Boolean))}
        aria-label={t('entry.tags')}
        removeLabel={(tag) => t('entry.removeTag', { tag })}
      />
    </Field>
  )
}

/**
 * Where it happened: one of the places already in the books, or a new one by
 * its country code and city.
 *
 * Countries go by their ISO code, the way the ledger keeps them; the field
 * says the country's name back as it is typed, so `UK` reads as nothing and
 * `GB` as the United Kingdom before the ledger is ever asked.
 */
export function PlaceField({ value, onValueChange }: { value: PlaceBody | null; onValueChange: (value: PlaceBody | null) => void }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const places = usePlaces()
  const known = places.data ?? []
  const matching = value ? known.find((place) => samePlace(place, value)) : undefined
  const [other, setOther] = useState(value !== null && matching === undefined && known.length > 0)

  const NONE = ''
  const items: Record<string, string> = { [NONE]: t('entry.nowhere') }
  for (const place of known) items[place.id] = placeLabel(place, locale)

  if (other || known.length === 0) {
    const code = value?.country ?? ''
    const named = code.length === 2 ? countryName(code, locale) : ''
    return (
      <div className="flex flex-col gap-2">
        <div className="grid grid-cols-[6rem_minmax(0,1fr)] gap-3">
          <Field label={t('entry.country')} help={named && named !== code ? named : t('entry.countryHelp')}>
            <Input
              value={code}
              maxLength={2}
              autoComplete="off"
              className="font-mono uppercase"
              onChange={(event) => {
                const country = event.target.value.toUpperCase()
                onValueChange(country === '' && !value?.city ? null : { country, city: value?.city ?? null })
              }}
            />
          </Field>
          <Field label={t('entry.city')}>
            <Input
              value={value?.city ?? ''}
              onChange={(event) => onValueChange({ country: code, city: event.target.value || null })}
            />
          </Field>
        </div>
        {known.length > 0 ? (
          <Button
            variant="link"
            className="self-start text-xs"
            onClick={() => {
              setOther(false)
              onValueChange(null)
            }}
          >
            {t('entry.knownPlace')}
          </Button>
        ) : null}
      </div>
    )
  }

  return (
    <Field label={t('entry.place')}>
      <div className="flex flex-col gap-1">
        <Select
          items={items}
          value={matching?.id ?? NONE}
          onValueChange={(id) => {
            const place = known.find((each) => each.id === id)
            onValueChange(place ? { country: place.country, city: place.city ?? null } : null)
          }}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectPopup>
            {Object.entries(items).map(([id, label]) => (
              <SelectItem key={id} value={id}>
                {label}
              </SelectItem>
            ))}
          </SelectPopup>
        </Select>
        <Button variant="link" className="self-start text-xs" onClick={() => setOther(true)}>
          {t('entry.otherPlace')}
        </Button>
      </div>
    </Field>
  )
}

function samePlace(a: PlaceBody, b: PlaceBody): boolean {
  return a.country.toUpperCase() === b.country.toUpperCase() && (a.city ?? '') === (b.city ?? '')
}

/** A choice of account, by name with its currency beside it. */
export function AccountSelect({
  accounts,
  value,
  onValueChange,
  label,
  exclude,
}: {
  accounts: readonly Account[]
  value: string | undefined
  onValueChange: (id: string) => void
  label: string
  exclude?: string
}) {
  const { t } = useTranslation()
  const shown = accounts.filter((account) => account.id !== exclude)
  const items = Object.fromEntries(shown.map((account) => [account.id, `${account.name} · ${account.currency}`]))
  return (
    <Field label={label} required>
      <Select items={items} value={value ?? null} onValueChange={(id) => onValueChange(id as string)}>
        <SelectTrigger>
          <SelectValue placeholder={t('entry.chooseAccount')} />
        </SelectTrigger>
        <SelectPopup>
          {shown.map((account) => (
            <SelectItem key={account.id} value={account.id}>
              {account.name} <span className="text-dim">· {account.currency}</span>
            </SelectItem>
          ))}
        </SelectPopup>
      </Select>
    </Field>
  )
}
