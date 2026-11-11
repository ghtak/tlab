import { createFileRoute, redirect } from '@tanstack/react-router'
import { DashboardPage } from '@/feature/dashboard/dashboard-page'

export const Route = createFileRoute('/')({
  beforeLoad: async ({ context }) => {
    if (!await context.session.ensure()) throw redirect({ to: '/login', replace: true })
  },
  component: Dashboard,
})
function Dashboard() {
  const { session } = Route.useRouteContext()
  return <DashboardPage session={session} />
}
