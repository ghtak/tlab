import { useMutation, useQuery } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { errorMessage } from '@/lib/api'
import type { Session } from '@/feature/auth/services/session'

export function DashboardPage({ session }: { session: Session }) {
  const user = useQuery(session.options())
  const logout = useMutation({ mutationFn: session.logout, retry: false })
  return <div className="flex-1 bg-background text-foreground" aria-busy={logout.isPending}>
    <header className="border-b border-border"><div className="mx-auto flex max-w-6xl items-center justify-between gap-4 px-6 py-4"><span className="text-lg font-semibold tracking-tight">tlab</span><div className="flex items-center gap-4"><span className="text-sm text-muted-foreground">{user.data?.user.nickname}</span><Button variant="outline" disabled={logout.isPending} onClick={() => logout.mutate()}>{logout.isPending ? '로그아웃 중…' : '로그아웃'}</Button></div></div></header>
    <main className="mx-auto max-w-6xl px-6 py-10"><h1 className="text-2xl font-semibold tracking-tight">대시보드</h1>
      {logout.error && <p role="alert" className="mt-4 text-sm text-destructive">로그아웃을 완료하지 못했습니다. {errorMessage(logout.error)} 다시 시도해 주세요.</p>}
      {user.error && <div role="alert" className="mt-4 text-sm"><p>{errorMessage(user.error)}</p><Button variant="outline" className="mt-3" onClick={() => void user.refetch()}>세션 다시 확인</Button></div>}
    </main>
  </div>
}
