import React from 'react'
import ReactDOM from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { createRouter, RouterProvider } from '@tanstack/react-router'
import { routeTree } from './routeTree.gen'
import { createApiClient } from './lib/api'
import { createAuthApi } from './feature/auth/services/auth-api'
import { createSession } from './feature/auth/services/session'
import { sessionKey } from './feature/auth/services/session'
import { ThemeProvider } from './components/theme-provider'
import './index.css'

const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false, gcTime: 0 } } })
const api = createApiClient({
  baseUrl: new URL('/web/v1/', window.location.origin).href,
  credentials: 'same-origin',
  generation: () => session.generation(),
  onUnauthorized: generation => session.unauthorized(generation),
})
const session = createSession(queryClient, createAuthApi(api), () => { void router.invalidate() })
const router = createRouter({ routeTree, context: { session }, defaultPendingMs: 0 })
let previousSession = queryClient.getQueryData(sessionKey)
queryClient.getQueryCache().subscribe(event => {
  if (event.query.queryKey[0] !== 'auth') return
  const current = queryClient.getQueryData(sessionKey)
  if (current !== undefined && current !== previousSession) {
    previousSession = current
    void router.invalidate()
  }
})

declare module '@tanstack/react-router' {
  interface Register { router: typeof router }
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </ThemeProvider>
  </React.StrictMode>,
)
