import { createFileRoute } from '@tanstack/react-router'
import { Button } from '@/components/ui/button'

export const Route = createFileRoute('/')({ component: Home })

function Home() {
  return (
    <main className="flex min-h-screen items-center justify-center bg-background p-6 text-foreground">
      <div className="space-y-4 text-center">
        <h1 className="text-3xl font-semibold tracking-tight">tlab</h1>
        <p className="text-muted-foreground">tlab 앱 UI가 준비되었습니다.</p>
        <Button asChild><a href="https://ui.shadcn.com/docs/components" target="_blank" rel="noreferrer">컴포넌트 문서</a></Button>
      </div>
    </main>
  )
}
