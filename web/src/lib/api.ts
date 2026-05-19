const BASE = ''

interface RequestOptions {
  method?: string
  body?: unknown
  token?: string
}

async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const token = options.token || localStorage.getItem('documosa.jwt') || ''
  const response = await fetch(`${BASE}${path}`, {
    method: options.method || 'GET',
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    ...(options.body ? { body: JSON.stringify(options.body) } : {}),
  })
  if (!response.ok) throw new Error(await response.text())
  return response.json()
}

// Page API
export const api = {
  listPages: () => request<Page[]>('/v1/pages'),
  createPage: (title: string) => request<PageSnapshot>('/v1/pages', { method: 'POST', body: { title } }),
  getPage: (id: string) => request<Page>('/v1/pages/' + id),
  updatePage: (id: string, title: string) => request<Page>('/v1/pages/' + id, { method: 'PATCH', body: { title } }),
  getSnapshot: (id: string) => request<PageSnapshot>('/v1/pages/' + id + '/snapshot'),

  getBlock: (id: string) => request<Block>('/v1/blocks/' + id),
  listChildren: (id: string, cursor?: string) =>
    request<{ blocks: Block[], next_cursor: string | null, has_more: boolean }>('/v1/blocks/' + id + '/children' + (cursor ? `?start_cursor=${cursor}` : '')),
  appendBlocks: (pageId: string, children: BlockInput[], after?: string) =>
    request<PageSnapshot>('/v1/pages/' + pageId + '/children', { method: 'PATCH', body: { children, after } }),
  updateBlock: (id: string, data: Partial<Pick<Block, 'block_type' | 'content_json' | 'properties_json'>>) =>
    request<PageSnapshot>('/v1/blocks/' + id, { method: 'PATCH', body: data }),
  deleteBlock: (id: string) =>
    request<PageSnapshot>('/v1/blocks/' + id, { method: 'DELETE' }),

  createComment: (blockId: string, body: string) =>
    request<PageSnapshot>('/v1/blocks/' + blockId + '/comments', { method: 'POST', body: { body } }),
  exportMarkdown: (id: string) =>
    request<string>('/v1/pages/' + id + '/export/md'),
}

// Types
export interface Page { id: string; title: string; properties_json: string; created_at: string; updated_at: string }
export interface Block { id: string; page_id: string; parent_id: string | null; order_index: number; block_type: string; content_json: string; properties_json: string; revision: number; deleted: boolean }
export interface BlockInput { block_type: string; content_json: string; properties_json?: string }
export interface PageSnapshot { page: Page; blocks: Block[]; comments: Comment[]; replies: CommentReply[]; suggestions: unknown[]; locks: BlockLock[]; audit_events: AuditEvent[] }
export interface Comment { id: string; page_id: string; target_block_id: string; author_nickname: string; body: string; resolved: boolean }
export interface CommentReply { id: string; comment_id: string; author_nickname: string; body: string }
export interface BlockLock { block_id: string; owner_nickname: string; expires_at: string }
export interface AuditEvent { id: string; actor_nickname: string; event_type: string; details_json: string; created_at: string; note_body: string | null }
