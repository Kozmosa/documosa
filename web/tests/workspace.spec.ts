import { expect, test } from '@playwright/test'

const longCommentBody = `${'Long comment detail '.repeat(22)}tail-full-comment`

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
    {
      id: 'line-deleted',
      document_id: 'doc-1',
      order_index: 3000,
      content: 'Deleted legacy line content',
      revision: 2,
      deleted: true,
      created_at: '2026-04-27T00:00:00Z',
      updated_at: '2026-04-27T00:30:00Z',
    },
  ],
  locks: [],
  comments: [
    {
      id: 'comment-1',
      document_id: 'doc-1',
      start_line_id: 'line-1',
      end_line_id: 'line-1',
      start_column: 0,
      end_column: 6,
      author_client_id: 'reviewer',
      author_nickname: 'Reviewer',
      role_mode: 'reviewer',
      body: 'Existing comment',
      resolved: false,
      created_at: '2026-04-27T00:00:00Z',
      updated_at: '2026-04-27T00:00:00Z',
    },
  ],
  replies: [],
  suggestions: [],
  audit_events: [
    {
      id: 'audit-5',
      document_id: 'doc-1',
      actor_client_id: 'system',
      actor_nickname: 'System',
      role_mode: 'writer',
      event_type: 'locks.heartbeat',
      details_json: JSON.stringify({ line_ids: ['line-2'] }),
      created_at: '2026-04-27T04:00:00Z',
    },
    {
      id: 'audit-4',
      document_id: 'doc-1',
      actor_client_id: 'reviewer',
      actor_nickname: 'Reviewer',
      role_mode: 'reviewer',
      event_type: 'suggestion.created',
      details_json: JSON.stringify({ suggestion_id: 'suggestion-1', kind: 'replace' }),
      created_at: '2026-04-27T03:00:00Z',
    },
    {
      id: 'audit-legacy-comment',
      document_id: 'doc-1',
      actor_client_id: 'reviewer',
      actor_nickname: 'Reviewer',
      role_mode: 'reviewer',
      event_type: 'comment.updated',
      details_json: JSON.stringify({ comment_id: 'comment-1' }),
      created_at: '2026-04-27T02:30:00Z',
    },
    {
      id: 'audit-3',
      document_id: 'doc-1',
      actor_client_id: 'reviewer',
      actor_nickname: 'Reviewer',
      role_mode: 'reviewer',
      event_type: 'comment.created',
      details_json: JSON.stringify({
        comment_id: 'comment-1',
        start_line_id: 'line-1',
        end_line_id: 'line-1',
        start_column: 0,
        end_column: 6,
        body: longCommentBody,
      }),
      created_at: '2026-04-27T02:00:00Z',
    },
    {
      id: 'audit-legacy-content',
      document_id: 'doc-1',
      actor_client_id: 'writer',
      actor_nickname: 'Writer',
      role_mode: 'writer',
      event_type: 'document.content_updated',
      details_json: JSON.stringify({
        deleted_line_ids: ['line-deleted'],
        inserted_line_ids: ['line-2'],
      }),
      created_at: '2026-04-27T01:30:00Z',
    },
    {
      id: 'audit-2',
      document_id: 'doc-1',
      actor_client_id: 'writer',
      actor_nickname: 'Writer',
      role_mode: 'writer',
      event_type: 'lines.inserted',
      details_json: JSON.stringify({
        line_ids: ['line-2'],
        count: 1,
        lines: [{ line_id: 'line-2', content_summary: 'Second line', content_length: 11 }],
      }),
      created_at: '2026-04-27T01:00:00Z',
    },
    {
      id: 'audit-1',
      document_id: 'doc-1',
      actor_client_id: 'writer',
      actor_nickname: 'Writer',
      role_mode: 'writer',
      event_type: 'document.created',
      details_json: '{}',
      created_at: '2026-04-27T00:00:00Z',
    },
  ],
}


async function setupRoutes(page: import('@playwright/test').Page) {
  let currentSnapshot = JSON.parse(JSON.stringify(snapshot)) as typeof snapshot
  let savedBody: unknown = null
  let commentBody: unknown = null
  const noteBodies: unknown[] = []

  await page.route('**/api/documents', async (route) => {
    if (route.request().method() === 'POST') {
      await route.fulfill({ json: currentSnapshot })
      return
    }
    await route.fulfill({ json: [currentSnapshot.document] })
  })
  await page.route('**/api/documents/doc-1/**', async (route) => route.fulfill({ json: currentSnapshot }))
  await page.route('**/api/documents/doc-1/content', async (route) => {
    savedBody = route.request().postDataJSON()
    await route.fulfill({ json: currentSnapshot })
  })
  await page.route('**/api/documents/doc-1/comments', async (route) => {
    commentBody = route.request().postDataJSON()
    await route.fulfill({ json: currentSnapshot })
  })
  await page.route('**/api/documents/doc-1/audit-events/*/note', async (route) => {
    const body = route.request().postDataJSON() as { body: string }
    noteBodies.push(body)
    const auditEventId = route.request().url().match(/audit-events\/([^/]+)\/note/)?.[1]
    const trimmed = body.body.trim()
    currentSnapshot = {
      ...currentSnapshot,
      audit_events: currentSnapshot.audit_events.map((event) =>
        event.id === auditEventId
          ? {
              ...event,
              note_body: trimmed || null,
              note_updated_by_nickname: trimmed ? 'Ari' : null,
              note_updated_at: trimmed ? '2026-04-27T05:00:00Z' : null,
            }
          : event,
      ),
    }
    await route.fulfill({ json: currentSnapshot })
  })
  await page.route('**/api/documents/doc-1/history-diff?*', async (route) => {
    const url = new URL(route.request().url())
    const from = url.searchParams.get('from') ?? ''
    const to = url.searchParams.get('to') ?? ''
    const fromEvent = currentSnapshot.audit_events.find((event) => event.id === from)
    const toEvent = currentSnapshot.audit_events.find((event) => event.id === to)
    if (!fromEvent || !toEvent) {
      await route.fulfill({ status: 404, json: { error: 'not found' } })
      return
    }
    if (from.includes('legacy') || to.includes('legacy')) {
      await route.fulfill({ status: 409, json: { error: '版本数据不可用' } })
      return
    }
    await route.fulfill({
      json: {
        from_event: fromEvent,
        to_event: toEvent,
        from_content: 'First line',
        to_content: 'First line\nSecond line',
      },
    })
  })
  await page.route('**/api/documents/doc-1', async (route) => route.fulfill({ json: currentSnapshot }))

  return {
    savedBody: () => savedBody,
    commentBody: () => commentBody,
    noteBodies: () => noteBodies,
  }
}

test('collapsed docs, icons, cherry editor, manual save, and current line review', async ({ page }) => {
  const requests = await setupRoutes(page)

  await page.goto('/')
  await page.evaluate(() => localStorage.setItem('documosa.locale', 'en'))
  await page.reload()
  await page.getByPlaceholder('Nickname').fill('Ari')
  await page.getByRole('button', { name: 'Writer' }).click()
  await page.getByRole('button', { name: 'Enter' }).click()

  await expect(page.getByRole('button', { name: 'Open documents' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Open documents' }).locator('.material-symbols-outlined')).toContainText('menu')
  await expect(page.getByPlaceholder('Title')).toBeHidden()
  await page.getByRole('button', { name: 'Open documents' }).click()
  await expect(page.getByPlaceholder('Title')).toBeVisible()
  await page.getByRole('button', { name: /LAN Draft/ }).click()

  await expect(page.locator('.cherry')).toBeVisible()
  await expect(page.locator('input[value="First line"]')).toHaveCount(0)
  await expect(page.locator('.cm-content')).toContainText('First line')

  await page.locator('.cm-content').click()
  await page.keyboard.type('abc')
  await expect
    .poll(() => page.evaluate(() => window.__documosaEditor?.getMarkdown() ?? ''))
    .toContain('abc')
  await expect(page.evaluate(() => window.__documosaEditor?.getMarkdown() ?? '')).resolves.not.toContain('cba')

  await page.evaluate(() => window.__documosaEditor?.setMarkdown('First line edited\nSecond line'))
  await expect(page.locator('.cm-content')).toContainText('First line edited')
  await expect(page.getByText(/Unsaved/)).toBeVisible()
  await page.getByRole('button', { name: 'Save' }).click()
  expect(requests.savedBody()).toMatchObject({
    content: 'First line edited\nSecond line',
    base_revisions: [
      { line_id: 'line-1', revision: 1 },
      { line_id: 'line-2', revision: 1 },
    ],
  })

  await page.getByRole('button', { name: 'Open documents' }).click()
  await page.getByRole('button', { name: 'Reviewer' }).click()
  await page.getByRole('button', { name: 'Close documents' }).click()
  await page.locator('.cm-content').click()
  await page.keyboard.press('ControlOrMeta+A')
  await page.keyboard.type('Reviewer cannot edit')
  await expect(page.locator('.cm-content')).toContainText('First line')

  await expect(page.getByText('Suggestions')).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Comment', exact: true })).toBeDisabled()
  await page.evaluate(() => window.__documosaEditor?.selectRange(11, 17))
  await page.getByPlaceholder('Comment').fill('Check this sentence')
  await page.getByRole('button', { name: 'Comment', exact: true }).click()
  expect(requests.commentBody()).toMatchObject({
    start_line_id: 'line-2',
    end_line_id: 'line-2',
    start_column: 0,
    end_column: 6,
    body: 'Check this sentence',
  })

  await page.locator('.comment-underline').first().click()
  await expect(page.locator('#comment-comment-1')).toHaveClass(/flash-comment/)

  await page.getByRole('button', { name: 'History' }).click()
  const dialog = page.getByRole('dialog', { name: 'History' })
  await expect(dialog).toContainText('document.created')
  await expect(dialog).toContainText('Line 2')
  await expect(dialog).toContainText('(no longer exists in current version)')
  await expect(dialog).not.toContainText('line-2')
  await expect(dialog).not.toContainText('chars')
  await expect(dialog).toContainText('Existing comment')
  await expect(dialog).not.toContainText('suggestion.created')
  await expect(dialog).not.toContainText('locks.heartbeat')
})

test('history filters category and date, and expands long audit details', async ({ page }) => {
  await setupRoutes(page)

  await page.goto('/')
  await page.evaluate(() => localStorage.setItem('documosa.locale', 'en'))
  await page.reload()
  await page.getByPlaceholder('Nickname').fill('Ari')
  await page.getByRole('button', { name: 'Writer' }).click()
  await page.getByRole('button', { name: 'Enter' }).click()
  await page.getByRole('button', { name: 'Open documents' }).click()
  await page.getByRole('button', { name: /LAN Draft/ }).click()

  await page.getByRole('button', { name: 'History' }).click()
  const dialog = page.getByRole('dialog', { name: 'History' })
  await expect(dialog.getByLabel('Category')).toBeHidden()
  await expect(dialog).toContainText('Comment added')
  await expect(dialog).not.toContainText('Suggestion created')
  await page.getByRole('button', { name: 'Filters' }).click()
  await expect(dialog.getByLabel('Category')).toBeVisible()
  await expect(dialog.getByLabel('Category')).toHaveValue('document_comment')

  await dialog.getByLabel('Category').selectOption('comment')
  await expect(dialog).toContainText('Comment added')
  await expect(dialog).not.toContainText('1 line inserted')
  await expect(dialog).not.toContainText('tail-full-comment')
  await page.getByRole('button', { name: 'Show more' }).click()
  await expect(dialog).toContainText('tail-full-comment')
  await page.getByRole('button', { name: 'Show less' }).click()
  await expect(dialog).not.toContainText('tail-full-comment')

  await dialog.getByLabel('Category').selectOption('content')
  await expect(dialog).toContainText('1 line inserted')
  await expect(dialog).toContainText('document.created')
  await expect(dialog).not.toContainText('Comment added')

  await dialog.getByLabel('Category').selectOption('all')
  await expect(dialog).toContainText('Suggestion created')
  await dialog.getByLabel('From').fill('2026-04-28T00:00')
  await expect(dialog).toContainText('No history events match these filters.')
  await page.getByRole('button', { name: 'Clear filters' }).click()
  await expect(dialog).toContainText('Comment added')
  await expect(dialog).not.toContainText('Suggestion created')

  const sticky = dialog.locator('.history-sticky')
  const before = await sticky.boundingBox()
  await dialog.locator('.history-panel').evaluate((element) => {
    element.scrollTop = element.scrollHeight
  })
  const after = await sticky.boundingBox()
  expect(Math.abs((after?.y ?? 0) - (before?.y ?? 0))).toBeLessThan(2)
})

test('history diff selects two events, opens AceDiff, and reports missing versions', async ({ page }) => {
  await setupRoutes(page)

  await page.goto('/')
  await page.evaluate(() => localStorage.setItem('documosa.locale', 'en'))
  await page.reload()
  await page.getByPlaceholder('Nickname').fill('Ari')
  await page.getByRole('button', { name: 'Writer' }).click()
  await page.getByRole('button', { name: 'Enter' }).click()
  await page.getByRole('button', { name: 'Open documents' }).click()
  await page.getByRole('button', { name: /LAN Draft/ }).click()

  await page.getByRole('button', { name: 'History' }).click()
  const dialog = page.getByRole('dialog', { name: 'History' })
  await page.getByRole('button', { name: 'Diff' }).click()
  await expect(dialog).toContainText('Select the starting history event.')
  await dialog.locator('[data-audit-id="audit-1"]').click()
  await expect(dialog.locator('.audit-row.diff-selected')).toContainText('document.created')
  await expect(dialog).toContainText('Select the ending history event.')
  await dialog.locator('[data-audit-id="audit-2"]').click()

  const diffDialog = page.getByRole('dialog', { name: /Diff:/ })
  await expect(diffDialog).toBeVisible()
  await expect(diffDialog.locator('.history-diff-view')).toBeVisible()
  await expect(diffDialog).toContainText('First line')
  await expect(diffDialog).toContainText('Second line')
  await diffDialog.getByRole('button', { name: 'Close diff' }).click()
  await expect(diffDialog).toBeHidden()
  await expect(page.getByRole('button', { name: 'Diff' })).toBeVisible()
  await expect(dialog.locator('.audit-row.diff-selected')).toHaveCount(0)

  await page.getByRole('button', { name: 'Diff' }).click()
  await dialog.locator('[data-audit-id="audit-legacy-content"]').click()
  await dialog.locator('[data-audit-id="audit-2"]').click()
  await expect(dialog).toContainText('版本数据不可用')
  await expect(page.getByRole('dialog', { name: /Diff:/ })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Cancel diff' })).toBeVisible()
})

test('comments panel collapses restores and persists preference', async ({ page }) => {
  await setupRoutes(page)

  await page.goto('/')
  await page.evaluate(() => localStorage.setItem('documosa.locale', 'en'))
  await page.reload()
  await page.getByPlaceholder('Nickname').fill('Ari')
  await page.getByRole('button', { name: 'Writer' }).click()
  await page.getByRole('button', { name: 'Enter' }).click()
  await page.getByRole('button', { name: 'Open documents' }).click()
  await page.getByRole('button', { name: /LAN Draft/ }).click()

  const workspace = page.locator('.workspace')
  const reviewColumn = page.locator('#review-column')
  const editor = page.locator('.editor')

  await expect(reviewColumn).toBeVisible()
  await expect(page.getByRole('button', { name: 'Hide comments' })).toHaveAttribute('aria-expanded', 'true')
  const editorWidthOpen = await editor.boundingBox().then((box) => box?.width ?? 0)

  await page.getByRole('button', { name: 'Hide comments' }).click()
  await expect(workspace).toHaveClass(/comments-collapsed/)
  await expect(reviewColumn).toBeHidden()
  await expect(page.getByRole('button', { name: 'Show comments' })).toHaveAttribute('aria-expanded', 'false')
  await expect.poll(() => page.evaluate(() => localStorage.getItem('documosa.comments_open'))).toBe('false')
  const editorWidthCollapsed = await editor.boundingBox().then((box) => box?.width ?? 0)
  expect(editorWidthCollapsed).toBeGreaterThan(editorWidthOpen)

  await page.reload()
  await expect(page.getByRole('button', { name: 'Show comments' })).toBeVisible()
  await expect(reviewColumn).toBeHidden()

  await page.getByRole('button', { name: 'Show comments' }).click()
  await expect(workspace).not.toHaveClass(/comments-collapsed/)
  await expect(reviewColumn).toBeVisible()
  await expect(page.getByRole('button', { name: 'Hide comments' })).toHaveAttribute('aria-expanded', 'true')
  await expect.poll(() => page.evaluate(() => localStorage.getItem('documosa.comments_open'))).toBe('true')
})

test('history shared notes can be added edited and cleared', async ({ page }) => {
  const requests = await setupRoutes(page)

  await page.goto('/')
  await page.evaluate(() => localStorage.setItem('documosa.locale', 'en'))
  await page.reload()
  await page.getByPlaceholder('Nickname').fill('Ari')
  await page.getByRole('button', { name: 'Writer' }).click()
  await page.getByRole('button', { name: 'Enter' }).click()
  await page.getByRole('button', { name: 'Open documents' }).click()
  await page.getByRole('button', { name: /LAN Draft/ }).click()

  await page.getByRole('button', { name: 'History' }).click()
  const dialog = page.getByRole('dialog', { name: 'History' })
  await dialog.getByRole('button', { name: 'Add note' }).first().click()
  await dialog.getByPlaceholder('Shared note').fill('Initial note')
  await dialog.getByRole('button', { name: 'Save note' }).click()
  expect(requests.noteBodies()).toContainEqual({ body: 'Initial note' })
  const noteButton = dialog.getByRole('button', { name: 'Initial note' })
  await expect(noteButton).toBeVisible()
  await expect(noteButton).toHaveCSS('text-decoration-line', 'underline')

  await noteButton.click()
  await dialog.getByPlaceholder('Shared note').fill('Edited note')
  await dialog.getByRole('button', { name: 'Save note' }).click()
  expect(requests.noteBodies()).toContainEqual({ body: 'Edited note' })
  await expect(dialog.getByRole('button', { name: 'Edited note' })).toBeVisible()

  await dialog.getByRole('button', { name: 'Edited note' }).click()
  await dialog.getByRole('button', { name: 'Clear note' }).click()
  expect(requests.noteBodies()).toContainEqual({ body: '' })
  await expect(dialog.getByRole('button', { name: 'Add note' }).first()).toBeVisible()
  await expect(dialog).not.toContainText('Edited note')
})


test('language switch localizes interface and persists', async ({ page }) => {
  await setupRoutes(page)
  await page.goto('/')
  await page.evaluate(() => localStorage.setItem('documosa.locale', 'en'))
  await page.reload()

  await page.getByPlaceholder('Nickname').fill('Ari')
  await page.getByRole('button', { name: 'Writer' }).click()
  await page.getByRole('button', { name: 'Enter' }).click()
  await page.getByRole('button', { name: 'Open documents' }).click()
  await page.getByRole('button', { name: '中文' }).click()

  await expect(page.getByRole('button', { name: '保存' })).toBeVisible()
  await expect(page.getByRole('button', { name: '导出' })).toBeVisible()
  await expect(page.getByRole('button', { name: '历史' })).toBeVisible()
  await expect(page.getByPlaceholder('标题')).toBeVisible()
  await expect(page.getByRole('heading', { name: '评论' })).toBeVisible()
  await expect(page.getByRole('button', { name: '打开文档' })).toBeVisible()
  await page.getByRole('button', { name: /LAN Draft/ }).click()
  await page.getByRole('button', { name: '历史' }).click()
  await page.getByRole('button', { name: '筛选' }).click()
  await expect(page.getByLabel('类别')).toBeVisible()
  await expect(page.getByRole('button', { name: '展开' })).toBeVisible()

  await page.reload()
  await expect(page.getByRole('button', { name: '打开文档' })).toBeVisible()
  await page.getByRole('button', { name: '打开文档' }).click()
  await expect(page.getByRole('button', { name: '作者' })).toBeVisible()
})
