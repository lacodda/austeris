import { NavLink, Navigate, Route, Routes, useLocation, useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'
import { ArrowLeftRight, ChevronDown, Coins, FileInput, PenLine, Wallet } from 'lucide-react'
import { AppShell, Screen } from '@/components/ui/app-shell'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useCommands } from '@/components/ui/commands'
import { Menu, MenuGroup, MenuGroupLabel, MenuItem, MenuPopup, MenuSeparator, MenuTrigger } from '@/components/ui/menu'
import { NavRail, type NavRailItem } from '@/components/ui/nav-rail'
import { ShortcutsDialog } from '@/components/ui/shortcuts-dialog'
import { languageName, LANGUAGES, setLanguage } from '@/i18n'
import { useModules } from '@/lib/imports'
import { useUnsortedCount } from '@/lib/ledger'
import { useSession } from '@/lib/session'
import { Accounts } from '@/pages/Accounts'
import { Entries } from '@/pages/Entries'
import { Import } from '@/pages/Import'
import { Portfolio } from '@/pages/Portfolio'
import { Record } from '@/pages/Record'
import { SignIn } from '@/pages/SignIn'
import { Trash } from '@/pages/Trash'

/** The screens, in the order the navigation lists them, with the letter
 * that goes to each after G. */
const SCREENS = [
  { id: 'record', path: '/record', icon: <PenLine aria-hidden />, key: 'R' },
  { id: 'accounts', path: '/accounts', icon: <Wallet aria-hidden />, key: 'A' },
  { id: 'entries', path: '/entries', icon: <ArrowLeftRight aria-hidden />, key: 'E' },
  // A module's screen, shown only while the installation runs it.
  { id: 'portfolio', path: '/portfolio', icon: <Coins aria-hidden />, key: 'P', module: 'portfolio' },
  { id: 'import', path: '/import', icon: <FileInput aria-hidden />, key: 'I', module: 'imports' },
] as const

/** Which screen is showing. */
export function App() {
  const { user } = useSession()
  // The session is asked of the server on every load; until it answers there
  // is nothing true to show - not the sign-in form, not the books.
  if (user === undefined) return null
  if (user === null) return <SignIn />
  return <Books />
}

/** Everything someone signed in sees: the frame, and the screen in it. */
function Books() {
  const { t } = useTranslation()
  const { pathname } = useLocation()
  const modules = useModules()
  const unsorted = useUnsortedCount()
  const shown = SCREENS.filter((screen) => !('module' in screen) || modules?.has(screen.module))
  // The drafts wait on the entries: how many is said where the screen is
  // named, in the header and in the phone's bar alike.
  const drafts = unsorted.data ?? 0
  const items: NavRailItem[] = shown.map((screen) => ({
    id: screen.id,
    label:
      screen.id === 'entries' && drafts > 0 ? (
        <span className="inline-flex items-center gap-1.5">
          {t('nav.entries')}
          <Badge variant="accent" aria-label={t('entries.unsortedCount', { count: drafts })}>
            {drafts}
          </Badge>
        </span>
      ) : (
        t(`nav.${screen.id}`)
      ),
    icon: screen.icon,
  }))
  const active = shown.find((screen) => pathname.startsWith(screen.path))?.id
  const link = (item: NavRailItem) => <NavLink to={shown.find((screen) => screen.id === item.id)?.path ?? '/'} />
  const navigate = useNavigate()
  // Declared here, before any screen's, so the frame's keys outrank a
  // screen's should two ever ask for the same.
  useCommands(
    shown.map((screen) => ({
      id: `go.${screen.id}`,
      label: t(`nav.${screen.id}`),
      group: t('keys.go'),
      keys: `G ${screen.key}`,
      run: () => void navigate(screen.path),
    })),
  )

  return (
    <AppShell top={<Header items={items} active={active} link={link} />}>
      {/* Sixteen pixels of margin on a phone rather than twenty-four: on a
          screen 390 wide, the eight given back are the amount's. */}
      <Screen className="px-4 sm:px-6">
        <Routes>
          <Route path="/record" element={<Record />} />
          <Route path="/accounts" element={<Accounts />} />
          <Route path="/entries" element={<Entries />} />
          {/* Under the entries, so the navigation keeps them lit: the
              trash is where an entry goes, not a place of its own. */}
          <Route path="/entries/trash" element={<Trash />} />
          {modules && !modules.has('imports') ? null : <Route path="/import" element={<Import />} />}
          {modules && !modules.has('portfolio') ? null : <Route path="/portfolio/*" element={<Portfolio />} />}
          {/* An address nothing answers lands on the money rather than on a
              blank page. */}
          <Route path="*" element={<Navigate to="/accounts" replace />} />
        </Routes>
      </Screen>
      {/* The phone's navigation: the same destinations along the bottom,
          where a thumb already is. Gone from `sm` up, where they sit in the
          header. */}
      <NavRail
        layout="bar"
        label={t('nav.label')}
        items={items}
        activeId={active}
        render={link}
        className="sm:hidden"
      />
      <ShortcutsDialog title={t('keys.title')} closeLabel={t('common.close')} group={t('keys.general')} description={t('keys.description')} />
    </AppShell>
  )
}

interface HeaderProps {
  items: NavRailItem[]
  active: string | undefined
  link: (item: NavRailItem) => React.ReactElement
}

function Header({ items, active, link }: HeaderProps) {
  const { t } = useTranslation()
  return (
    <header className="flex h-full items-center justify-between gap-3 border-b border-line px-4 sm:px-6">
      <div className="flex min-w-0 items-center gap-4">
        <span className="flex shrink-0 items-center gap-2">
          <img src="/favicon.svg" alt="" className="size-6" />
          <span className="font-mono text-sm font-semibold text-accent">austeris</span>
        </span>
        <NavRail
          layout="row"
          label={t('nav.label')}
          items={items}
          activeId={active}
          render={link}
          className="hidden sm:flex"
        />
      </div>
      <PersonMenu />
    </header>
  )
}

/** Whose books these are, the language, and the way out. */
function PersonMenu() {
  const { t, i18n } = useTranslation()
  const { user, signOut } = useSession()
  return (
    <Menu>
      <MenuTrigger render={<Button variant="icon" size="sm" className="min-w-0 gap-1" />}>
        <span className="truncate">{user?.display_name}</span>
        <ChevronDown aria-hidden />
      </MenuTrigger>
      <MenuPopup align="end">
        <MenuGroup>
          <MenuGroupLabel>{t('language.label')}</MenuGroupLabel>
          {LANGUAGES.map((language) => (
            <MenuItem
              key={language}
              onClick={() => setLanguage(language)}
              aria-current={i18n.language === language ? 'true' : undefined}
              className="aria-[current=true]:font-semibold"
            >
              <span lang={language}>{languageName(language)}</span>
            </MenuItem>
          ))}
        </MenuGroup>
        <MenuSeparator />
        <MenuItem onClick={() => void signOut()}>{t('nav.signOut')}</MenuItem>
        <MenuSeparator />
        {/* The server's version is this bundle's: the UI ships inside the
            binary, and release_consistency holds web/package.json to it. */}
        <div className="px-2 py-1 font-mono text-xs text-faint">austeris {__APP_VERSION__}</div>
      </MenuPopup>
    </Menu>
  )
}
