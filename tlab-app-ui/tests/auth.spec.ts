import { test, expect } from '@playwright/test'
import { sign } from 'node:crypto'
import { readFileSync } from 'node:fs'

const credentialFile = process.env.TLAB_UI_TEST_CREDENTIALS

test('actual backend: login, restore, server refresh, logout and protected entry', async ({
  page,
  context,
  baseURL,
}) => {
  test.skip(!credentialFile, 'Set TLAB_UI_TEST_CREDENTIALS to an isolated test account JSON file')
  const credentials = JSON.parse(readFileSync(credentialFile!, 'utf8')) as {
    email: string
    password: string
  }
  await page.goto('/')
  await expect(page).toHaveURL(/\/login$/)
  await page.getByLabel('이메일').fill(credentials.email)
  await page.getByLabel('비밀번호').fill(credentials.password)
  await page.getByRole('button', { name: '로그인', exact: true }).click()
  await expect(page.getByRole('heading', { name: '대시보드' })).toBeVisible()
  const cookies = await context.cookies()
  expect(
    cookies
      .filter((cookie) => cookie.name.startsWith('tlab_dev_'))
      .map((cookie) => ({
        name: cookie.name,
        httpOnly: cookie.httpOnly,
        sameSite: cookie.sameSite,
      })),
  ).toEqual(
    expect.arrayContaining([
      { name: 'tlab_dev_access', httpOnly: true, sameSite: 'Lax' },
      { name: 'tlab_dev_refresh', httpOnly: true, sameSite: 'Lax' },
    ]),
  )
  await page.reload()
  await expect(page.getByRole('heading', { name: '대시보드' })).toBeVisible()
  await page.goto('/login')
  await expect(page).toHaveURL(baseURL! + '/')
  await context.clearCookies({ name: 'tlab_dev_access' })
  await page.reload()
  await expect(page.getByRole('heading', { name: '대시보드' })).toBeVisible()
  expect((await context.cookies()).some((cookie) => cookie.name === 'tlab_dev_access')).toBe(true)
  if (process.env.TLAB_UI_TEST_SIGNING_KEY) {
    const access = (await context.cookies()).find((cookie) => cookie.name === 'tlab_dev_access')!
    const [header, payload] = access.value.split('.')
    const claims = JSON.parse(Buffer.from(payload, 'base64url').toString())
    claims.iat = Math.floor(Date.now() / 1000) - 601
    claims.exp = claims.iat + 600
    const message = header + '.' + Buffer.from(JSON.stringify(claims)).toString('base64url')
    const signature = sign(
      null,
      Buffer.from(message),
      readFileSync(process.env.TLAB_UI_TEST_SIGNING_KEY),
    ).toString('base64url')
    await context.addCookies([{ ...access, value: message + '.' + signature }])
    await page.reload()
    await expect(page.getByRole('heading', { name: '대시보드' })).toBeVisible()
    const refreshed = (await context.cookies()).find((cookie) => cookie.name === 'tlab_dev_access')!
    expect(refreshed.value === message + '.' + signature).toBe(false)
  }
  await page.getByRole('button', { name: '로그아웃', exact: true }).click()
  await expect(page).toHaveURL(/\/login$/)
  expect((await context.cookies()).some((cookie) => cookie.name.startsWith('tlab_dev_'))).toBe(
    false,
  )
  await page.goto('/')
  await expect(page).toHaveURL(/\/login$/)
  // A revoked Refresh cannot restore access even if copied back into this context.
  await context.addCookies(cookies.filter((cookie) => cookie.name === 'tlab_dev_refresh'))
  await page.goto('/')
  await expect(page).toHaveURL(/\/login$/)
})

test('actual backend: credential rejection remains on login', async ({ page }) => {
  test.skip(!credentialFile, 'Needs isolated backend')
  await page.goto('/login')
  await page.getByLabel('이메일').fill('unknown@example.test')
  await page.getByLabel('비밀번호').fill('invalid')
  await page.getByRole('button', { name: '로그인', exact: true }).click()
  await expect(page.getByRole('alert')).toHaveText('이메일 또는 비밀번호가 올바르지 않습니다.')
  await expect(page.getByLabel('비밀번호')).toBeEnabled()
})

const user = { sub: '1', sid: 'mock', user: { nickname: 'Tester', status: 'ACTIVE', role_ids: [] } }
test('mock: initial server failure has retry without credential prompt', async ({ page }) => {
  let failing = true
  await page.route('**/web/v1/auth/me', (route) =>
    route.fulfill({
      status: failing ? 503 : 401,
      json: { code: failing ? 'INTERNAL_ERROR' : 'INVALID_ACCESS_TOKEN' },
    }),
  )
  await page.goto('/')
  await expect(page.getByRole('heading', { name: '세션을 확인하지 못했습니다' })).toBeVisible()
  await expect(page.getByLabel('비밀번호')).toHaveCount(0)
  failing = false
  await page.getByRole('button', { name: '다시 시도', exact: true }).click()
  await expect(page.getByRole('heading', { name: '로그인' })).toBeVisible()
})

test('mock: confirmation retry never repeats login and duplicate submit is blocked', async ({
  page,
}) => {
  let signedIn = false
  let confirmationFail = true
  let logins = 0
  let release!: () => void
  const held = new Promise<void>((done) => {
    release = done
  })
  await page.route('**/web/v1/auth/me', (route) =>
    route.fulfill({
      status: !signedIn ? 401 : confirmationFail ? 503 : 200,
      json: !signedIn
        ? { code: 'INVALID_ACCESS_TOKEN' }
        : confirmationFail
          ? { code: 'INTERNAL_ERROR' }
          : user,
    }),
  )
  await page.route('**/web/v1/auth/local', async (route) => {
    logins++
    await held
    signedIn = true
    await route.fulfill({ json: {} })
  })
  await page.goto('/login')
  await page.getByLabel('이메일').fill('test@example.test')
  await page.getByLabel('비밀번호').fill('temporary')
  await page.getByRole('button', { name: '로그인', exact: true }).click()
  await expect(page.getByRole('button', { name: '로그인 확인 중…' })).toBeDisabled()
  release()
  await expect(page.getByRole('button', { name: '세션 다시 확인' })).toBeVisible()
  confirmationFail = false
  await page.getByRole('button', { name: '세션 다시 확인' }).click()
  await expect(page.getByRole('heading', { name: '대시보드' })).toBeVisible()
  expect(logins).toBe(1)
})

test('mock: logout failure preserves dashboard and offers retry', async ({ page }) => {
  let signedIn = true
  let failing = true
  await page.route('**/web/v1/auth/me', (route) =>
    route.fulfill({
      status: signedIn ? 200 : 401,
      json: signedIn ? user : { code: 'INVALID_ACCESS_TOKEN' },
    }),
  )
  await page.route('**/web/v1/auth/logout', (route) => {
    if (!failing) signedIn = false
    return route.fulfill({ status: failing ? 503 : 200, json: {} })
  })
  await page.goto('/')
  await page.getByRole('button', { name: '로그아웃', exact: true }).click()
  await expect(page.getByRole('alert')).toContainText('로그아웃을 완료하지 못했습니다')
  await expect(page.getByRole('heading', { name: '대시보드' })).toBeVisible()
  failing = false
  await page.getByRole('button', { name: '로그아웃', exact: true }).click()
  await expect(page.getByRole('heading', { name: '로그인' })).toBeVisible()
})
