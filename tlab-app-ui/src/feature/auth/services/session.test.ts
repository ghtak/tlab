import { describe, expect, it, vi } from 'vitest'
import { QueryClient } from '@tanstack/react-query'
import { ApiError, createApiClient } from '../../../lib/api'
import { createAuthApi } from './auth-api'
import type { AuthApi, SessionUser } from './auth-api'
import { createSession, sessionKey, SessionConfirmationError } from './session'

const user: SessionUser = { sub: '1', sid: 'session', user: { nickname: 'Tester', status: 'ACTIVE', role_ids: [] } }
function setup(overrides: Partial<AuthApi> = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  const auth: AuthApi = { me: vi.fn(async () => user), login: vi.fn(async () => {}), logout: vi.fn(async () => {}), ...overrides }
  const changed = vi.fn()
  return { client, auth, changed, session: createSession(client, auth, changed) }
}
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(done => { resolve = done })
  return { promise, resolve }
}

describe('session contracts', () => {
  it('confirms login with me and clears prior user caches', async () => {
    const { session, client, auth } = setup()
    client.setQueryData(['user', 'old', 'private'], { secret: true })
    await session.login({ email: 'test@example.test', password: 'temporary' })
    expect(auth.login).toHaveBeenCalledOnce()
    expect(client.getQueryData(sessionKey)).toEqual(user)
    expect(client.getQueryData(['user', 'old', 'private'])).toBeUndefined()
  })
  it('keeps failed confirmation distinct and retries only me', async () => {
    const me = vi.fn().mockRejectedValueOnce(new ApiError(503)).mockResolvedValue(user)
    const { session, auth } = setup({ me })
    await expect(session.login({ email: 'test@example.test', password: 'temporary' })).rejects.toBeInstanceOf(SessionConfirmationError)
    await expect(session.retry()).resolves.toEqual(user)
    expect(auth.login).toHaveBeenCalledOnce()
  })
  it('distinguishes unauthenticated from initial server or network failure', async () => {
    for (const error of [new ApiError(503), new TypeError('offline'), new ApiError(403, 'CSRF_REJECTED')]) {
      const { session, client } = setup({ me: async () => { throw error } })
      await expect(session.ensure()).rejects.toBe(error)
      expect(client.getQueryData(sessionKey)).toBeUndefined()
    }
    const { session } = setup({ me: async () => { throw new ApiError(401, 'INVALID_ACCESS_TOKEN') } })
    await expect(session.ensure()).resolves.toBeNull()
  })
  it('preserves authenticated state on logout failure and clears it only on success', async () => {
    const logout = vi.fn().mockRejectedValueOnce(new ApiError(503)).mockResolvedValue(undefined)
    const { session, client } = setup({ logout })
    await session.ensure()
    client.setQueryData(['user', user.sub, 'private'], 'data')
    await expect(session.logout()).rejects.toBeInstanceOf(ApiError)
    expect(client.getQueryData(sessionKey)).toEqual(user)
    await session.logout()
    expect(client.getQueryData(sessionKey)).toBeNull()
    expect(client.getQueryData(['user', user.sub, 'private'])).toBeUndefined()
  })
  it('aborts old session lookups and ignores a late old-generation 401 after login', async () => {
    const old = deferred<SessionUser>()
    let signal: AbortSignal | undefined
    const me = vi.fn().mockImplementationOnce((value: AbortSignal) => { signal = value; return old.promise }).mockResolvedValue(user)
    const { session, client } = setup({ me })
    const oldGeneration = session.generation()
    const initial = session.ensure().catch(() => undefined)
    await session.login({ email: 'test@example.test', password: 'temporary' })
    expect(signal?.aborted).toBe(true)
    old.resolve({ ...user, sub: 'old' })
    await initial
    session.unauthorized(oldGeneration)
    expect(client.getQueryData(sessionKey)).toEqual(user)
    session.unauthorized(session.generation())
    expect(client.getQueryData(sessionKey)).toBeNull()
  })
  it('rejects overlapping session transitions', async () => {
    const request = deferred<void>()
    const { session } = setup({ login: () => request.promise })
    const login = session.login({ email: 'test@example.test', password: 'temporary' })
    await expect(session.logout()).rejects.toThrow('already running')
    request.resolve()
    await login
  })
})

describe('HTTP boundary', () => {
  it('rejects late protected successes after the session changed', async () => {
    const response = deferred<Response>()
    const fetch = vi.fn(() => response.promise)
    vi.stubGlobal('fetch', fetch)
    let generation = 0
    const api = createApiClient({ baseUrl: 'http://localhost/web/v1/', credentials: 'same-origin', generation: () => generation, onUnauthorized: vi.fn() })
    const request = api('private')
    await vi.waitFor(() => expect(fetch).toHaveBeenCalledOnce())
    generation++
    response.resolve(new Response('{}'))
    await expect(request).rejects.toMatchObject({ name: 'AbortError' })
    vi.unstubAllGlobals()
  })

  it('retains non-JSON status, never retries, and distinguishes public and protected errors', async () => {
    const fetch = vi.fn().mockImplementation(async () => new Response(JSON.stringify({ code: 'INVALID_ACCESS_TOKEN' }), { status: 401 }))
    vi.stubGlobal('fetch', fetch)
    const unauthorized = vi.fn()
    const api = createApiClient({ baseUrl: 'http://localhost/web/v1/', credentials: 'same-origin', generation: () => 7, onUnauthorized: unauthorized })
    await expect(createAuthApi(api).login({ email: 'test@example.test', password: 'temporary' })).rejects.toMatchObject({ status: 401 })
    expect(unauthorized).not.toHaveBeenCalled()
    await expect(api('protected')).rejects.toMatchObject({ status: 401 })
    expect(unauthorized).toHaveBeenCalledWith(7)
    expect(fetch).toHaveBeenCalledTimes(2)
    fetch.mockImplementation(async () => new Response('proxy unavailable', { status: 502 }))
    await expect(api('protected')).rejects.toMatchObject({ status: 502 })
    vi.unstubAllGlobals()
  })
})
