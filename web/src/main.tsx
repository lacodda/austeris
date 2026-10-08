import { StrictMode, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClientProvider } from '@tanstack/react-query'
import { BrowserRouter } from 'react-router'
import '@/i18n'
import { App } from '@/App'
import { Toasts } from '@/components/Toasts'
import { ToastProvider } from '@/components/ui/toast'
import { queryClient } from '@/lib/query'
import { SessionProvider } from '@/lib/session'
import '@/styles.css'

const container = document.getElementById('root')
if (!container) {
  throw new Error('The #root element is missing from index.html')
}

/*
 * What the application is wrapped in, outermost first. A provider is one line
 * here, so adding or removing one never re-indents the others.
 */
const wrappers: ((children: ReactNode) => ReactNode)[] = [
  (children) => <StrictMode>{children}</StrictMode>,
  (children) => <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>,
  (children) => <BrowserRouter>{children}</BrowserRouter>,
  (children) => <SessionProvider>{children}</SessionProvider>,
  (children) => (
    <ToastProvider>
      {children}
      <Toasts />
    </ToastProvider>
  ),
]

createRoot(container).render(wrappers.reduceRight<ReactNode>((children, wrap) => wrap(children), <App />))
