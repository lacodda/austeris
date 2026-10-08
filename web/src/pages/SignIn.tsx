import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Panel } from '@/components/ui/panel'
import { PasswordField } from '@/components/ui/password-field'
import { LanguageSwitch } from '@/components/LanguageSwitch'
import { ApiError } from '@/lib/api'
import { useSession } from '@/lib/session'

/**
 * iOS Safari zooms the page into any field whose text is under 16px and does
 * not zoom back out, leaving the form at 130% with the password off the side
 * of the screen. dowel's Input is `text-sm`, right on a desktop, so the size is
 * lifted here and handed back from `sm` up - on this screen, not in the copy
 * of the primitive, which stays dowel's.
 *
 * `text-lg`, not `text-base`: on dowel's scale `base` is 14px and `lg` is the
 * 16px step.
 */
const FIELD_ON_A_PHONE = 'text-lg sm:text-sm'

export function SignIn() {
  const { t } = useTranslation()
  const { signIn } = useSession()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [failure, setFailure] = useState<string | null>(null)
  const [pending, setPending] = useState(false)

  async function submit(event: FormEvent) {
    event.preventDefault()
    setFailure(null)
    setPending(true)
    try {
      await signIn(email, password)
    } catch (error: unknown) {
      // The server refuses an unknown address and a wrong password with the
      // same answer on purpose, so the form does not guess which it was. Too
      // many tries is its own answer, and the server says how long to wait.
      // Anything else is the server out of reach, and saying so is more use
      // than "wrong password" to someone whose password is right.
      if (error instanceof ApiError && error.isUnauthorized) setFailure(t('signIn.wrong'))
      else if (error instanceof ApiError && error.status === 429) setFailure(error.message)
      else setFailure(t('signIn.unreachable'))
      setPending(false)
    }
  }

  return (
    <main className="flex min-h-full flex-col items-center justify-center gap-4 p-4">
      <Panel className="w-full max-w-sm p-6">
        <div className="flex items-center gap-3">
          <img src="/favicon.svg" alt="" className="size-8" />
          <h1 className="text-lg font-semibold text-text">{t('signIn.title')}</h1>
        </div>
        <form onSubmit={(event) => void submit(event)} className="mt-6 flex flex-col gap-4">
          <Field label={t('signIn.email')} required>
            <Input
              className={FIELD_ON_A_PHONE}
              type="email"
              autoComplete="username"
              required
              value={email}
              onChange={(event) => setEmail(event.target.value)}
            />
          </Field>
          <Field label={t('signIn.password')} required>
            <PasswordField
              className={FIELD_ON_A_PHONE}
              autoComplete="current-password"
              required
              value={password}
              onValueChange={setPassword}
              showLabel={t('signIn.show')}
              hideLabel={t('signIn.hide')}
            />
          </Field>
          {/* `role="alert"`, so the failure is announced and not only drawn. */}
          {failure ? (
            <Alert tone="bad" role="alert">
              {failure}
            </Alert>
          ) : null}
          <Button type="submit" variant="primary" disabled={pending}>
            {pending ? t('signIn.pending') : t('signIn.submit')}
          </Button>
        </form>
      </Panel>
      <LanguageSwitch />
    </main>
  )
}
