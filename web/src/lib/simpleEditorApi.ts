import type { DocumosaBlock } from '@/lib/converter'

async function postJson<T>(path: string, body: unknown): Promise<T> {
  const response = await fetch(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })

  if (!response.ok) throw new Error(await response.text())
  return response.json() as Promise<T>
}

export async function markdownToBlocks(markdown: string): Promise<DocumosaBlock[]> {
  const response = await postJson<{ blocks: DocumosaBlock[] }>('/simple/editor/md2blocks', { markdown })
  return response.blocks
}

export async function blocksToMarkdown(blocks: DocumosaBlock[]): Promise<string> {
  const response = await postJson<{ markdown: string }>('/simple/editor/blocks2md', { blocks })
  return response.markdown
}
