import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { FormEvent } from 'react'
import AceDiff from 'ace-diff'
import * as ace from 'ace-builds'
import 'ace-builds/src-noconflict/mode-markdown'
import 'ace-builds/src-noconflict/theme-textmate'
import 'ace-diff/styles.css'
import { useTranslation } from 'react-i18next'
import type { CherryInstance, CommentRange, EditorSelection } from './CherryEditor'
import type { Locale } from './i18n'
import './App.css'

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
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  ToggleGroup,
  ToggleGroupItem,
} from '@/components/ui/toggle-group'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Card, CardContent } from '@/components/ui/card'
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
} from 'lucide-react'

type RoleMode = 'reviewer' | 'writer'

type DocumentSummary = {
  id: string
  title: string
  created_at: string
  updated_at: string
}

type Line = {
  id: string
  document_id: string
  order_index: number
  content: string
  revision: number
  deleted: boolean
}

type LineLock = {
  line_id: string
  owner_client_id: string
  owner_nickname: string
  expires_at: string
}

type Comment = {
  id: string
  start_line_id: string
  end_line_id: string
  start_column?: number | null
  end_column?: number | null
  author_client_id: string
  author_nickname: string
  role_mode: RoleMode
  body: string
  resolved: boolean
}

type CommentReply = {
  id: string
  comment_id: string
  author_nickname: string
  role_mode: RoleMode
  body: string
}

type AuditEvent = {
  id: string
  document_id?: string
  actor_client_id?: string
  actor_nickname: string
  role_mode: RoleMode
  event_type: string
  details_json: string
  created_at: string
  note_body?: string | null
  note_updated_by_nickname?: string | null
  note_updated_at?: string | null
}

type Snapshot = {
  document: DocumentSummary
  lines: Line[]
  locks: LineLock[]
  comments: Comment[]
  replies: CommentReply[]
  suggestions: unknown[]
  audit_events: AuditEvent[]
}

type HistoryDiffResponse = {
  from_event: AuditEvent
  to_event: AuditEvent
  from_content: string
  to_content: string
}

type Identity = {
  clientId: string
  nickname: string
  roleMode: RoleMode
}

type BaseRevision = {
  line_id: string
  revision: number
}

const API = ''
const CherryEditor = lazy(() => import('./CherryEditor'))
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

function snapshotText(lines: Line[]) {
  return lines
    .filter((line) => !line.deleted)
    .map((line) => line.content)
    .join('\n')
}

function linePreview(line: Line | undefined, t: Translation) {
  if (!line) return t('review.noLine')
  return line.content.trim() || t('review.blankLine')
}

type AuditCategory = 'document_comment' | 'all' | 'content' | 'comment' | 'suggestion' | 'system'

type AuditDetails = Record<string, unknown>

const AUDIT_CATEGORIES: AuditCategory[] = ['document_comment', 'all', 'content', 'comment', 'suggestion', 'system']
const AUDIT_COLLAPSE_LIMIT = 260

type AuditFormatContext = {
  lines: Map<string, Line>
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
  if (eventType === 'document.created' || eventType === 'document.content_updated' || eventType.startsWith('lines.')) return 'content'
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
      const line = context.lines.get(lineId)
      if (!line) return null
      return {
        line_id: line.id,
        content_summary: contentSummary(line.content),
      }
    })
    .filter((line): line is { line_id: string; content_summary: string } => line !== null)
}

function describeLegacyLineIds(lineIds: string[], context: AuditFormatContext, t: Translation, mode: 'inserted' | 'deleted') {
  const lines = lineDetailsFromIds(lineIds, context)
  if (lines.length === 0) return lineIds.map((lineId) => lineLabel(lineId, context, t)).join('\n')
  return describeAuditLines({ lines }, t, context, mode)
}

function describeLegacyReplacedLineIds(lineIds: string[], context: AuditFormatContext, t: Translation) {
  const lines = lineIds
    .map((lineId) => {
      const line = context.lines.get(lineId)
      if (!line) return null
      return t('history.line.current', {
        line: lineLabel(lineId, context, t),
        summary: contentSummary(line.content),
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
          count: detailNumber(details, 'line_count') ?? context.lines.size,
          summary: detailString(details, 'initial_content_summary'),
        }),
      }
    case 'document.content_updated':
      if (detailNumber(details, 'inserted_count') === undefined || detailNumber(details, 'deleted_count') === undefined) {
        const insertedLineIds = detailStringArray(details, 'inserted_line_ids')
        const deletedLineIds = detailStringArray(details, 'deleted_line_ids')
        return {
          category,
          title: t('history.events.documentContentUpdated'),
          body: [
            t('history.summary.documentContentUpdated', {
              inserted: insertedLineIds.length,
              deleted: deletedLineIds.length,
            }),
            describeLegacyLineIds(insertedLineIds, context, t, 'inserted'),
            describeLegacyLineIds(deletedLineIds, context, t, 'deleted'),
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
            inserted: detailNumber(details, 'inserted_count') ?? detailArray(details, 'inserted_line_ids').length,
            deleted: detailNumber(details, 'deleted_count') ?? detailArray(details, 'deleted_line_ids').length,
          }),
          describeAuditLines({ lines: detailArray(details, 'inserted_lines') }, t, context, 'inserted'),
          describeAuditLines({ lines: detailArray(details, 'deleted_lines') }, t, context, 'deleted'),
        ]
          .filter(Boolean)
          .join('\n'),
      }
    case 'lines.inserted':
      if (detailArray(details, 'lines').length === 0) {
        const lineIds = detailStringArray(details, 'line_ids')
        return {
          category,
          title: t('history.events.linesInserted', { count: lineIds.length }),
          body: describeLegacyLineIds(lineIds, context, t, 'inserted'),
        }
      }
      return {
        category,
        title: t('history.events.linesInserted', { count: detailNumber(details, 'count') ?? detailArray(details, 'line_ids').length }),
        body: describeAuditLines(details, t, context, 'inserted'),
      }
    case 'lines.replaced':
      if (detailArray(details, 'lines').length === 0) {
        const lineIds = detailStringArray(details, 'line_ids')
        return {
          category,
          title: t('history.events.linesReplaced', { count: lineIds.length }),
          body: describeLegacyReplacedLineIds(lineIds, context, t),
        }
      }
      return {
        category,
        title: t('history.events.linesReplaced', { count: detailNumber(details, 'count') ?? detailArray(details, 'line_ids').length }),
        body: describeAuditReplacements(details, t, context),
      }
    case 'lines.deleted':
      if (detailArray(details, 'lines').length === 0) {
        const lineIds = detailStringArray(details, 'line_ids')
        return {
          category,
          title: t('history.events.linesDeleted', { count: lineIds.length }),
          body: describeLegacyLineIds(lineIds, context, t, 'deleted'),
        }
      }
      return {
        category,
        title: t('history.events.linesDeleted', { count: detailNumber(details, 'count') ?? detailArray(details, 'deleted_line_ids').length }),
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
  closeLabel,
}: {
  diff: HistoryDiffResponse
  formatDate: (value: string) => string
  onClose: () => void
  closeLabel: string
}) {
  const containerRef = useRef<HTMLDivElement | null>(null)
  const title = `Diff: ${formatDate(diff.from_event.created_at)} -> ${formatDate(diff.to_event.created_at)}`

  useEffect(() => {
    const container = containerRef.current
    if (!container) return undefined
    const aceDiff = new AceDiff({
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
    return () => aceDiff.destroy()
  }, [diff])

  return (
    <Dialog open onOpenChange={onClose}>
      <DialogContent className="max-w-[1120px] w-[96vw] max-h-[760px] h-[92vh] flex flex-col gap-3 p-5" aria-label={title}>
        <DialogHeader className="flex flex-row items-center justify-between gap-3">
          <DialogTitle className="text-lg">{title}</DialogTitle>
          <Button variant="ghost" size="icon-sm" onClick={onClose} aria-label={closeLabel}>
            <X className="h-4 w-4" />
          </Button>
        </DialogHeader>
        <div className="flex justify-between text-xs text-muted-foreground px-1">
          <span>{diff.from_event.event_type}</span>
          <span>{diff.to_event.event_type}</span>
        </div>
        <div className="history-diff-view flex-1 min-h-0" ref={containerRef} />
      </DialogContent>
    </Dialog>
  )
}

function lineIdIndex(lines: Line[]) {
  return new Map(lines.map((line, index) => [line.id, index]))
}

function commentRangeForEditor(comment: Comment, lines: Line[], indexes: Map<string, number>) {
  const startIndex = indexes.get(comment.start_line_id)
  const endIndex = indexes.get(comment.end_line_id)
  if (startIndex === undefined || endIndex === undefined) return null
  const firstIndex = Math.min(startIndex, endIndex)
  const lastIndex = Math.max(startIndex, endIndex)
  let lineStartOffset = 0
  for (let index = 0; index < firstIndex; index += 1) {
    lineStartOffset += lines[index].content.length + 1
  }
  const spanLength = lines
    .slice(firstIndex, lastIndex + 1)
    .reduce((sum, line, index) => sum + line.content.length + (index < lastIndex - firstIndex ? 1 : 0), 0)
  let from = lineStartOffset
  let to = lineStartOffset + spanLength
  if (startIndex <= endIndex && comment.start_column !== null && comment.start_column !== undefined) {
    from = lineStartOffset + Math.max(0, Math.min(comment.start_column, lines[firstIndex].content.length))
  }
  if (startIndex <= endIndex && comment.end_column !== null && comment.end_column !== undefined) {
    const beforeEndLine = lines
      .slice(firstIndex, lastIndex)
      .reduce((sum, line) => sum + line.content.length + 1, 0)
    to = lineStartOffset + beforeEndLine + Math.max(0, Math.min(comment.end_column, lines[lastIndex].content.length))
  }
  return to > from ? { id: comment.id, from, to } : null
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
  const [documents, setDocuments] = useState<DocumentSummary[]>([])
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null)
  const [editorText, setEditorText] = useState('')
  const [editorResetToken, setEditorResetToken] = useState(0)
  const [selection, setSelection] = useState<EditorSelection>({
    startLineNumber: 1,
    endLineNumber: 1,
    startColumn: 0,
    endColumn: 0,
    text: '',
  })
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
  const [draftContent, setDraftContent] = useState('')
  const [commentBody, setCommentBody] = useState('')
  const [activeCommentId, setActiveCommentId] = useState<string | null>(null)
  const [flashingCommentId, setFlashingCommentId] = useState<string | null>(null)
  const [status, setStatus] = useState('')
  const cherryRef = useRef<CherryInstance | null>(null)
  const dirtyRef = useRef(false)

  useEffect(() => {
    dirtyRef.current = dirty
  }, [dirty])

  useEffect(() => {
    localStorage.setItem(COMMENTS_OPEN_KEY, String(commentsOpen))
  }, [commentsOpen])

  const headers = useMemo(
    () => ({
      'content-type': 'application/json',
      'x-documosa-client-id': identity.clientId,
      'x-documosa-nickname': identity.nickname,
      'x-documosa-role-mode': identity.roleMode,
    }),
    [identity],
  )

  const request = useCallback(
    async <T,>(path: string, init: RequestInit = {}) => {
      const response = await fetch(`${API}${path}`, {
        ...init,
        headers: { ...headers, ...(init.headers ?? {}) },
      })
      if (!response.ok) {
        const text = await response.text()
        throw new Error(text)
      }
      const contentType = response.headers.get('content-type') ?? ''
      if (!contentType.includes('application/json')) {
        return (await response.text()) as T
      }
      return (await response.json()) as T
    },
    [headers],
  )

  const activeLines = useMemo(() => snapshot?.lines.filter((line) => !line.deleted) ?? [], [snapshot])
  const selectedLineIndex = Math.min(Math.max(selection.startLineNumber, 1), Math.max(activeLines.length, 1)) - 1
  const selectedLine = activeLines[selectedLineIndex]
  const baseEditorText = useMemo(() => snapshotText(activeLines), [activeLines])
  const baseRevisions: BaseRevision[] = useMemo(
    () => activeLines.map((line) => ({ line_id: line.id, revision: line.revision })),
    [activeLines],
  )
  const lineIndexes = useMemo(() => lineIdIndex(activeLines), [activeLines])
  const commentRanges = useMemo(
    () =>
      snapshot?.comments
        .filter((comment) => !comment.resolved)
        .map((comment) => commentRangeForEditor(comment, activeLines, lineIndexes))
        .filter((range): range is CommentRange => range !== null) ?? [],
    [activeLines, lineIndexes, snapshot?.comments],
  )
  const selectedText = selection.text.trim()
  const annotationsDisabled = dirty || remoteConflict || !selectedLine || !selectedText
  const auditFormatContext = useMemo<AuditFormatContext>(
    () => ({
      lines: new Map(snapshot?.lines.map((line) => [line.id, line]) ?? []),
      activeLineNumberById: new Map(activeLines.map((line, index) => [line.id, index + 1])),
      comments: new Map(snapshot?.comments.map((comment) => [comment.id, comment]) ?? []),
      replies: new Map(snapshot?.replies.map((reply) => [reply.id, reply]) ?? []),
    }),
    [activeLines, snapshot?.comments, snapshot?.lines, snapshot?.replies],
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

  const applySnapshot = useCallback((next: Snapshot, resetEditor: boolean) => {
    setSnapshot(next)
    setExpandedAuditIds(new Set())
    setEditingAuditNoteId(null)
    if (resetEditor) {
      setEditorText(snapshotText(next.lines))
      setEditorResetToken((current) => current + 1)
      setDirty(false)
      setRemoteConflict(false)
      setSelection({ startLineNumber: 1, endLineNumber: 1, startColumn: 0, endColumn: 0, text: '' })
    }
  }, [])

  const refreshDocuments = useCallback(async () => {
    const nextDocuments = await request<DocumentSummary[]>('/api/documents')
    setDocuments(nextDocuments)
  }, [request])

  const refreshSnapshot = useCallback(
    async (documentId: string, resetEditor = true) => {
      const next = await request<Snapshot>(`/api/documents/${documentId}`)
      applySnapshot(next, resetEditor)
    },
    [applySnapshot, request],
  )

  useEffect(() => {
    if (!identity.nickname) return undefined
    let cancelled = false
    void request<DocumentSummary[]>('/api/documents')
      .then((nextDocuments) => {
        if (!cancelled) setDocuments(nextDocuments)
      })
      .catch((error) => {
        if (!cancelled) setStatus(error.message)
      })
    return () => {
      cancelled = true
    }
  }, [identity.nickname, request])

  useEffect(() => {
    if (!snapshot || !identity.nickname) return undefined
    const scheme = location.protocol === 'https:' ? 'wss' : 'ws'
    const url = `${scheme}://${location.host}/api/documents/${snapshot.document.id}/ws?client_id=${encodeURIComponent(identity.clientId)}&nickname=${encodeURIComponent(identity.nickname)}&role_mode=${identity.roleMode}`
    const socket = new WebSocket(url)
    socket.onmessage = (message) => {
      let topic = ''
      try {
        const event = JSON.parse(message.data) as { type?: string; topic?: string }
        if (event.type === 'document_changed') topic = event.topic ?? ''
      } catch {
        topic = ''
      }
      void request<Snapshot>(`/api/documents/${snapshot.document.id}`)
        .then((next) => {
          void refreshDocuments().catch(() => undefined)
          if (topic === 'audit.note.updated') {
            applySnapshot(next, false)
            return
          }
          if (dirtyRef.current) {
            setRemoteConflict(true)
            setStatus(t('conflict.message'))
            return
          }
          applySnapshot(next, true)
        })
        .catch((error) => setStatus(error.message))
    }
    return () => socket.close()
  }, [applySnapshot, identity, refreshDocuments, request, snapshot, t])

  function saveIdentity(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    localStorage.setItem('documosa.nickname', identity.nickname)
    localStorage.setItem('documosa.role_mode', identity.roleMode)
    setIdentitySaved(true)
  }

  async function createDocument(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const next = await request<Snapshot>('/api/documents', {
      method: 'POST',
      body: JSON.stringify({ title: draftTitle || 'Untitled', content: draftContent }),
    })
    setDraftTitle('')
    setDraftContent('')
    await refreshDocuments()
    applySnapshot(next, true)
    setSidebarOpen(false)
    setStatus(t('status.documentCreated'))
  }

  async function selectDocument(documentId: string) {
    if (dirty && !confirm(t('confirm.discardUnsaved'))) return
    await refreshSnapshot(documentId)
    setSidebarOpen(false)
  }

  async function exportDocument() {
    if (!snapshot) return
    const text = await request<string>(`/api/documents/${snapshot.document.id}/export`)
    const blob = new Blob([text], { type: 'text/plain' })
    const href = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = href
    anchor.download = `${snapshot.document.title}.txt`
    anchor.click()
    URL.revokeObjectURL(href)
  }

  async function saveContent() {
    if (!snapshot || identity.roleMode !== 'writer' || remoteConflict) return
    const content = cherryRef.current?.getMarkdown() ?? editorText
    const next = await request<Snapshot>(`/api/documents/${snapshot.document.id}/content`, {
      method: 'PUT',
      body: JSON.stringify({ content, base_revisions: baseRevisions }),
    })
    await refreshDocuments()
    applySnapshot(next, true)
    setStatus(t('status.saved'))
  }

  async function createComment(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const endLine = activeLines[Math.min(Math.max(selection.endLineNumber, 1), Math.max(activeLines.length, 1)) - 1]
    if (!snapshot || !selectedLine || !endLine || annotationsDisabled || !commentBody.trim()) return
    const next = await request<Snapshot>(`/api/documents/${snapshot.document.id}/comments`, {
      method: 'POST',
      body: JSON.stringify({
        start_line_id: selectedLine.id,
        end_line_id: endLine.id,
        start_column: selection.startColumn,
        end_column: selection.endColumn,
        body: commentBody,
      }),
    })
    setCommentBody('')
    applySnapshot(next, false)
  }

  function flashComment(commentId: string) {
    setActiveCommentId(commentId)
    setFlashingCommentId(commentId)
    window.setTimeout(() => {
      document.getElementById(`comment-${commentId}`)?.scrollIntoView({ block: 'nearest' })
    })
    window.setTimeout(() => setFlashingCommentId((current) => (current === commentId ? null : current)), 900)
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
      const nextDiff = await request<HistoryDiffResponse>(
        `/api/documents/${snapshot.document.id}/history-diff?from=${encodeURIComponent(historyDiffFromId)}&to=${encodeURIComponent(event.id)}`,
      )
      setHistoryDiff(nextDiff)
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

  async function saveAuditNote(event: AuditEvent, body = auditNoteDraft) {
    if (!snapshot) return
    const next = await request<Snapshot>(`/api/documents/${snapshot.document.id}/audit-events/${event.id}/note`, {
      method: 'PUT',
      body: JSON.stringify({ body }),
    })
    applySnapshot(next, false)
    setStatus(body.trim() ? t('status.noteSaved') : t('status.noteCleared'))
  }

  if (!identitySaved) {
    return (
      <main className="identity-screen min-h-screen grid place-items-center bg-background">
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
    )
  }

  return (
    <main className={commentsOpen ? 'workspace' : 'workspace comments-collapsed'}>
      <Button variant="ghost" size="icon" className="sidebar-toggle" onClick={() => setSidebarOpen(true)} aria-label={t('docs.open')}>
        <Menu className="h-4 w-4" />
      </Button>

      <Sheet open={sidebarOpen} onOpenChange={setSidebarOpen}>
        <SheetContent side="left" className="w-[300px] max-w-[calc(100vw-28px)] flex flex-col gap-4 overflow-auto">
          <SheetHeader className="px-0">
            <div className="flex items-baseline justify-between gap-3">
              <SheetTitle className="text-xl font-semibold">{t('app.name')}</SheetTitle>
              <span className="text-xs text-muted-foreground">{identity.nickname}</span>
            </div>
          </SheetHeader>

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
            <Textarea
              value={draftContent}
              onChange={(event) => setDraftContent(event.target.value)}
              placeholder={t('docs.contentPlaceholder')}
              className="min-h-[82px]"
            />
            <Button>
              <Upload className="h-4 w-4 mr-1.5" />
              {t('docs.createImport')}
            </Button>
          </form>

          <div className="flex flex-col gap-1.5">
            {documents.map((document) => (
              <Button
                key={document.id}
                variant={snapshot?.document.id === document.id ? 'secondary' : 'ghost'}
                className="w-full justify-start flex-col items-start h-auto gap-0.5 py-2"
                onClick={() => void selectDocument(document.id)}
              >
                <span className="font-medium text-sm">{document.title}</span>
                <span className="text-xs text-muted-foreground">{formatDate(document.updated_at)}</span>
              </Button>
            ))}
          </div>
        </SheetContent>
      </Sheet>

      <section className="editor min-w-0 grid grid-rows-[auto_auto_1fr]">
        <header className="flex items-center justify-between gap-4 px-5 py-4 min-h-[72px] bg-card border-b">
          <div className="min-w-0">
            <h2 className="text-xl font-semibold truncate">{snapshot?.document.title ?? t('toolbar.noDocument')}</h2>
            <span className="text-xs text-muted-foreground">
              {snapshot
                ? `${t('toolbar.lineCount', { count: activeLines.length })} · ${dirty ? t('toolbar.unsaved') : t('toolbar.saved')}`
                : t('toolbar.openOrCreate')}
            </span>
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
          <Alert className="flex items-center justify-between gap-3 rounded-none border-x-0 border-t-0 border-amber-200 bg-amber-50 text-amber-900">
            <AlertDescription>{t('conflict.message')}</AlertDescription>
            <Button variant="outline" size="sm" disabled={!snapshot} onClick={() => snapshot && void refreshSnapshot(snapshot.document.id)}>
              <RefreshCw className="h-3.5 w-3.5 mr-1.5" />
              {t('conflict.refresh')}
            </Button>
          </Alert>
        ) : null}

        <div className="editor-pane min-h-0 p-4 pb-6 overflow-hidden">
          {snapshot ? (
            <Suspense fallback={<div className="grid place-items-center h-full text-muted-foreground text-sm">{t('editor.loading')}</div>}>
              <CherryEditor
                key={snapshot.document.id}
                documentId={snapshot.document.id}
                value={baseEditorText}
                resetToken={editorResetToken}
                readOnly={identity.roleMode !== 'writer'}
                commentRanges={commentRanges}
                onReady={(instance) => {
                  cherryRef.current = instance
                }}
                onMarkdownChange={setEditorText}
                onDirtyChange={setDirty}
                onSelectionChange={setSelection}
                onCommentClick={flashComment}
              />
            </Suspense>
          ) : (
            <div className="grid place-items-center h-full text-muted-foreground text-sm">{t('editor.empty')}</div>
          )}
        </div>
      </section>

      <aside
        id="review-column"
        className="min-h-screen min-w-0 flex flex-col gap-4 p-4 overflow-auto bg-card border-l"
        style={{ opacity: commentsOpen ? 1 : 0, pointerEvents: commentsOpen ? 'auto' : 'none', transform: commentsOpen ? 'translateX(0)' : 'translateX(18px)', transition: 'opacity 220ms, transform 220ms' }}
        aria-hidden={!commentsOpen}
      >
        <Card>
          <CardContent className="pt-4 flex flex-col gap-2">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">{t('review.cursor')}</h3>
            <p className="text-lg font-semibold">{t('review.line', { number: selectedLine ? selectedLineIndex + 1 : 0 })}</p>
            <p className="text-sm whitespace-pre-wrap">{selectedText || linePreview(selectedLine, t)}</p>
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
                flashingCommentId === comment.id ? 'flash-comment' : '',
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
        <SheetContent side="right" className="w-[420px] max-w-full flex flex-col gap-3 overflow-auto">
          <SheetHeader className="px-0 pb-0">
            <div className="flex items-center justify-between gap-3">
              <SheetTitle className="text-base font-medium">{t('history.title')}</SheetTitle>
              <Button variant="ghost" size="icon-sm" onClick={closeHistoryDrawer} aria-label={t('history.close')}>
                <X className="h-4 w-4" />
              </Button>
            </div>
          </SheetHeader>

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
                  {event.actor_nickname} · {t(`role.${event.role_mode}`)} · {t(`history.categories.${formatted.category}`)} · {formatDate(event.created_at)}
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
          closeLabel={t('history.diff.close')}
          onClose={closeHistoryDiff}
        />
      ) : null}
    </main>
  )
}

export default App
