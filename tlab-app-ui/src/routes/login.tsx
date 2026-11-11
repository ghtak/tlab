import { createFileRoute, redirect, useRouter } from '@tanstack/react-router'
import { LoginPage } from '@/feature/auth/pages/login/login-page'

export const Route = createFileRoute('/login')({
  beforeLoad: async ({ context }) => {
    if (await context.session.ensure()) throw redirect({ to: '/', replace: true })
  },
  component: Login,
})
function Login() {
  const { session } = Route.useRouteContext()
  const router = useRouter()
  return <LoginPage session={session} onAuthenticated={async () => { await router.invalidate(); await router.navigate({ to: '/', replace: true }) }} />
}
