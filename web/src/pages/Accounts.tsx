import { useMemo, useState } from 'react'
import { Link } from 'react-router'
import { useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { MoreHorizontal } from 'lucide-react'
import { Money } from '@/components/Money'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { Menu, MenuItem, MenuPopup, MenuTrigger } from '@/components/ui/menu'
import { Container, PageHeader } from '@/components/ui/page-header'
import { Panel } from '@/components/ui/panel'
import { QueryState } from '@/components/ui/query-state'
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableScroll } from '@/components/ui/table'
import { useToastManager } from '@/components/ui/toast'
import { api } from '@/lib/api'
import { useAccounts, useBalances, useBooksChanged } from '@/lib/ledger'
import type { Account, Balance, Total } from '@/lib/types'
import { NewAccount } from '@/pages/accounts/NewAccount'

const TOTAL_CURRENCY = 'austeris.totalCurrency'

/**
 * The currency the total is added up in.
 *
 * Remembered per browser, like the language: it is how this person reads
 * their money, not a fact about the books. Until they choose, the currency
 * most of their accounts are in - for a household paid in guaraníes with a
 * dollar account on the side, that is the one a total means something in.
 */
function useTotalCurrency(accounts: readonly Account[]): [string | undefined, (currency: string) => void] {
  const [chosen, setChosen] = useState<string | null>(() => {
    try {
      return localStorage.getItem(TOTAL_CURRENCY)
    } catch {
      return null
    }
  })
  const held = useMemo(() => currencies(accounts), [accounts])
  const current = chosen && held.includes(chosen) ? chosen : held[0]
  const choose = (currency: string) => {
    setChosen(currency)
    try {
      localStorage.setItem(TOTAL_CURRENCY, currency)
    } catch {
      // Not remembered; still chosen for this visit.
    }
  }
  return [current, choose]
}

/** The currencies money is held in, the commonest first. */
function currencies(accounts: readonly Account[]): string[] {
  const count = new Map<string, number>()
  for (const account of accounts) count.set(account.currency, (count.get(account.currency) ?? 0) + 1)
  return [...count.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([currency]) => currency)
}

/** Every account, what it holds, and what that adds up to. */
export function Accounts() {
  const { t } = useTranslation()
  const accounts = useAccounts()
  const open = useMemo(() => (accounts.data ?? []).filter((account) => !account.closed_at), [accounts.data])
  const closed = useMemo(() => (accounts.data ?? []).filter((account) => account.closed_at), [accounts.data])
  const [currency, setCurrency] = useTotalCurrency(open)
  const balances = useBalances(currency)
  const byAccount = useMemo(
    () => new Map((balances.data?.accounts ?? []).map((balance) => [balance.account_id, balance])),
    [balances.data],
  )

  // The column of what is available is drawn only when something is held:
  // otherwise it repeats the balance, or stands empty.
  const anyHeld = (balances.data?.accounts ?? []).some((balance) => balance.available !== balance.amount)

  return (
    <Container>
      <PageHeader title={t('accounts.title')} description={t('accounts.description')} actions={<NewAccount />} />
      <QueryState
        pending={accounts.isPending || (open.length > 0 && balances.isPending)}
        error={accounts.error ?? balances.error}
        empty={open.length === 0 && closed.length === 0}
        emptyState={<EmptyState title={t('accounts.emptyTitle')} body={t('accounts.emptyBody')} action={<NewAccount />} />}
        errorLabels={{ title: t('common.loadFailed') }}
        onRetry={() => void (accounts.error ? accounts.refetch() : balances.refetch())}
        retryLabel={t('common.retry')}
      >
        <div className="flex flex-col gap-4">
          {balances.data?.total && currency ? (
            <TotalPanel
              total={balances.data.total}
              currencies={currencies(open)}
              currency={currency}
              onCurrencyChange={setCurrency}
            />
          ) : null}
          {open.length > 0 ? (
            <Panel>
              <TableScroll>
                <Table>
                  <TableHead>
                    <TableRow>
                      <TableHeader>{t('accounts.account')}</TableHeader>
                      <TableHeader numeric>{t('accounts.balance')}</TableHeader>
                      {anyHeld ? <TableHeader numeric>{t('accounts.available')}</TableHeader> : null}
                      {currency ? <TableHeader numeric>{t('accounts.inCurrency', { currency })}</TableHeader> : null}
                      <TableHeader>
                        <span className="sr-only">{t('common.actions')}</span>
                      </TableHeader>
                    </TableRow>
                  </TableHead>
                  <TableBody>
                    {open.map((account) => (
                      <AccountRow
                        key={account.id}
                        account={account}
                        balance={byAccount.get(account.id)}
                        totalCurrency={currency}
                        showAvailable={anyHeld}
                      />
                    ))}
                  </TableBody>
                </Table>
              </TableScroll>
            </Panel>
          ) : null}
          {closed.length > 0 ? <ClosedAccounts accounts={closed} /> : null}
        </div>
      </QueryState>
    </Container>
  )
}

interface TotalPanelProps {
  total: Total
  currencies: string[]
  currency: string
  onCurrencyChange: (currency: string) => void
}

/**
 * What everything adds up to, in one currency - and what it leaves out.
 *
 * A total that silently drops the accounts it had no rate for looks like an
 * answer, so the currencies left out are named under it, and so are the ones
 * added at a rate old enough to doubt.
 */
function TotalPanel({ total, currencies, currency, onCurrencyChange }: TotalPanelProps) {
  const { t } = useTranslation()
  const held = total.amount !== total.available
  const choices = Object.fromEntries(currencies.map((code) => [code, code]))
  return (
    <Panel className="flex flex-wrap items-end justify-between gap-x-8 gap-y-3 p-4">
      <div className="flex flex-col gap-1">
        <span className="caption">{t('accounts.total')}</span>
        <Money amount={total.amount} currency={total.currency} className="text-2xl font-semibold text-text" />
        {held ? (
          <span className="text-sm text-dim">
            {t('accounts.totalAvailable')} <Money amount={total.available} currency={total.currency} />
          </span>
        ) : null}
        {total.unconverted?.length ? (
          <span className="text-sm text-warn">{t('accounts.unconverted', { currencies: total.unconverted.join(', ') })}</span>
        ) : null}
        {total.stale?.length ? (
          <span className="text-sm text-dim">{t('accounts.stale', { currencies: total.stale.join(', ') })}</span>
        ) : null}
      </div>
      {currencies.length > 1 ? (
        <Select items={choices} value={currency} onValueChange={(value) => onCurrencyChange(value as string)}>
          <SelectTrigger aria-label={t('accounts.totalIn')} className="w-28" size="sm">
            <SelectValue />
          </SelectTrigger>
          <SelectPopup>
            {currencies.map((code) => (
              <SelectItem key={code} value={code}>
                {code}
              </SelectItem>
            ))}
          </SelectPopup>
        </Select>
      ) : null}
    </Panel>
  )
}

interface AccountRowProps {
  account: Account
  balance: Balance | undefined
  totalCurrency: string | undefined
  showAvailable: boolean
}

function AccountRow({ account, balance, totalCurrency, showAvailable }: AccountRowProps) {
  const { t } = useTranslation()
  const sameCurrency = account.currency === totalCurrency
  return (
    <TableRow>
      <TableCell>
        {/* A link rather than a clickable row: it is reachable by Tab, opens
            in a new tab, and says where it goes. */}
        <Link to={`/entries?account=${account.id}`} className="font-medium text-text hover:text-accent">
          {account.name}
        </Link>
        <div className="text-xs text-dim">{t(`accountKind.${account.kind}`)}</div>
      </TableCell>
      <TableCell numeric>{balance ? <Money amount={balance.amount} currency={account.currency} /> : null}</TableCell>
      {showAvailable ? (
        <TableCell numeric className="text-dim">
          {balance && balance.available !== balance.amount ? (
            <Money amount={balance.available} currency={account.currency} />
          ) : null}
        </TableCell>
      ) : null}
      {totalCurrency ? (
        <TableCell numeric className="text-dim">
          {balance && !sameCurrency ? (
            balance.converted ? (
              <span title={balance.rate_used?.stale ? t('accounts.staleRate', { date: balance.rate_used.on_date }) : undefined}>
                <Money amount={balance.converted} currency={totalCurrency} />
                {balance.rate_used?.stale ? <span className="text-warn"> *</span> : null}
              </span>
            ) : (
              <span className="text-warn">{t('accounts.noRate')}</span>
            )
          ) : null}
        </TableCell>
      ) : null}
      <TableCell className="w-10">
        <AccountActions account={account} />
      </TableCell>
    </TableRow>
  )
}

/** Closing and reopening: an account is never deleted, because its entries
 * are still true - closing takes it out of the lists of where money can go. */
function useSetClosed() {
  const { t } = useTranslation()
  const changed = useBooksChanged()
  const toasts = useToastManager()
  return useMutation({
    mutationFn: ({ account, closed }: { account: Account; closed: boolean }) => api.setClosed(account.id, closed),
    onSuccess: (account) => {
      void changed()
      toasts.add({
        type: 'success',
        title: t(account.closed_at ? 'accounts.closedToast' : 'accounts.reopenedToast', { name: account.name }),
      })
    },
    onError: (error: Error) => toasts.add({ type: 'error', title: error.message }),
  })
}

function AccountActions({ account }: { account: Account }) {
  const { t } = useTranslation()
  const setClosed = useSetClosed()
  return (
    <Menu>
      <MenuTrigger render={<Button variant="icon" size="icon-sm" aria-label={t('accounts.actionsFor', { name: account.name })} />}>
        <MoreHorizontal aria-hidden />
      </MenuTrigger>
      <MenuPopup align="end">
        <MenuItem render={<Link to={`/entries?account=${account.id}`} />}>{t('accounts.showEntries')}</MenuItem>
        <MenuItem onClick={() => setClosed.mutate({ account, closed: true })}>{t('accounts.close')}</MenuItem>
      </MenuPopup>
    </Menu>
  )
}

function ClosedAccounts({ accounts }: { accounts: Account[] }) {
  const { t } = useTranslation()
  const setClosed = useSetClosed()
  return (
    <section className="flex flex-col gap-2">
      <h2 className="caption">{t('accounts.closedTitle')}</h2>
      <Panel className="divide-y divide-line">
        {accounts.map((account) => (
          <div key={account.id} className="flex items-center justify-between gap-3 px-3 py-2">
            <div className="flex min-w-0 items-center gap-2">
              <Link to={`/entries?account=${account.id}`} className="truncate text-dim hover:text-accent">
                {account.name}
              </Link>
              <Badge variant="outline">{account.currency}</Badge>
            </div>
            <Button size="sm" onClick={() => setClosed.mutate({ account, closed: false })}>
              {t('accounts.reopen')}
            </Button>
          </div>
        ))}
      </Panel>
    </section>
  )
}
