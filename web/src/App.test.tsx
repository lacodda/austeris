import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { App } from '@/App'
import { app, failure, json, serve, signedIn } from '@/test/server'

const BOOKS = {
  '/api/v1/ledger/accounts': json([]),
  '/api/v1/ledger/balances': json({ as_of: '2026-10-08', accounts: [] }),
}

describe('App', () => {
  it('asks someone signed out to sign in, and nothing else', async () => {
    serve({ '/api/v1/auth/me': failure(401, 'not signed in') })
    render(app(<App />))
    expect(await screen.findByRole('button', { name: 'Sign in' })).toBeTruthy()
    expect(screen.queryByRole('navigation')).toBeNull()
  })

  it('opens on the accounts of someone signed in', async () => {
    signedIn(BOOKS)
    render(app(<App />))
    expect(await screen.findByRole('heading', { name: 'Accounts' })).toBeTruthy()
    expect(screen.getByText('Ada')).toBeTruthy()
  })

  it('sends an address nothing answers to the accounts', async () => {
    signedIn(BOOKS)
    render(app(<App />, '/nowhere'))
    expect(await screen.findByRole('heading', { name: 'Accounts' })).toBeTruthy()
  })

  it('says a wrong password without saying which half was wrong', async () => {
    serve({
      '/api/v1/auth/me': failure(401, 'not signed in'),
      'POST /api/v1/auth/login': failure(401, 'wrong email or password'),
    })
    render(app(<App />))
    await userEvent.type(await screen.findByLabelText(/Email/), 'ada@example.com')
    await userEvent.type(screen.getByLabelText(/Password/, { selector: 'input' }), 'nope')
    await userEvent.click(screen.getByRole('button', { name: 'Sign in' }))
    expect((await screen.findByRole('alert')).textContent).toBe('That email and password do not match.')
  })

  it('signs in and shows the books', async () => {
    const requests = serve({
      '/api/v1/auth/me': failure(401, 'not signed in'),
      'POST /api/v1/auth/login': () => {
        // From now on the cookie is good: `me` answers with the person.
        serve({ '/api/v1/auth/me': json({ id: 'u1', email: 'ada@example.com', display_name: 'Ada' }), ...BOOKS })
        return new Response(null, { status: 200 })
      },
    })
    render(app(<App />))
    await userEvent.type(await screen.findByLabelText(/Email/), 'ada@example.com')
    await userEvent.type(screen.getByLabelText(/Password/, { selector: 'input' }), 'secret')
    await userEvent.click(screen.getByRole('button', { name: 'Sign in' }))
    expect(await screen.findByRole('heading', { name: 'Accounts' })).toBeTruthy()
    expect(requests.find((sent) => sent.method === 'POST')?.body).toEqual({ email: 'ada@example.com', password: 'secret' })
  })
})
