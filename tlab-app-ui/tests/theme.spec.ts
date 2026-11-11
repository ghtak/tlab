import { test, expect } from '@playwright/test'

test.beforeEach(async ({ page }) => {
  await page.route('**/web/v1/auth/me', route => route.fulfill({ status: 401, json: { code: 'INVALID_ACCESS_TOKEN' } }))
})

test('theme choice changes actual colors and survives reload', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' })
  await page.goto('/login')
  const select = page.getByLabel('테마', { exact: true })
  await expect(select).toHaveValue('system')
  const lightBackground = await page.locator('body').evaluate(element => getComputedStyle(element).backgroundColor)
  await select.selectOption('dark')
  await expect(page.locator('html')).toHaveClass('dark')
  expect(await page.locator('body').evaluate(element => getComputedStyle(element).backgroundColor)).not.toBe(lightBackground)
  await expect(page.getByRole('heading', { name: '로그인' })).toBeVisible()
  await page.reload()
  await expect(select).toHaveValue('dark')
  await expect(page.locator('html')).toHaveCSS('color-scheme', 'dark')
  await select.selectOption('light')
  await expect(page.locator('html')).not.toHaveClass('dark')
  await expect(page.locator('body')).toHaveCSS('background-color', lightBackground)
})

test('system follows OS changes while explicit choice takes precedence', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' })
  await page.goto('/login')
  await expect(page.locator('html')).toHaveClass('dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await expect(page.locator('html')).not.toHaveClass('dark')
  await page.getByLabel('테마', { exact: true }).selectOption('light')
  await page.emulateMedia({ colorScheme: 'dark' })
  await expect(page.locator('html')).not.toHaveClass('dark')
  await page.getByLabel('테마', { exact: true }).selectOption('system')
  await expect(page.locator('html')).toHaveClass('dark')
  await page.reload()
  await expect(page.getByLabel('테마', { exact: true })).toHaveValue('system')
})

test('saved theme applies before React mounts', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' })
  await page.addInitScript(() => localStorage.setItem('tlab.theme', 'dark'))
  await page.route('**/src/main.tsx*', route => route.abort())
  await page.goto('/login')
  await expect(page.locator('html')).toHaveClass('dark')
  await expect(page.locator('#root')).toBeEmpty()
})
