import { expect, test } from '@playwright/test'

const snapshot = {
  document: {
    id: 'doc-1',
    title: 'LAN Draft',
    created_at: '2026-04-27T00:00:00Z',
    updated_at: '2026-04-27T00:00:00Z',
  },
  lines: [
    {
      id: 'line-1',
      document_id: 'doc-1',
      order_index: 1000,
      content: 'First line',
      revision: 1,
      deleted: false,
      created_at: '2026-04-27T00:00:00Z',
      updated_at: '2026-04-27T00:00:00Z',
    },
    {
      id: 'line-2',
      document_id: 'doc-1',
      order_index: 2000,
      content: 'Second line',
      revision: 1,
      deleted: false,
      created_at: '2026-04-27T00:00:00Z',
      updated_at: '2026-04-27T00:00:00Z',
    },
  ],
  locks: [] as {
    line_id: string
    owner_client_id: string
    owner_nickname: string
    expires_at: string
  }[],
  comments: [],
  replies: [],
  suggestions: [],
  audit_events: [],
}

async function setupRoutes(page: import('@playwright/test').Page, state: typeof snapshot) {
  await page.route('**/api/documents', async (route) => {
    if (route.request().method() === 'POST') {
      await route.fulfill({ json: state })
      return
    }
    await route.fulfill({ json: [state.document] })
  })
  await page.route('**/api/documents/doc-1/**', async (route) => route.fulfill({ json: state }))
  await page.route('**/api/documents/doc-1/content', async (route) => route.fulfill({ json: state }))
  await page.route('**/api/documents/doc-1/locks/heartbeat', async (route) => {
    const body = route.request().postDataJSON() as { line_ids: string[] }
    for (const lineId of body.line_ids) {
      if (!state.locks.find((l) => l.line_id === lineId)) {
        state.locks.push({
          line_id: lineId,
          owner_client_id: 'writer-a',
          owner_nickname: 'Writer A',
          expires_at: new Date(Date.now() + 30000).toISOString(),
        })
      }
    }
    await route.fulfill({ json: state })
  })
  await page.route('**/api/documents/doc-1/locks/release', async (route) => {
    const body = route.request().postDataJSON() as { line_ids: string[] }
    state.locks = state.locks.filter((l) => !body.line_ids.includes(l.line_id))
    await route.fulfill({ json: state })
  })
  await page.route('**/api/documents/doc-1', async (route) => route.fulfill({ json: state }))
}

test('lock badge appears when another user holds a line lock', async ({ page }) => {
  const state = JSON.parse(JSON.stringify(snapshot)) as typeof snapshot
  state.locks = [
    {
      line_id: 'line-1',
      owner_client_id: 'other-writer',
      owner_nickname: 'Other Writer',
      expires_at: new Date(Date.now() + 30000).toISOString(),
    },
  ]

  await setupRoutes(page, state)

  await page.goto('/')
  await page.evaluate(() => localStorage.setItem('documosa.locale', 'en'))
  await page.reload()
  await page.getByPlaceholder('Nickname').fill('Ari')
  await page.getByRole('radio', { name: 'Writer' }).click()
  await page.getByRole('button', { name: 'Enter' }).click()

  await page.getByRole('button', { name: 'Open documents' }).click()
  await page.getByRole('button', { name: /LAN Draft/ }).click()

  await expect(page.locator('.cm-content')).toContainText('First line')

  // Lock badge should show 1 active lock
  const toolbar = page.locator('header')
  await expect(toolbar.getByText('1')).toBeVisible()

  // Hover over lock badge to see tooltip
  await toolbar.getByText('1').hover()
  await expect(page.getByText('Other Writer').first()).toBeVisible()
})

test('two browsers see each other via websocket presence', async ({ browser }) => {
  const state = JSON.parse(JSON.stringify(snapshot)) as typeof snapshot

  // Shared presence state across both contexts
  const presenceUsers = [
    { document_id: 'doc-1', client_id: 'writer-a', nickname: 'Alice', role_mode: 'writer' as const },
    { document_id: 'doc-1', client_id: 'writer-b', nickname: 'Bob', role_mode: 'writer' as const },
  ]

  const contextA = await browser.newContext()
  const contextB = await browser.newContext()
  const pageA = await contextA.newPage()
  const pageB = await contextB.newPage()

  await setupRoutes(pageA, state)
  await setupRoutes(pageB, state)

  // Mock WebSocket for both pages to simulate presence
  await pageA.routeWebSocket(/\/api\/documents\/doc-1\/ws/, (ws) => {
    ws.onMessage(() => {
      // Page A connects, send presence with both users
    })
    setTimeout(() => {
      ws.send(JSON.stringify({ type: 'presence', document_id: 'doc-1', users: presenceUsers }))
    }, 500)
  })

  await pageB.routeWebSocket(/\/api\/documents\/doc-1\/ws/, (ws) => {
    ws.onMessage(() => {
      // Page B connects, send presence with both users
    })
    setTimeout(() => {
      ws.send(JSON.stringify({ type: 'presence', document_id: 'doc-1', users: presenceUsers }))
    }, 500)
  })

  // Set up Page A (Alice)
  await pageA.goto('/')
  await pageA.evaluate(() => localStorage.setItem('documosa.locale', 'en'))
  await pageA.reload()
  await pageA.getByPlaceholder('Nickname').fill('Alice')
  await pageA.getByRole('radio', { name: 'Writer' }).click()
  await pageA.getByRole('button', { name: 'Enter' }).click()
  await pageA.getByRole('button', { name: 'Open documents' }).click()
  await pageA.getByRole('button', { name: /LAN Draft/ }).click()

  // Set up Page B (Bob)
  await pageB.goto('/')
  await pageB.evaluate(() => localStorage.setItem('documosa.locale', 'en'))
  await pageB.reload()
  await pageB.getByPlaceholder('Nickname').fill('Bob')
  await pageB.getByRole('radio', { name: 'Writer' }).click()
  await pageB.getByRole('button', { name: 'Enter' }).click()
  await pageB.getByRole('button', { name: 'Open documents' }).click()
  await pageB.getByRole('button', { name: /LAN Draft/ }).click()

  // Both pages should show 2 online users (look for badge, not line count)
  await expect(pageA.locator('header [data-slot="badge"]').getByText('2')).toBeVisible()
  await expect(pageB.locator('header [data-slot="badge"]').getByText('2')).toBeVisible()

  // Clean up
  await contextA.close()
  await contextB.close()
})
