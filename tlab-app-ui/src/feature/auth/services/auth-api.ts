import type { ApiClient } from '@/lib/api'

export type SessionUser = {
  sub: string
  sid: string
  user: { nickname: string; status: string; role_ids: number[] }
}
export type LoginInput = { email: string; password: string }
export function createAuthApi(api: ApiClient) {
  return {
    me: async (signal?: AbortSignal) => (await api('auth/me', { signal, protected: false })).json() as Promise<SessionUser>,
    login: async (json: LoginInput) => { await api('auth/local', { method: 'post', json, protected: false }) },
    logout: async () => { await api('auth/logout', { method: 'post', protected: false }) },
  }
}
export type AuthApi = ReturnType<typeof createAuthApi>
