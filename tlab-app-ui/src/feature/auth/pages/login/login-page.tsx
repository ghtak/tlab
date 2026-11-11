import { useState, type FormEvent } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { errorMessage } from '@/lib/api'
import { SessionConfirmationError, type Session } from '../../services/session'

export function LoginPage({
  session,
  onAuthenticated,
}: {
  session: Session
  onAuthenticated: () => Promise<void>
}) {
  // Keep focus/reconnect checks active on the public login screen as well.
  useQuery(session.options())
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [confirming, setConfirming] = useState(false)
  const login = useMutation({
    mutationFn: async () => {
      const input = { email, password }
      setPassword('')
      await session.login(input)
      await onAuthenticated()
    },
    retry: false,
    onError: (error) => {
      // A login may have succeeded while its confirmation failed. Retry only /me.
      setConfirming(error instanceof SessionConfirmationError)
    },
  })
  const retryConfirmation = useMutation({
    mutationFn: async () => {
      const user = await session.retry()
      if (user) await onAuthenticated()
      else {
        setConfirming(false)
        login.reset()
      }
    },
    retry: false,
  })
  function submit(event: FormEvent) {
    event.preventDefault()
    if (!login.isPending) login.mutate()
  }
  const pending = login.isPending || retryConfirmation.isPending
  const inputClass =
    'h-11 w-full rounded-md border border-input bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50'
  return (
    <main className="flex flex-1 items-center justify-center bg-muted p-6 text-foreground">
      <section
        className="w-full max-w-sm rounded-xl border border-border bg-card p-8 text-card-foreground shadow-sm"
        aria-labelledby="login-title"
      >
        <p className="mb-7 text-sm font-semibold tracking-widest">TLAB</p>
        <h1 id="login-title" className="text-2xl font-semibold tracking-tight">
          로그인
        </h1>
        <p className="mt-2 mb-7 text-sm text-muted-foreground">계정으로 로그인해 주세요.</p>
        <form onSubmit={submit} className="space-y-5" aria-busy={pending}>
          <div className="space-y-2">
            <label htmlFor="email" className="text-sm font-medium">
              이메일
            </label>
            <input
              id="email"
              name="email"
              type="email"
              autoComplete="username"
              required
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              disabled={pending || confirming}
              className={inputClass}
            />
          </div>
          <div className="space-y-2">
            <label htmlFor="password" className="text-sm font-medium">
              비밀번호
            </label>
            <input
              id="password"
              name="password"
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              disabled={pending || confirming}
              className={inputClass}
            />
          </div>
          {(login.error || retryConfirmation.error) && (
            <p role="alert" className="text-sm text-destructive">
              {errorMessage(
                retryConfirmation.error ??
                  (login.error instanceof SessionConfirmationError
                    ? login.error.cause
                    : login.error),
              )}
            </p>
          )}
          {confirming ? (
            <div className="space-y-3">
              <Button
                type="button"
                className="w-full"
                disabled={pending}
                onClick={() => retryConfirmation.mutate()}
              >
                {pending ? '확인 중…' : '세션 다시 확인'}
              </Button>
              <Button
                type="button"
                variant="ghost"
                className="w-full"
                disabled={pending}
                onClick={() => {
                  setConfirming(false)
                  login.reset()
                  retryConfirmation.reset()
                }}
              >
                다시 입력
              </Button>
            </div>
          ) : (
            <Button type="submit" className="w-full" disabled={pending}>
              {pending ? '로그인 확인 중…' : '로그인'}
            </Button>
          )}
        </form>
      </section>
    </main>
  )
}
