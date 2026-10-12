import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Chip, ChipGroup } from '@/components/ui/chip'
import { addDays } from '@/components/ui/calendar-math'
import { DatePicker } from '@/components/ui/date-picker'
import { Drawer } from '@/components/ui/drawer'
import { Container, PageHeader } from '@/components/ui/page-header'
import { today } from '@/lib/dates'
import { openAccounts, useAccounts } from '@/lib/ledger'
import { EntryDetail } from '@/pages/entries/EntryDetail'
import { QuickLine } from '@/pages/entries/QuickLine'
import { Buttons } from '@/pages/record/Buttons'
import { Recent } from '@/pages/record/Recent'

/** Where this browser keeps the account the screen records against. */
const ACCOUNT_KEY = 'austeris.record.account'

function remembered(): string | undefined {
  try {
    return localStorage.getItem(ACCOUNT_KEY) ?? undefined
  } catch {
    // Storage refused: the person chooses again.
    return undefined
  }
}

function remember(id: string) {
  try {
    localStorage.setItem(ACCOUNT_KEY, id)
  } catch {
    // Not remembered; still chosen for this visit.
  }
}

/**
 * Recording what was just spent, where it was spent: the phone's screen.
 *
 * The account and the day are chosen once, beside the line rather than in it,
 * and stay while several things are recorded. Then a typed line, a button, or
 * yesterday's entry again - and what was recorded today underneath, a press
 * away from its card.
 *
 * The account is remembered in this browser; with none remembered and more
 * than one open, none is chosen, and a line naming none is asked about rather
 * than posted somewhere nobody agreed on.
 */
export function Record() {
  const { t } = useTranslation()
  const accounts = useAccounts()
  const open = openAccounts(accounts.data)
  const [chosen, setChosen] = useState<string | undefined>(remembered)
  const account = open.find((each) => each.id === chosen)?.id ?? (open.length === 1 ? open[0]?.id : undefined)
  const now = today()
  const yesterday = addDays(now, -1)
  const [day, setDay] = useState(now)
  const [opened, setOpened] = useState<string | null>(null)

  const pressedDay = day === now ? ['today'] : day === yesterday ? ['yesterday'] : []

  return (
    <Container width="prose" className="flex flex-col gap-5">
      <PageHeader title={t('record.title')} description={t('record.description')} className="mb-0" />

      <div className="flex flex-col gap-3">
        {open.length > 1 ? (
          <div className="flex flex-col gap-1.5">
            <span id="record-account" className="caption">
              {t('record.account')}
            </span>
            <ChipGroup
              aria-labelledby="record-account"
              value={account ? [account] : []}
              onValueChange={(value) => {
                const next = value[0]
                if (next) {
                  setChosen(next)
                  remember(next)
                }
              }}
            >
              {open.map((each) => (
                <Chip key={each.id} value={each.id}>
                  {each.name}
                </Chip>
              ))}
            </ChipGroup>
          </div>
        ) : null}
        <div className="flex flex-col gap-1.5">
          <span id="record-day" className="caption">
            {t('record.day')}
          </span>
          <div className="flex flex-wrap items-center gap-2">
            <ChipGroup
              aria-labelledby="record-day"
              value={pressedDay}
              onValueChange={(value) => {
                if (value[0] === 'today') setDay(now)
                if (value[0] === 'yesterday') setDay(yesterday)
              }}
            >
              <Chip value="today">{t('record.today')}</Chip>
              <Chip value="yesterday">{t('record.yesterday')}</Chip>
            </ChipGroup>
            <DatePicker
              value={day}
              onValueChange={setDay}
              max={now}
              placeholder={t('record.day')}
              aria-label={t('record.otherDay')}
              previousMonthLabel={t('calendar.previous')}
              nextMonthLabel={t('calendar.next')}
              className="w-auto"
            />
          </div>
        </div>
        <QuickLine accountId={account} day={day} />
      </div>

      <Buttons day={day} />
      <Recent day={day} onOpen={setOpened} />

      <Drawer swipeDirection="right" open={opened !== null} onOpenChange={(isOpen) => (isOpen ? undefined : setOpened(null))}>
        {opened ? <EntryDetail id={opened} onDone={() => setOpened(null)} /> : null}
      </Drawer>
    </Container>
  )
}
