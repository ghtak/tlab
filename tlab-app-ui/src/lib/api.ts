import ky, { type Options } from 'ky'

export class ApiError extends Error {
  constructor(
    public status: number,
    public code?: string,
  ) {
    super(code ?? `HTTP ${status}`)
    this.name = 'ApiError'
  }
}

type RequestOptions = Options & { protected?: boolean }
export function createApiClient(config: {
  baseUrl: string
  credentials: RequestCredentials
  generation: () => number
  onUnauthorized: (generation: number) => void
}) {
  const http = ky.create({
    baseUrl: config.baseUrl,
    credentials: config.credentials,
    retry: 0,
    timeout: 15000,
    throwHttpErrors: false,
  })
  return async (path: string, { protected: guarded = true, ...options }: RequestOptions = {}) => {
    const generation = config.generation()
    const response = await http(path, options)
    if (!response.ok) {
      let code: string | undefined
      try {
        const body: unknown = await response.json()
        if (body && typeof body === 'object' && 'code' in body && typeof body.code === 'string')
          code = body.code
      } catch {
        /* A non-JSON proxy error still retains its HTTP status. */
      }
      if (guarded && response.status === 401 && code === 'INVALID_ACCESS_TOKEN')
        config.onUnauthorized(generation)
      throw new ApiError(response.status, code)
    }
    if (guarded && generation !== config.generation())
      throw new DOMException('Session changed', 'AbortError')
    return response
  }
}
export type ApiClient = ReturnType<typeof createApiClient>

export function errorMessage(error: unknown): string {
  if (error instanceof ApiError) {
    switch (error.code) {
      case 'INVALID_CREDENTIALS':
        return '이메일 또는 비밀번호가 올바르지 않습니다.'
      case 'ACCOUNT_UNAVAILABLE':
        return '로그인할 수 없는 계정입니다.'
      case 'CSRF_REJECTED':
        return '요청 환경을 확인할 수 없습니다. 접속 주소를 확인해 주세요.'
      case 'AUTH_COOKIE_TOO_LARGE':
        return '인증 정보를 처리할 수 없습니다. 관리자에게 문의해 주세요.'
      case 'INVALID_ACCESS_TOKEN':
        return '세션이 만료되었습니다. 다시 로그인해 주세요.'
    }
    if (error.status >= 500) return '서버에 문제가 발생했습니다. 잠시 후 다시 시도해 주세요.'
    if (error.status === 403) return '이 작업을 수행할 권한이 없습니다.'
    if (error.status === 400 || error.status === 422) return '입력한 내용을 확인해 주세요.'
  }
  return '요청을 완료하지 못했습니다. 연결을 확인하고 다시 시도해 주세요.'
}
