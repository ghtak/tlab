import { createRootRouteWithContext, Outlet, useRouter } from '@tanstack/react-router'
import { Button } from '@/components/ui/button'
import { ThemeSelect } from '@/components/theme-select'
import type { Session } from '@/feature/auth/services/session'
import { errorMessage } from '@/lib/api'

export const Route = createRootRouteWithContext<{ session: Session }>()({
  component: () => <div className="flex min-h-dvh flex-col"><div className="flex justify-end border-b border-border bg-background px-6 py-3"><ThemeSelect /></div><Outlet /></div>,
  pendingComponent: () => <main className="p-8" role="status">세션 확인 중…</main>,
  errorComponent: ({ error, reset }) => {
    const router = useRouter()
    return <main className="p-8 space-y-4"><h1 className="text-xl font-semibold">세션을 확인하지 못했습니다</h1><p role="alert">{errorMessage(error)}</p><Button onClick={() => { reset(); void router.invalidate() }}>다시 시도</Button></main>
  },
})
