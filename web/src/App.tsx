import { NavLink, Navigate, Route, Routes, useLocation } from 'react-router'
import { useTranslation } from 'react-i18next'
import { ArrowLeftRight, ChevronDown, Wallet } from 'lucide-react'
import { AppShell, Screen } from '@/components/ui/app-shell'
import { Button } from '@/components/ui/button'
import { Menu, MenuGroup, MenuGroupLabel, MenuItem, MenuPopup, MenuSeparator, MenuTrigger } from '@/components/ui/menu'
import { NavRail, type NavRailItem } from '@/components/ui/nav-rail'
import { languageName, LANGUAGES, setLanguage } from '@/i18n'
import { useSession } from '@/lib/session'
import { Accounts } from '@/pages/Accounts'
import { Entries } from '@/pages/Entries'
import { SignIn } from '@/pages/SignIn'

/** The screens, in the order the navigation lists them. */
const SCREENS = [
  { id: 'accounts', path: '/accounts', icon: <Wallet aria-hidden /> },
  { id: 'entries', path: '/entries', icon: <ArrowLeftRight aria-hidden /> },
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
  const items: NavRailItem[] = SCREENS.map((screen) => ({ id: screen.id, label: t(`nav.${screen.id}`), icon: screen.icon }))
  const active = SCREENS.find((screen) => pathname.startsWith(screen.path))?.id
  const link = (item: NavRailItem) => <NavLink to={SCREENS.find((screen) => screen.id === item.id)?.path ?? '/'} />

  return (
    <AppShell top={<Header items={items} active={active} link={link} />}>
      <Screen>
        <Routes>
          <Route path="/accounts" element={<Accounts />} />
          <Route path="/entries" element={<Entries />} />
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
