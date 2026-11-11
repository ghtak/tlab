import { useId } from 'react'
import { useTheme } from './theme-provider'

export function ThemeSelect() {
  const { theme, setTheme } = useTheme()
  const id = useId()
  return (
    <div className="flex items-center gap-2 text-sm text-foreground">
      <label htmlFor={id}>테마</label>
      <select
        id={id}
        value={theme}
        onChange={(event) => setTheme(event.target.value as typeof theme)}
        className="h-9 rounded-md border border-input bg-background px-2 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <option value="system">시스템 설정</option>
        <option value="light">라이트</option>
        <option value="dark">다크</option>
      </select>
    </div>
  )
}
