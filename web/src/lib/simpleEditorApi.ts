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

export function markdownToBlocks(markdown: string): Promise<DocumosaBlock[]> {
  return postJson<DocumosaBlock[]>('/simple/editor/md2blocks', { markdown })
}

export function blocksToMarkdown(blocks: DocumosaBlock[]): Promise<string> {
  return postJson<string>('/simple/editor/blocks2md', { blocks })
}
