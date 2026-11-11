import { queryOptions, type QueryClient } from '@tanstack/react-query'
import { ApiError } from '@/lib/api'
import type { AuthApi, LoginInput, SessionUser } from './auth-api'

export class SessionConfirmationError extends Error {
  constructor(public cause: unknown) {
    super('Login confirmation failed')
  }
}

export const sessionKey = ['auth', 'session'] as const
export function createSession(queryClient: QueryClient, auth: AuthApi, changed: () => void) {
  let generation = 0
  let transitioning = false
  const userQueries = {
    predicate: (query: { queryKey: readonly unknown[] }) => query.queryKey[0] === 'user',
  }
  const clearUser = async () => {
    const started = generation
    await queryClient.cancelQueries(userQueries)
    if (started === generation) queryClient.removeQueries(userQueries)
  }
  const options = () =>
    queryOptions({
      queryKey: sessionKey,
      staleTime: Infinity,
      retry: false,
      refetchOnWindowFocus: () => (transitioning ? false : ('always' as const)),
      refetchOnReconnect: () => (transitioning ? false : ('always' as const)),
      queryFn: async ({ signal }): Promise<SessionUser | null> => {
        const started = generation
        try {
          const user = await auth.me(signal)
          if (started !== generation || signal.aborted)
            throw new DOMException('Session changed', 'AbortError')
          const previous = queryClient.getQueryData<SessionUser | null>(sessionKey)
          if (previous && previous.sub !== user.sub) {
            generation++
            const replacement = generation
            await clearUser()
            if (replacement !== generation || signal.aborted)
              throw new DOMException('Session changed', 'AbortError')
          }
          return user
        } catch (error) {
          if (started !== generation || signal.aborted)
            throw new DOMException('Session changed', 'AbortError')
          if (
            error instanceof ApiError &&
            error.status === 401 &&
            error.code === 'INVALID_ACCESS_TOKEN'
          ) {
            generation++
            const ended = generation
            await clearUser()
            if (ended !== generation || signal.aborted)
              throw new DOMException('Session changed', 'AbortError')
            return null
          }
          throw error
        }
      },
    })
  const begin = async () => {
    if (transitioning) throw new Error('Session transition already running')
    transitioning = true
    generation++
    await queryClient.cancelQueries({ queryKey: sessionKey })
    await queryClient.cancelQueries(userQueries)
  }
  return {
    options,
    generation: () => generation,
    ensure: () => queryClient.fetchQuery(options()),
    retry: () => queryClient.fetchQuery({ ...options(), staleTime: 0 }),
    async login(input: LoginInput) {
      await begin()
      try {
        await auth.login(input)
        await clearUser()
        try {
          const user = await queryClient.fetchQuery({ ...options(), staleTime: 0 })
          if (!user) throw new ApiError(401, 'INVALID_ACCESS_TOKEN')
          changed()
          return
        } catch (error) {
          throw new SessionConfirmationError(error)
        }
      } finally {
        transitioning = false
      }
    },
    async logout() {
      await begin()
      try {
        await auth.logout()
        await clearUser()
        queryClient.setQueryData(sessionKey, null)
      } catch (error) {
        // Response loss can mean the server already ended the session.
        try {
          await queryClient.fetchQuery({ ...options(), staleTime: 0 })
        } catch {
          /* Keep the original failure. */
        }
        throw error
      } finally {
        transitioning = false
      }
    },
    unauthorized(started: number) {
      if (started !== generation || transitioning || queryClient.getQueryData(sessionKey) === null)
        return
      generation++
      void queryClient.cancelQueries({ queryKey: sessionKey })
      void clearUser()
      queryClient.setQueryData(sessionKey, null)
      changed()
    },
  }
}
export type Session = ReturnType<typeof createSession>
