type WsEventHandler = (event: WsEvent) => void

interface WsEvent {
  type: string
  page_id?: string
  document_id?: string
  users?: PresenceUser[]
  line_ids?: string[]
  block_ids?: string[]
  after_block_id?: string | null
  block_id?: string
  comment_id?: string
  suggestion_id?: string
  accepted?: boolean
  title?: string
}

interface PresenceUser {
  client_id: string
  nickname: string
  role_mode: string
}

export function connectWs(pageId: string, clientId: string, nickname: string, roleMode: string, onEvent: WsEventHandler): () => void {
  const scheme = location.protocol === 'https:' ? 'wss' : 'ws'
  const url = `${scheme}://${location.host}/v1/pages/${pageId}/ws?client_id=${encodeURIComponent(clientId)}&nickname=${encodeURIComponent(nickname)}&role_mode=${roleMode}`
  const socket = new WebSocket(url)

  socket.onmessage = (msg) => {
    try {
      const event = JSON.parse(msg.data) as WsEvent
      onEvent(event)
    } catch { /* ignore parse errors */ }
  }

  return () => socket.close()
}

export type { WsEvent, PresenceUser }
