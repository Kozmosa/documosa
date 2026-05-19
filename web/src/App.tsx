import { Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { FormEvent } from 'react'
import AceDiff from 'ace-diff'
import * as ace from 'ace-builds'
import 'ace-builds/src-noconflict/mode-markdown'
import 'ace-builds/src-noconflict/theme-textmate'
import 'ace-diff/styles.css'
import { useTranslation } from 'react-i18next'
import type { Locale } from './i18n'
import './App.css'

import { api, pageTitle } from '@/lib/api'
import type { Page, PageSnapshot, Comment, CommentReply, AuditEvent, Block, BlockInput } from '@/lib/api'
import { connectWs } from '@/lib/ws'
import type { PresenceUser } from '@/lib/ws'
import TiptapEditor from '@/TiptapEditor'
import type { DocumosaBlock } from '@/lib/converter'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetTitle,
} from '@/components/ui/sheet'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  ToggleGroup,
  ToggleGroupItem,
} from '@/components/ui/toggle-group'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import {
  LogIn,
  Menu,
  Languages,
  Upload,
  Save,
  Download,
  History,
  PanelRightClose,
  PanelRightOpen,
  X,
  Filter,
  ArrowLeftRight,
  MessageSquarePlus,
  RefreshCw,
  Users,
  Lock,
} from 'lucide-react'

type RoleMode = 'reviewer' | 'writer'

type Identity = {
  clientId: string
  nickname: string
  roleMode: RoleMode
}

type HistoryDiffResponse = {
  from_event: AuditEvent
  to_event: AuditEvent
  from_content: string
  to_content: string
}

const COMMENTS_OPEN_KEY = 'documosa.comments_open'

type Translation = ReturnType<typeof useTranslation>['t']

function getClientId() {
  const existing = localStorage.getItem('documosa.client_id')
  if (existing) return existing
  const created = crypto.randomUUID()
  localStorage.setItem('documosa.client_id', created)
  return created
}

function getInitialCommentsOpen() {
  return localStorage.getItem(COMMENTS_OPEN_KEY) !== 'false'
}

function blockText(blocks: DocumosaBlock[]) {
  return blocks
    .filter((b) => !b.id || b.block_type !== 'divider')
    .map((b) => {
      try {
        const tokens = JSON.parse(b.content_json) as { plain_text: string }[]
        return tokens.map(t => t.plain_text).join('')
      } catch {
        return ''
      }
    })
    .join('\n')
}

function blockPreview(block: DocumosaBlock | undefined, t: Translation) {
  if (!block) return t('review.noLine')
  try {
    const tokens = JSON.parse(block.content_json) as { plain_text: string }[]
    const text = tokens.map(t => t.plain_text).join('').trim()
    return text || t('review.blankLine')
  } catch {
    return t('review.blankLine')
  }
}

type AuditCategory = 'document_comment' | 'all' | 'content' | 'comment' | 'suggestion' | 'system'

type AuditDetails = Record<string, unknown>

const AUDIT_CATEGORIES: AuditCategory[] = ['document_comment', 'all', 'content', 'comment', 'suggestion', 'system']
const AUDIT_COLLAPSE_LIMIT = 260

type AuditFormatContext = {
  blocks: Map<string, Block>
  activeLineNumberById: Map<string, number>
  comments: Map<string, Comment>
  replies: Map<string, CommentReply>
}

function parseAuditDetails(event: AuditEvent): AuditDetails | null {
  try {
    const parsed = JSON.parse(event.details_json) as unknown
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) return parsed as AuditDetails
  } catch {
    return null
  }
  return null
}

function hasDetails(details: AuditDetails | null): details is AuditDetails {
  return details !== null && Object.keys(details).length > 0
}

function auditCategory(eventType: string): Exclude<AuditCategory, 'document_comment' | 'all'> {
  if (eventType === 'document.created' || eventType === 'document.content_updated' || eventType.startsWith('lines.') || eventType.startsWith('blocks.')) return 'content'
  if (eventType.startsWith('comment.')) return 'comment'
  if (eventType.startsWith('suggestion.')) return 'suggestion'
  return 'system'
}

function detailString(details: AuditDetails, key: string) {
  const value = details[key]
  return typeof value === 'string' ? value : ''
}

function detailNumber(details: AuditDetails, key: string) {
  const value = details[key]
  return typeof value === 'number' ? value : undefined
}

function detailArray(details: AuditDetails, key: string) {
  const value = details[key]
  return Array.isArray(value) ? value : []
}

function detailStringArray(details: AuditDetails, key: string) {
  return detailArray(details, key).filter((value): value is string => typeof value === 'string')
}

function contentSummary(value: string) {
  return value.length <= 120 ? value : `${value.slice(0, 120).trimEnd()}...`
}

function lineLabel(lineId: string, context: AuditFormatContext, t: Translation) {
  const lineNumber = context.activeLineNumberById.get(lineId)
  return lineNumber === undefined ? t('history.line.missing') : t('history.line.label', { number: lineNumber })
}

function describeAuditLines(details: AuditDetails, t: Translation, context: AuditFormatContext, mode: 'inserted' | 'deleted') {
  const lines = detailArray(details, 'lines')
  if (lines.length === 0) return ''
  return lines
    .map((value) => {
      const line = value && typeof value === 'object' ? (value as AuditDetails) : {}
      const id = detailString(line, 'line_id')
      const summary = detailString(line, 'content_summary')
      return t(`history.line.${mode}`, { line: lineLabel(id, context, t), summary })
    })
    .join('\n')
}

function describeAuditReplacements(details: AuditDetails, t: Translation, context: AuditFormatContext) {
  const lines = detailArray(details, 'lines')
  if (lines.length === 0) return ''
  return lines
    .map((value) => {
      const line = value && typeof value === 'object' ? (value as AuditDetails) : {}
      return t('history.line.replaced', {
        line: lineLabel(detailString(line, 'line_id'), context, t),
        before: detailString(line, 'before_summary'),
        after: detailString(line, 'after_summary'),
      })
    })
    .join('\n')
}

function lineDetailsFromIds(lineIds: string[], context: AuditFormatContext) {
  return lineIds
    .map((lineId) => {
      const block = context.blocks.get(lineId)
      if (!block) return null
      return {
        line_id: block.id,
        content_summary: contentSummary(block.content_json),
      }
    })
    .filter((item): item is { line_id: string; content_summary: string } => item !== null)
}

function describeLegacyLineIds(lineIds: string[], context: AuditFormatContext, t: Translation, mode: 'inserted' | 'deleted') {
  const lines = lineDetailsFromIds(lineIds, context)
  if (lines.length === 0) return lineIds.map((lineId) => lineLabel(lineId, context, t)).join('\n')
  return describeAuditLines({ lines }, t, context, mode)
}

function describeLegacyReplacedLineIds(lineIds: string[], context: AuditFormatContext, t: Translation) {
  const lines = lineIds
    .map((lineId) => {
      const block = context.blocks.get(lineId)
      if (!block) return null
      return t('history.line.current', {
        line: lineLabel(lineId, context, t),
        summary: contentSummary(block.content_json),
      })
    })
    .filter(Boolean)
    .join('\n')
  return lines || lineIds.map((lineId) => lineLabel(lineId, context, t)).join('\n')
}

function commentBodyFromDetails(details: AuditDetails, context: AuditFormatContext) {
  const commentId = detailString(details, 'comment_id')
  return commentId ? context.comments.get(commentId)?.body ?? '' : ''
}

function replyBodyFromDetails(details: AuditDetails, context: AuditFormatContext) {
  const replyId = detailString(details, 'reply_id')
  return replyId ? context.replies.get(replyId)?.body ?? '' : ''
}

function commentReference(details: AuditDetails, t: Translation) {
  const commentId = detailString(details, 'comment_id')
  return commentId ? t('history.summary.commentReference', { id: commentId }) : ''
}

function legacyAuditFormat(event: AuditEvent, category: Exclude<AuditCategory, 'document_comment' | 'all'>) {
  return {
    category,
    title: event.event_type,
    body: event.details_json,
  }
}

function formatAuditEvent(event: AuditEvent, t: Translation, context: AuditFormatContext) {
  const details = parseAuditDetails(event)
  const category = auditCategory(event.event_type)
  if (!hasDetails(details)) {
    return legacyAuditFormat(event, category)
  }

  switch (event.event_type) {
    case 'document.created':
      return {
        category,
        title: t('history.events.documentCreated'),
        body: t('history.summary.documentCreated', {
          title: detailString(details, 'title'),
          count: detailNumber(details, 'line_count') ?? context.blocks.size,
          summary: detailString(details, 'initial_content_summary'),
        }),
      }
    case 'document.content_updated':
      if (detailNumber(details, 'inserted_count') === undefined || detailNumber(details, 'deleted_count') === undefined) {
        const insertedBlockIds = detailStringArray(details, 'inserted_block_ids')
        const deletedBlockIds = detailStringArray(details, 'deleted_block_ids')
        return {
          category,
          title: t('history.events.documentContentUpdated'),
          body: [
            t('history.summary.documentContentUpdated', {
              inserted: insertedBlockIds.length,
              deleted: deletedBlockIds.length,
            }),
            describeLegacyLineIds(insertedBlockIds, context, t, 'inserted'),
            describeLegacyLineIds(deletedBlockIds, context, t, 'deleted'),
          ]
            .filter(Boolean)
            .join('\n'),
        }
      }
      return {
        category,
        title: t('history.events.documentContentUpdated'),
        body: [
          t('history.summary.documentContentUpdated', {
            inserted: detailNumber(details, 'inserted_count') ?? detailArray(details, 'inserted_block_ids').length,
            deleted: detailNumber(details, 'deleted_count') ?? detailArray(details, 'deleted_block_ids').length,
          }),
          describeAuditLines({ lines: detailArray(details, 'inserted_lines') }, t, context, 'inserted'),
          describeAuditLines({ lines: detailArray(details, 'deleted_lines') }, t, context, 'deleted'),
        ]
          .filter(Boolean)
          .join('\n'),
      }
    case 'lines.inserted':
    case 'blocks.inserted':
      if (detailArray(details, 'lines').length === 0) {
        const blockIds = detailStringArray(details, 'block_ids')
        return {
          category,
          title: t('history.events.linesInserted', { count: blockIds.length }),
          body: describeLegacyLineIds(blockIds, context, t, 'inserted'),
        }
      }
      return {
        category,
        title: t('history.events.linesInserted', { count: detailNumber(details, 'count') ?? detailArray(details, 'block_ids').length }),
        body: describeAuditLines(details, t, context, 'inserted'),
      }
    case 'lines.replaced':
    case 'blocks.replaced':
      if (detailArray(details, 'lines').length === 0) {
        const blockIds = detailStringArray(details, 'block_ids')
        return {
          category,
          title: t('history.events.linesReplaced', { count: blockIds.length }),
          body: describeLegacyReplacedLineIds(blockIds, context, t),
        }
      }
      return {
        category,
        title: t('history.events.linesReplaced', { count: detailNumber(details, 'count') ?? detailArray(details, 'block_ids').length }),
        body: describeAuditReplacements(details, t, context),
      }
    case 'lines.deleted':
    case 'blocks.deleted':
      if (detailArray(details, 'lines').length === 0) {
        const blockIds = detailStringArray(details, 'block_ids')
        return {
          category,
          title: t('history.events.linesDeleted', { count: blockIds.length }),
          body: describeLegacyLineIds(blockIds, context, t, 'deleted'),
        }
      }
      return {
        category,
        title: t('history.events.linesDeleted', { count: detailNumber(details, 'count') ?? detailArray(details, 'block_ids').length }),
        body: describeAuditLines(details, t, context, 'deleted'),
      }
    case 'comment.created':
      return {
        category,
        title: t('history.events.commentCreated'),
        body: t('history.summary.commentCreated', {
          start: lineLabel(detailString(details, 'start_line_id'), context, t),
          end: lineLabel(detailString(details, 'end_line_id'), context, t),
          body: detailString(details, 'body') || commentBodyFromDetails(details, context) || commentReference(details, t),
        }),
      }
    case 'comment.updated':
      return {
        category,
        title: t('history.events.commentUpdated'),
        body: t('history.summary.commentUpdated', {
          before: detailString(details, 'before_body') || commentReference(details, t),
          after: detailString(details, 'after_body') || commentBodyFromDetails(details, context),
        }),
      }
    case 'comment.deleted':
      return {
        category,
        title: t('history.events.commentDeleted'),
        body: detailString(details, 'body') || commentBodyFromDetails(details, context) || commentReference(details, t),
      }
    case 'comment.replied':
      return {
        category,
        title: t('history.events.commentReplied'),
        body: detailString(details, 'body') || replyBodyFromDetails(details, context) || commentReference(details, t),
      }
    case 'comment.resolved':
      return {
        category,
        title: t('history.events.commentResolved'),
        body: detailString(details, 'body') || commentBodyFromDetails(details, context) || commentReference(details, t),
      }
    case 'suggestion.created':
      return {
        category,
        title: t('history.events.suggestionCreated'),
        body: t('history.summary.suggestionCreated', { kind: detailString(details, 'kind') }),
      }
    case 'suggestion.accepted':
      return {
        category,
        title: t('history.events.suggestionAccepted'),
        body: detailString(details, 'suggestion_id'),
      }
    case 'suggestion.rejected':
      return {
        category,
        title: t('history.events.suggestionRejected'),
        body: detailString(details, 'suggestion_id'),
      }
    default:
      return legacyAuditFormat(event, category)
  }
}

function truncateAuditText(value: string) {
  if (value.length <= AUDIT_COLLAPSE_LIMIT) return value
  return `${value.slice(0, AUDIT_COLLAPSE_LIMIT).trimEnd()}...`
}

function readableError(error: unknown) {
  if (!(error instanceof Error)) return String(error)
  try {
    const parsed = JSON.parse(error.message) as { error?: unknown }
    if (typeof parsed.error === 'string') return parsed.error
  } catch {
    return error.message
  }
  return error.message
}

function HistoryDiffModal({
  diff,
  formatDate,
  onClose,
}: {
  diff: HistoryDiffResponse
  formatDate: (value: string) => string
  onClose: () => void
}) {
  const containerRef = useRef<HTMLDivElement | null>(null)
  const title = `Diff: ${formatDate(diff.from_event.created_at)} -> ${formatDate(diff.to_event.created_at)}`

  useEffect(() => {
    const container = containerRef.current
    if (!container) return undefined
    let destroyed = false
    let aceDiff: AceDiff | null = null
    const handle = setTimeout(() => {
      if (destroyed || !container.isConnected) return
      aceDiff = new AceDiff({
        ace,
        element: container,
        mode: 'ace/mode/markdown',
        theme: 'ace/theme/textmate',
        diffGranularity: 'specific',
        showConnectors: true,
        showDiffs: true,
        lockScrolling: true,
        left: {
          content: diff.from_content,
          editable: false,
          copyLinkEnabled: false,
        },
        right: {
          content: diff.to_content,
          editable: false,
          copyLinkEnabled: false,
        },
      })
      if (destroyed) {
        aceDiff.destroy()
        return
      }
      const editors = aceDiff.getEditors()
      for (const editor of [editors.left, editors.right]) {
        editor.setReadOnly(true)
        editor.session.setUseWorker(false)
        editor.setOptions({
          highlightActiveLine: false,
          highlightGutterLine: false,
          showPrintMargin: false,
          wrap: true,
        })
      }
    }, 100)
    return () => {
      destroyed = true
      clearTimeout(handle)
      aceDiff?.destroy()
    }
  }, [diff])

  return (
    <Dialog open onOpenChange={onClose}>
      <DialogContent className="max-w-[1120px] w-[96vw] max-h-[760px] h-[92vh] flex flex-col gap-3 p-5">
        <DialogHeader className="flex flex-row items-center justify-between gap-3">
          <DialogTitle className="text-lg">{title}</DialogTitle>
        </DialogHeader>
        <DialogDescription className="sr-only">Side-by-side diff view of document versions</DialogDescription>
        <div className="flex justify-between text-xs text-muted-foreground px-1">
          <span>{diff.from_event.event_type}</span>
          <span>{diff.to_event.event_type}</span>
        </div>
        <div className="history-diff-view flex-1 min-h-0" ref={containerRef} />
      </DialogContent>
    </Dialog>
  )
}

function App() {
  const { t, i18n } = useTranslation()
  const locale = i18n.language.startsWith('zh') ? 'zh' : 'en'
  const formatDate = useCallback(
    (value: string) =>
      new Intl.DateTimeFormat(locale === 'zh' ? 'zh-CN' : 'en-US', {
        dateStyle: 'medium',
        timeStyle: 'short',
      }).format(new Date(value)),
    [locale],
  )
  const changeLocale = useCallback(
    (nextLocale: Locale) => {
      localStorage.setItem('documosa.locale', nextLocale)
      void i18n.changeLanguage(nextLocale)
    },
    [i18n],
  )

  const [identity, setIdentity] = useState<Identity>(() => ({
    clientId: getClientId(),
    nickname: localStorage.getItem('documosa.nickname') ?? '',
    roleMode: (localStorage.getItem('documosa.role_mode') as RoleMode) ?? 'reviewer',
  }))
  const [identitySaved, setIdentitySaved] = useState(() => localStorage.getItem('documosa.nickname') !== null)
  const [pages, setPages] = useState<Page[]>([])
  const [snapshot, setSnapshot] = useState<PageSnapshot | null>(null)
  const [blocks, setBlocks] = useState<DocumosaBlock[]>([])
  const [selectedBlockIndex, setSelectedBlockIndex] = useState<number | null>(null)
  const [dirty, setDirty] = useState(false)
  const [remoteConflict, setRemoteConflict] = useState(false)
  const [sidebarOpen, setSidebarOpen] = useState(false)
  const [historyOpen, setHistoryOpen] = useState(false)
  const [historyFiltersOpen, setHistoryFiltersOpen] = useState(false)
  const [historyCategory, setHistoryCategory] = useState<AuditCategory>('document_comment')
  const [commentsOpen, setCommentsOpen] = useState(getInitialCommentsOpen)
  const [historyFrom, setHistoryFrom] = useState('')
  const [historyTo, setHistoryTo] = useState('')
  const [expandedAuditIds, setExpandedAuditIds] = useState<Set<string>>(() => new Set())
  const [editingAuditNoteId, setEditingAuditNoteId] = useState<string | null>(null)
  const [auditNoteDraft, setAuditNoteDraft] = useState('')
  const [historyDiffMode, setHistoryDiffMode] = useState(false)
  const [historyDiffFromId, setHistoryDiffFromId] = useState<string | null>(null)
  const [historyDiffError, setHistoryDiffError] = useState('')
  const [historyDiffLoading, setHistoryDiffLoading] = useState(false)
  const [historyDiff, setHistoryDiff] = useState<HistoryDiffResponse | null>(null)
  const [draftTitle, setDraftTitle] = useState('')
  const [commentBody, setCommentBody] = useState('')
  const [activeCommentId, setActiveCommentId] = useState<string | null>(null)
  const [status, setStatus] = useState('')
  const [onlineUsers, setOnlineUsers] = useState<PresenceUser[]>([])
  const [presenceOpen, setPresenceOpen] = useState(false)
  const dirtyRef = useRef(false)

  useEffect(() => {
    dirtyRef.current = dirty
  }, [dirty])

  useEffect(() => {
    localStorage.setItem(COMMENTS_OPEN_KEY, String(commentsOpen))
  }, [commentsOpen])

  const activeBlocks = useMemo(() => blocks, [blocks])
  const selectedBlock = selectedBlockIndex !== null ? activeBlocks[selectedBlockIndex] ?? null : null
  const selectedText = selectedBlock ? (() => {
    try {
      const tokens = JSON.parse(selectedBlock.content_json) as { plain_text: string }[]
      return tokens.map(t => t.plain_text).join('')
    } catch {
      return ''
    }
  })() : ''
  const annotationsDisabled = dirty || remoteConflict || !selectedBlock || !selectedText

  const auditFormatContext = useMemo<AuditFormatContext>(
    () => ({
      blocks: new Map(snapshot?.blocks.map((b) => [b.id, b]) ?? []),
      activeLineNumberById: new Map(activeBlocks.map((b, index) => [b.id || String(index), index + 1])),
      comments: new Map(snapshot?.comments.map((comment) => [comment.id, comment]) ?? []),
      replies: new Map(snapshot?.replies.map((reply) => [reply.id, reply]) ?? []),
    }),
    [activeBlocks, snapshot?.blocks, snapshot?.comments, snapshot?.replies],
  )
  const filteredAuditEvents = useMemo(() => {
    const fromTime = historyFrom ? new Date(historyFrom).getTime() : null
    const toTime = historyTo ? new Date(historyTo).getTime() : null
    return (
      snapshot?.audit_events.filter((event) => {
        const eventCategory = auditCategory(event.event_type)
        if (historyCategory === 'document_comment') {
          if (eventCategory !== 'content' && eventCategory !== 'comment') return false
        } else if (historyCategory !== 'all' && eventCategory !== historyCategory) {
          return false
        }
        const eventTime = new Date(event.created_at).getTime()
        if (fromTime !== null && eventTime < fromTime) return false
        if (toTime !== null && eventTime > toTime) return false
        return true
      }) ?? []
    )
  }, [historyCategory, historyFrom, historyTo, snapshot?.audit_events])

  const applySnapshot = useCallback((next: PageSnapshot, resetEditor: boolean) => {
    setSnapshot(next)
    setExpandedAuditIds(new Set())
    setEditingAuditNoteId(null)
    if (resetEditor) {
      const documosaBlocks: DocumosaBlock[] = next.blocks
        .filter((b) => !b.deleted)
        .map((b) => ({
          id: b.id,
          block_type: b.block_type,
          content_json: b.content_json,
          properties_json: b.properties_json,
        }))
      setBlocks(documosaBlocks)
      setDirty(false)
      setRemoteConflict(false)
      setSelectedBlockIndex(null)
    }
  }, [])

  const refreshPages = useCallback(async () => {
    const nextPages = await api.listPages()
    setPages(nextPages)
  }, [])

  const refreshSnapshot = useCallback(
    async (pageId: string, resetEditor = true) => {
      const next = await api.getSnapshot(pageId)
      applySnapshot(next, resetEditor)
    },
    [applySnapshot],
  )

  useEffect(() => {
    if (!identity.nickname) return undefined
    let cancelled = false
    void api.listPages()
      .then((nextPages) => {
        if (!cancelled) setPages(nextPages)
      })
      .catch((error) => {
        if (!cancelled) setStatus(error.message)
      })
    return () => {
      cancelled = true
    }
  }, [identity.nickname])

  useEffect(() => {
    if (!snapshot || !identity.nickname) return undefined
    return connectWs(
      snapshot.page.id,
      identity.clientId,
      identity.nickname,
      identity.roleMode,
      (event) => {
        switch (event.type) {
          case 'presence':
            if (event.users) setOnlineUsers(event.users.map(u => ({
              client_id: u.client_id,
              nickname: u.nickname,
              role_mode: u.role_mode,
            })))
            return

          case 'block_inserted':
          case 'block_updated':
          case 'block_deleted':
          case 'comment_created':
          case 'comment_resolved':
          case 'suggestion_created':
          case 'suggestion_decided':
          case 'page_title_updated':
          case 'locks_changed':
          case 'content_changed':
            void api.getSnapshot(snapshot.page.id)
              .then((next) => {
                void refreshPages().catch(() => undefined)
                applySnapshot(next, false)
              })
              .catch((error) => setStatus(error.message))
            return
        }
      },
    )
  }, [applySnapshot, identity, refreshPages, snapshot])

  function saveIdentity(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    localStorage.setItem('documosa.nickname', identity.nickname)
    localStorage.setItem('documosa.role_mode', identity.roleMode)
    setIdentitySaved(true)
  }

  async function createDocument(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const next = await api.createPage(draftTitle || 'Untitled')
    setDraftTitle('')
    await refreshPages()
    applySnapshot(next, true)
    setSidebarOpen(false)
    setStatus(t('status.documentCreated'))
  }

  async function selectPage(pageId: string) {
    if (dirty && !confirm(t('confirm.discardUnsaved'))) return
    await refreshSnapshot(pageId)
    setSidebarOpen(false)
  }

  async function exportDocument() {
    if (!snapshot) return
    const text = await api.exportMarkdown(snapshot.page.id)
    const blob = new Blob([text], { type: 'text/plain' })
    const href = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = href
    anchor.download = `${pageTitle(snapshot.page)}.md`
    anchor.click()
    URL.revokeObjectURL(href)
  }

  async function saveContent() {
    if (!snapshot || identity.roleMode !== 'writer' || remoteConflict) return
    const next = await api.appendBlocks(snapshot.page.id, blocks.map((b) => ({
      block_type: b.block_type,
      content_json: b.content_json,
      properties_json: b.properties_json,
    } as BlockInput)))
    await refreshPages()
    applySnapshot(next, true)
    setStatus(t('status.saved'))
  }

  async function createComment(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!snapshot || !selectedBlock || annotationsDisabled || !commentBody.trim()) return
    const blockId = selectedBlock.id
    if (!blockId) return
    const next = await api.createComment(blockId, commentBody)
    setCommentBody('')
    applySnapshot(next, false)
  }

  function handleEditorChange(newBlocks: DocumosaBlock[], text: string) {
    setBlocks(newBlocks)
    setDirty(text !== blockText(snapshot?.blocks.filter((b) => !b.deleted).map((b) => ({
      id: b.id,
      block_type: b.block_type,
      content_json: b.content_json,
      properties_json: b.properties_json,
    })) ?? []))
  }

  function clearHistoryFilters() {
    setHistoryCategory('document_comment')
    setHistoryFrom('')
    setHistoryTo('')
  }

  function cancelHistoryDiff() {
    setHistoryDiffMode(false)
    setHistoryDiffFromId(null)
    setHistoryDiffError('')
    setHistoryDiffLoading(false)
  }

  function closeHistoryDrawer() {
    setHistoryOpen(false)
    cancelHistoryDiff()
    setHistoryDiff(null)
  }

  function closeHistoryDiff() {
    setHistoryDiff(null)
    cancelHistoryDiff()
  }

  async function selectAuditForDiff(event: AuditEvent) {
    if (!snapshot || historyDiffLoading) return
    setHistoryDiffError('')
    if (!historyDiffFromId) {
      setHistoryDiffFromId(event.id)
      return
    }
    setHistoryDiffLoading(true)
    try {
      setHistoryDiffError(t('history.diff.unavailable'))
    } catch (error) {
      setHistoryDiffError(readableError(error))
      setHistoryDiffFromId(null)
    } finally {
      setHistoryDiffLoading(false)
    }
  }

  function toggleAuditEvent(eventId: string) {
    setExpandedAuditIds((current) => {
      const next = new Set(current)
      if (next.has(eventId)) {
        next.delete(eventId)
      } else {
        next.add(eventId)
      }
      return next
    })
  }

  function startAuditNoteEdit(event: AuditEvent) {
    setEditingAuditNoteId(event.id)
    setAuditNoteDraft(event.note_body ?? '')
  }

  async function saveAuditNote(_event: AuditEvent, body = auditNoteDraft) {
    if (!snapshot) return
    // Audit note saving would need a dedicated API endpoint
    setStatus(body.trim() ? t('status.noteSaved') : t('status.noteCleared'))
  }

  if (!identitySaved) {
    return (
      <TooltipProvider>
        <main className="min-h-screen grid place-items-center bg-background animate-in fade-in duration-300">
        <Card className="w-full max-w-md mx-auto">
          <CardContent className="pt-6">
            <form className="flex flex-col gap-4" onSubmit={saveIdentity}>
              <h1 className="text-2xl font-semibold tracking-tight">{t('app.name')}</h1>
              <Input
                autoFocus
                value={identity.nickname}
                onChange={(event) => setIdentity((current) => ({ ...current, nickname: event.target.value }))}
                placeholder={t('identity.nicknamePlaceholder')}
              />
              <ToggleGroup
                type="single"
                value={identity.roleMode}
                onValueChange={(value) => {
                  if (value) setIdentity((current) => ({ ...current, roleMode: value as RoleMode }))
                }}
                className="w-full"
              >
                <ToggleGroupItem value="reviewer" className="flex-1">{t('role.reviewer')}</ToggleGroupItem>
                <ToggleGroupItem value="writer" className="flex-1">{t('role.writer')}</ToggleGroupItem>
              </ToggleGroup>
              <Button disabled={!identity.nickname.trim()}>
                <LogIn className="h-4 w-4 mr-1.5" />
                {t('identity.enter')}
              </Button>
            </form>
          </CardContent>
        </Card>
      </main>
      </TooltipProvider>
    )
  }

  return (
    <TooltipProvider>
    <main className={commentsOpen ? 'workspace' : 'workspace comments-collapsed'}>
      <Button variant="ghost" size="icon" className="sidebar-toggle" onClick={() => setSidebarOpen(true)} aria-label={t('docs.open')}>
        <Menu className="h-4 w-4" />
      </Button>

      <Sheet open={sidebarOpen} onOpenChange={setSidebarOpen}>
        <SheetContent side="left" className="w-[300px] max-w-[calc(100vw-28px)] flex flex-col gap-4 overflow-auto p-6">
          <SheetTitle className="flex items-baseline justify-between gap-3 text-xl font-semibold">
            <span>{t('app.name')}</span>
            <span className="text-xs text-muted-foreground font-normal">{identity.nickname}</span>
          </SheetTitle>
          <SheetDescription className="sr-only">Document list and settings</SheetDescription>

          <ToggleGroup
            type="single"
            value={identity.roleMode}
            onValueChange={(value) => {
              if (!value) return
              localStorage.setItem('documosa.role_mode', value)
              setIdentity((current) => ({ ...current, roleMode: value as RoleMode }))
            }}
            className="w-full"
          >
            <ToggleGroupItem value="reviewer" className="flex-1">{t('role.reviewer')}</ToggleGroupItem>
            <ToggleGroupItem value="writer" className="flex-1">{t('role.writer')}</ToggleGroupItem>
          </ToggleGroup>

          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <Languages className="h-3.5 w-3.5" />
            {t('locale.label')}
          </div>
          <ToggleGroup
            type="single"
            value={locale}
            onValueChange={(value) => {
              if (value) changeLocale(value as Locale)
            }}
            className="w-full"
          >
            <ToggleGroupItem value="zh" className="flex-1">{t('locale.chinese')}</ToggleGroupItem>
            <ToggleGroupItem value="en" className="flex-1">{t('locale.english')}</ToggleGroupItem>
          </ToggleGroup>

          <form className="flex flex-col gap-2.5" onSubmit={createDocument}>
            <Input
              value={draftTitle}
              onChange={(event) => setDraftTitle(event.target.value)}
              placeholder={t('docs.titlePlaceholder')}
            />
            <Button>
              <Upload className="h-4 w-4 mr-1.5" />
              {t('docs.createImport')}
            </Button>
          </form>

          <div className="flex flex-col gap-1.5">
            {pages.map((page) => (
              <Button
                key={page.id}
                variant={snapshot?.page.id === page.id ? 'secondary' : 'ghost'}
                className="w-full justify-start flex-col items-start h-auto gap-0.5 py-2"
                onClick={() => void selectPage(page.id)}
              >
                <span className="font-medium text-sm">{pageTitle(page)}</span>
                <span className="text-xs text-muted-foreground">{formatDate(page.last_edited_time)}</span>
              </Button>
            ))}
          </div>
        </SheetContent>
      </Sheet>

      <section className="editor min-w-0 grid grid-rows-[auto_auto_1fr]">
        <header className="flex items-center justify-between gap-4 pl-16 pr-5 py-4 min-h-[72px] bg-card border-b">
          <div className="min-w-0">
            <h2 className="text-xl font-semibold truncate">{snapshot ? pageTitle(snapshot.page) : t('toolbar.noDocument')}</h2>
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <span>
                {snapshot
                  ? `${t('toolbar.lineCount', { count: activeBlocks.length })} · ${dirty ? t('toolbar.unsaved') : t('toolbar.saved')}`
                  : t('toolbar.openOrCreate')}
              </span>
              {snapshot && onlineUsers.length > 0 && (
                <button
                  type="button"
                  className="flex items-center gap-1 hover:text-foreground transition-colors"
                  onClick={() => setPresenceOpen((open) => !open)}
                >
                  <Users className="h-3 w-3" />
                  <Badge variant="secondary" className="text-[10px] px-1 py-0 h-4">
                    {onlineUsers.length}
                  </Badge>
                </button>
              )}
              {snapshot && snapshot.locks.length > 0 && (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span className="flex items-center gap-1 cursor-help">
                      <Lock className="h-3 w-3" />
                      <Badge variant="secondary" className="text-[10px] px-1 py-0 h-4">
                        {snapshot.locks.length}
                      </Badge>
                    </span>
                  </TooltipTrigger>
                  <TooltipContent side="bottom">
                    {snapshot.locks.map((lock) => (
                      <div key={lock.block_id} className="text-xs">
                        {lock.owner_nickname} · {new Date(lock.expires_at).toLocaleTimeString()}
                      </div>
                    ))}
                  </TooltipContent>
                </Tooltip>
              )}
            </div>
            {presenceOpen && onlineUsers.length > 0 && (
              <div className="mt-1.5 flex flex-wrap gap-1 animate-in fade-in slide-in-from-top-1 duration-200">
                {onlineUsers.map((user) => (
                  <Badge
                    key={user.client_id}
                    variant={user.role_mode === 'writer' ? 'default' : 'outline'}
                    className="text-[10px] px-1.5 py-0 h-5"
                  >
                    {user.nickname}
                  </Badge>
                ))}
              </div>
            )}
          </div>
          <div className="flex gap-2 flex-wrap">
            <Button size="sm" disabled={!snapshot || !dirty || identity.roleMode !== 'writer' || remoteConflict} onClick={() => void saveContent()}>
              <Save className="h-3.5 w-3.5 mr-1.5" />
              {t('toolbar.save')}
            </Button>
            <Button size="sm" variant="outline" disabled={!snapshot} onClick={() => void exportDocument()}>
              <Download className="h-3.5 w-3.5 mr-1.5" />
              {t('toolbar.export')}
            </Button>
            <Button size="sm" variant="outline" disabled={!snapshot} onClick={() => setHistoryOpen(true)}>
              <History className="h-3.5 w-3.5 mr-1.5" />
              {t('toolbar.history')}
            </Button>
            <Button
              size="sm"
              variant="outline"
              aria-controls="review-column"
              aria-expanded={commentsOpen}
              onClick={() => setCommentsOpen((open) => !open)}
            >
              {commentsOpen ? (
                <>
                  <PanelRightClose className="h-3.5 w-3.5 mr-1.5" />
                  {t('review.hidePanel')}
                </>
              ) : (
                <>
                  <PanelRightOpen className="h-3.5 w-3.5 mr-1.5" />
                  {t('review.showPanel')}
                </>
              )}
            </Button>
          </div>
        </header>

        {remoteConflict ? (
          <Alert className="rounded-none border-x-0 border-t-0">
            <AlertDescription className="flex items-center justify-between gap-3">
              <span>{t('conflict.message')}</span>
              <Button variant="outline" size="sm" disabled={!snapshot} onClick={() => snapshot && void refreshSnapshot(snapshot.page.id)}>
                <RefreshCw className="h-3.5 w-3.5 mr-1.5" />
                {t('conflict.refresh')}
              </Button>
            </AlertDescription>
          </Alert>
        ) : null}

        <div className="editor-pane min-h-0 p-4 pb-6 overflow-hidden">
          {snapshot ? (
            <Suspense fallback={<div className="grid place-items-center h-full text-muted-foreground text-sm">{t('editor.loading')}</div>}>
              <TiptapEditor
                key={snapshot.page.id}
                blocks={blocks}
                readOnly={identity.roleMode !== 'writer'}
                onChange={handleEditorChange}
                onSelectionChange={(blockId) => {
                  setSelectedBlockIndex(blockId ? Number(blockId) : null)
                }}
              />
            </Suspense>
          ) : (
            <div className="grid place-items-center h-full text-muted-foreground text-sm">{t('editor.empty')}</div>
          )}
        </div>
      </section>

      <aside
        id="review-column"
        className="review-column min-h-screen min-w-0 flex flex-col gap-4 p-4 overflow-auto bg-card border-l"
        aria-hidden={!commentsOpen}
      >
        <Card>
          <CardContent className="pt-4 flex flex-col gap-2">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">{t('review.cursor')}</h3>
            <p className="text-lg font-semibold">{t('review.line', { number: selectedBlockIndex !== null ? selectedBlockIndex + 1 : 0 })}</p>
            <p className="text-sm whitespace-pre-wrap">{selectedText || (selectedBlock ? blockPreview(selectedBlock, t) : '')}</p>
            {snapshot && (dirty || remoteConflict) ? (
              <span className="text-xs text-muted-foreground">{t('review.saveOrRefresh')}</span>
            ) : null}
            {snapshot && !selectedText && !dirty && !remoteConflict ? (
              <span className="text-xs text-muted-foreground">{t('review.selectText')}</span>
            ) : null}
          </CardContent>
        </Card>

        <div className="flex flex-col gap-3">
          <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">{t('review.comments')}</h3>
          <form className="flex flex-col gap-2" onSubmit={createComment}>
            <Textarea
              value={commentBody}
              onChange={(event) => setCommentBody(event.target.value)}
              placeholder={t('review.commentPlaceholder')}
              className="min-h-[82px]"
            />
            <Button disabled={annotationsDisabled || !commentBody.trim()}>
              <MessageSquarePlus className="h-4 w-4 mr-1.5" />
              {t('review.comment')}
            </Button>
          </form>

          {snapshot?.comments.map((comment) => (
            <Card
              key={comment.id}
              id={`comment-${comment.id}`}
              className={[
                'cursor-pointer',
                comment.resolved ? 'opacity-60' : '',
                activeCommentId === comment.id ? 'ring-2 ring-primary/20 border-primary' : '',
              ].filter(Boolean).join(' ')}
              onClick={() => setActiveCommentId(comment.id)}
            >
              <CardContent className="pt-4 flex flex-col gap-1.5">
                <strong className="text-sm">{comment.author_nickname}</strong>
                <p className="text-sm whitespace-pre-wrap">{comment.body}</p>
                {snapshot.replies
                  .filter((reply) => reply.comment_id === comment.id)
                  .map((reply) => (
                    <p className="text-sm border-l-2 pl-3 text-muted-foreground" key={reply.id}>
                      {reply.author_nickname}: {reply.body}
                    </p>
                  ))}
              </CardContent>
            </Card>
          ))}
        </div>

        {status ? <p className="text-sm text-primary">{status}</p> : null}
      </aside>

      <Sheet open={historyOpen} onOpenChange={(open) => { if (!open) closeHistoryDrawer() }}>
        <SheetContent side="right" className="w-[420px] max-w-full flex flex-col gap-3 overflow-auto p-5">
          <SheetTitle className="text-base font-medium">{t('history.title')}</SheetTitle>
          <SheetDescription className="sr-only">Document history and audit events</SheetDescription>

          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between gap-2">
              <Button variant="ghost" size="sm" className="gap-1" onClick={() => setHistoryFiltersOpen((open) => !open)}>
                <Filter className="h-3.5 w-3.5" />
                {t('history.filters')}
              </Button>
              <Button
                variant={historyDiffMode ? 'default' : 'outline'}
                size="sm"
                className="gap-1"
                onClick={() => {
                  if (historyDiffMode) {
                    cancelHistoryDiff()
                  } else {
                    setHistoryDiffMode(true)
                    setHistoryDiffFromId(null)
                    setHistoryDiffError('')
                  }
                }}
              >
                {historyDiffMode ? <X className="h-3.5 w-3.5" /> : <ArrowLeftRight className="h-3.5 w-3.5" />}
                {historyDiffMode ? t('history.diff.cancel') : t('history.diff.button')}
              </Button>
            </div>

            {historyFiltersOpen && (
              <div className="flex flex-col gap-2.5 p-3 rounded-lg border bg-muted/50">
                <label className="flex flex-col gap-1 text-xs text-muted-foreground">
                  {t('history.category')}
                  <Select value={historyCategory} onValueChange={(value) => setHistoryCategory(value as AuditCategory)}>
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {AUDIT_CATEGORIES.map((category) => (
                        <SelectItem key={category} value={category}>
                          {t(`history.categories.${category}`)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </label>
                <label className="flex flex-col gap-1 text-xs text-muted-foreground">
                  {t('history.from')}
                  <Input type="datetime-local" value={historyFrom} onChange={(event) => setHistoryFrom(event.target.value)} />
                </label>
                <label className="flex flex-col gap-1 text-xs text-muted-foreground">
                  {t('history.to')}
                  <Input type="datetime-local" value={historyTo} onChange={(event) => setHistoryTo(event.target.value)} />
                </label>
                <Button variant="ghost" size="sm" onClick={clearHistoryFilters}>
                  {t('history.clearFilters')}
                </Button>
              </div>
            )}

            {historyDiffMode && (
              <p className={historyDiffError ? 'text-sm text-destructive bg-destructive/10 p-2 rounded-md' : 'text-sm text-muted-foreground bg-muted p-2 rounded-md'}>
                {historyDiffError ||
                  (historyDiffLoading
                    ? t('history.diff.loading')
                    : historyDiffFromId
                      ? t('history.diff.pickEnd')
                      : t('history.diff.pickStart'))}
              </p>
            )}
          </div>

          {filteredAuditEvents.length === 0 && (
            <p className="text-sm text-muted-foreground text-center py-6">{t('history.empty')}</p>
          )}

          {filteredAuditEvents.map((event) => {
            const formatted = formatAuditEvent(event, t, auditFormatContext)
            const expanded = expandedAuditIds.has(event.id)
            const expandable = formatted.body.length > AUDIT_COLLAPSE_LIMIT
            const body = expandable && !expanded ? truncateAuditText(formatted.body) : formatted.body
            const diffSelected = historyDiffFromId === event.id
            return (
              <div
                key={event.id}
                data-audit-id={event.id}
                className={[
                  'flex flex-col gap-1.5 p-3 rounded-lg border text-sm',
                  historyDiffMode ? 'cursor-pointer hover:border-primary hover:bg-primary/5' : '',
                  diffSelected ? 'border-primary bg-primary/5' : 'bg-card',
                ].filter(Boolean).join(' ')}
                tabIndex={0}
                onClick={() => {
                  if (historyDiffMode) {
                    void selectAuditForDiff(event)
                    return
                  }
                  if (expandable) toggleAuditEvent(event.id)
                }}
                onKeyDown={(keyboardEvent) => {
                  if (keyboardEvent.key === 'Enter' || keyboardEvent.key === ' ') {
                    keyboardEvent.preventDefault()
                    if (historyDiffMode) {
                      void selectAuditForDiff(event)
                    } else if (expandable) {
                      toggleAuditEvent(event.id)
                    }
                  }
                }}
              >
                <div className="flex items-baseline justify-between gap-3">
                  <strong className="text-sm font-medium">{formatted.title}</strong>
                  {editingAuditNoteId === event.id ? null : (
                    <button
                      type="button"
                      className="text-xs text-primary underline truncate max-w-[46%] text-right"
                      onClick={(clickEvent) => {
                        clickEvent.stopPropagation()
                        startAuditNoteEdit(event)
                      }}
                      onKeyDown={(keyboardEvent) => keyboardEvent.stopPropagation()}
                    >
                      {event.note_body || t('history.note.add')}
                    </button>
                  )}
                </div>
                <span className="text-xs text-muted-foreground">
                  {event.actor_nickname} · {formatDate(event.created_at)}
                </span>
                {editingAuditNoteId === event.id && (
                  <div className="flex flex-col gap-2" onClick={(clickEvent) => clickEvent.stopPropagation()}>
                    <Textarea
                      value={auditNoteDraft}
                      maxLength={2000}
                      onChange={(changeEvent) => setAuditNoteDraft(changeEvent.target.value)}
                      placeholder={t('history.note.placeholder')}
                      className="min-h-[72px]"
                    />
                    <div className="flex gap-2">
                      <Button size="sm" onClick={() => void saveAuditNote(event)}>{t('history.note.save')}</Button>
                      <Button size="sm" variant="ghost" onClick={() => setEditingAuditNoteId(null)}>{t('history.note.cancel')}</Button>
                      <Button size="sm" variant="ghost" onClick={() => void saveAuditNote(event, '')}>{t('history.note.clear')}</Button>
                    </div>
                  </div>
                )}
                {body ? <p className="text-sm text-foreground whitespace-pre-wrap overflow-wrap-anywhere">{body}</p> : null}
                {expandable && (
                  <button
                    type="button"
                    className="text-xs text-primary underline self-start"
                    onClick={(clickEvent) => {
                      clickEvent.stopPropagation()
                      toggleAuditEvent(event.id)
                    }}
                  >
                    {expanded ? t('history.showLess') : t('history.showMore')}
                  </button>
                )}
              </div>
            )
          })}
        </SheetContent>
      </Sheet>

      {historyDiff ? (
        <HistoryDiffModal
          diff={historyDiff}
          formatDate={formatDate}
          onClose={closeHistoryDiff}
        />
      ) : null}
    </main>
    </TooltipProvider>
  )
}

export default App
