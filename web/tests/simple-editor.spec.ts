import { expect, test } from '@playwright/test'
import { Buffer } from 'node:buffer'

function richText(content: string) {
  return JSON.stringify([
    {
      type: 'text',
      text: { content, link: null },
      annotations: {
        bold: false,
        italic: false,
        strikethrough: false,
        underline: false,
        code: false,
        color: 'default',
      },
      plain_text: content,
      href: null,
    },
  ])
}

test('simple editor preserves fenced code and equation block semantics after editing and download', async ({ page }) => {
  let downloadRequestBlocks: unknown = null

  await page.route('**/simple/editor/md2blocks', async (route) => {
    await route.fulfill({
      json: {
        blocks: [
          {
            block_type: 'code',
            content_json: richText('const answer = 42'),
            properties_json: JSON.stringify({ language: 'javascript' }),
          },
          {
            block_type: 'code',
            content_json: richText('\\int_0^1 x dx'),
            properties_json: JSON.stringify({ language: 'math' }),
          },
          {
            block_type: 'equation',
            content_json: richText('E = mc^2'),
            properties_json: '{}',
          },
        ],
      },
    })
  })

  await page.route('**/simple/editor/blocks2md', async (route) => {
    const body = route.request().postDataJSON() as { blocks: unknown }
    downloadRequestBlocks = body.blocks
    await route.fulfill({ json: { markdown: '```javascript\nconst answer = 42\n```\n\n```math\n\\int_0^1 x dx + C\n```\n\n$$\nE = mc^2 + 1\n$$\n' } })
  })

  await page.goto('/simple/editor')
  await page.locator('input[type="file"]').setInputFiles({
    name: 'math-and-code.md',
    mimeType: 'text/markdown',
    buffer: Buffer.from('```javascript\nconst answer = 42\n```\n\n```math\n\\int_0^1 x dx\n```\n\n$$\nE = mc^2\n$$\n'),
  })

  await expect(page.locator('.ProseMirror pre')).toHaveCount(3)
  await expect(page.locator('.ProseMirror pre').first()).toContainText('const answer = 42')
  await expect(page.locator('.ProseMirror pre').nth(1)).toContainText('\\int_0^1 x dx')
  await expect(page.locator('.ProseMirror pre').nth(2)).toContainText('E = mc^2')

  await page.getByText('\\int_0^1 x dx').click()
  await page.keyboard.press('End')
  await page.keyboard.type(' + C')
  await page.getByText('E = mc^2').click()
  await page.keyboard.press('End')
  await page.keyboard.type(' + 1')

  await expect.poll(() => {
    return page.evaluate(() => {
      const raw = localStorage.getItem('documosa.simple_editor.draft')
      if (!raw) return []
      return JSON.parse(raw).blocks
        .map((block: { block_type: string; properties_json?: string; content_json: string }) => ({
          block_type: block.block_type,
          language: JSON.parse(block.properties_json || '{}').language ?? null,
          text: JSON.parse(block.content_json).map((token: { plain_text: string }) => token.plain_text).join(''),
        }))
        .filter((block: { text: string }) => block.text.length > 0)
    })
  }).toEqual([
    { block_type: 'code', language: 'javascript', text: 'const answer = 42' },
    { block_type: 'code', language: 'math', text: '\\int_0^1 x dx + C' },
    { block_type: 'equation', language: null, text: 'E = mc^2 + 1' },
  ])

  await page.getByRole('button', { name: 'Download' }).click()
  expect((downloadRequestBlocks as { content_json: string }[]).filter((block) => {
    return JSON.parse(block.content_json).map((token: { plain_text: string }) => token.plain_text).join('').length > 0
  })).toMatchObject([
    { block_type: 'code', properties_json: JSON.stringify({ language: 'javascript' }) },
    { block_type: 'code', properties_json: JSON.stringify({ language: 'math' }) },
    { block_type: 'equation' },
  ])
})

test('simple editor presents the Anthropic paper writing surface', async ({ page }) => {
  await page.route('**/simple/editor/md2blocks', async (route) => {
    await route.fulfill({ json: { blocks: [] } })
  })

  await page.route('**/simple/editor/blocks2md', async (route) => {
    await route.fulfill({ json: { markdown: '' } })
  })

  await page.goto('/simple/editor')

  await expect(page.getByText('Documosa')).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Simple Markdown Editor' })).toBeVisible()
  await expect(page.getByText('A quiet local draft space for quick Markdown edits.')).toBeVisible()
  await expect(page.getByTestId('simple-editor-shell')).toBeVisible()
  await expect(page.getByTestId('simple-editor-paper')).toBeVisible()
  await expect(page.getByTestId('simple-editor-writing-surface')).toBeVisible()
  await expect(page.getByText(/Saved|Saving|Auto-save unavailable/)).toBeVisible()

  await expect(page.locator('main')).toHaveCSS('background-color', 'rgb(236, 233, 224)')
  await expect(page.getByRole('button', { name: 'Download' })).toHaveCSS('background-color', 'rgb(217, 119, 87)')
  await expect(page.getByRole('button', { name: 'Clear' })).toHaveCSS('color', 'rgb(192, 69, 58)')
})

test('simple editor paper layout stays usable on a narrow viewport', async ({ page }) => {
  await page.route('**/simple/editor/md2blocks', async (route) => {
    await route.fulfill({ json: { blocks: [] } })
  })

  await page.route('**/simple/editor/blocks2md', async (route) => {
    await route.fulfill({ json: { markdown: '' } })
  })

  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/simple/editor')

  await expect(page.getByLabel('Filename')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Upload' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Download' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Clear' })).toBeVisible()

  await expect(page.evaluate(() => {
    const scrollingElement = document.scrollingElement
    if (!scrollingElement) return false
    return scrollingElement.scrollWidth > window.innerWidth
  })).resolves.toBe(false)
})
