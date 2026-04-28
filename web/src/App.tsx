import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { FormEvent, ReactNode } from 'react'
import AceDiff from 'ace-diff'
import * as ace from 'ace-builds'
import 'ace-builds/src-noconflict/mode-markdown'
import 'ace-builds/src-noconflict/theme-textmate'
import 'ace-diff/styles.css'
import { useTranslation } from 'react-i18next'
import type { CherryInstance, CommentRange, EditorSelection } from './CherryEditor'
import type { Locale } from './i18n'
import './App.css'

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
        title: t('history.events.linesDeleted', { count: detailNumber(details, 'count') ?? detailArray(details, 'line_ids').length }),
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


function Icon({ name }: { name: string }) {
  return <span className="material-symbols-outlined icon" aria-hidden="true">{name}</span>
}

function ButtonWithIcon({
  icon,
  children,
  className,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { icon: string; children: ReactNode }) {
  return (
    <button {...props} className={["with-icon", className].filter(Boolean).join(' ')}>
      <Icon name={icon} />
      <span>{children}</span>
    </button>
  )
}

function IconButton({ icon, label, className, ...props }: React.ButtonHTMLAttributes<HTMLButtonElement> & { icon: string; label: string }) {
  return (
    <button {...props} className={["icon-button", className].filter(Boolean).join(' ')} aria-label={label} title={label}>
      <Icon name={icon} />
    </button>
  )
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

  useEffect(() => {
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', closeOnEscape)
    return () => window.removeEventListener('keydown', closeOnEscape)
  }, [onClose])

  return (
    <div className="history-diff-backdrop" role="dialog" aria-modal="true" aria-label={title}>
      <section className="history-diff-modal">
        <header>
          <h3>{title}</h3>
          <IconButton icon="close" label={closeLabel} onClick={onClose} />
        </header>
        <div className="history-diff-labels" aria-hidden="true">
          <span>{diff.from_event.event_type}</span>
          <span>{diff.to_event.event_type}</span>
        </div>
        <div className="history-diff-view" ref={containerRef} />
      </section>
    </div>
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
      <main className="identity-screen">
        <form className="identity-panel" onSubmit={saveIdentity}>
          <h1>{t('app.name')}</h1>
          <input
            autoFocus
            value={identity.nickname}
            onChange={(event) => setIdentity((current) => ({ ...current, nickname: event.target.value }))}
            placeholder={t('identity.nicknamePlaceholder')}
          />
          <div className="segmented">
            <button
              type="button"
              className={identity.roleMode === 'reviewer' ? 'active' : ''}
              onClick={() => setIdentity((current) => ({ ...current, roleMode: 'reviewer' }))}
            >
              {t('role.reviewer')}
            </button>
            <button
              type="button"
              className={identity.roleMode === 'writer' ? 'active' : ''}
              onClick={() => setIdentity((current) => ({ ...current, roleMode: 'writer' }))}
            >
              {t('role.writer')}
            </button>
          </div>
          <ButtonWithIcon icon="login" className="primary" disabled={!identity.nickname.trim()}>
            {t('identity.enter')}
          </ButtonWithIcon>
        </form>
      </main>
    )
  }

  return (
    <main className={commentsOpen ? 'workspace' : 'workspace comments-collapsed'}>
      <IconButton className="sidebar-toggle" icon="menu" label={t('docs.open')} onClick={() => setSidebarOpen(true)} />
      <button
        className={sidebarOpen ? 'sheet-backdrop docs-backdrop open' : 'sheet-backdrop docs-backdrop'}
        aria-label={t('docs.close')}
        aria-hidden={!sidebarOpen}
        tabIndex={sidebarOpen ? 0 : -1}
        onClick={() => setSidebarOpen(false)}
      />
      <aside className={sidebarOpen ? 'documents open' : 'documents'}>
        <div className="brand">
          <h1>{t('app.name')}</h1>
          <span>{identity.nickname}</span>
        </div>
        <div className="segmented">
          {(['reviewer', 'writer'] as RoleMode[]).map((mode) => (
            <button
              key={mode}
              className={identity.roleMode === mode ? 'active' : ''}
              onClick={() => {
                localStorage.setItem('documosa.role_mode', mode)
                setIdentity((current) => ({ ...current, roleMode: mode }))
              }}
            >
              {t(`role.${mode}`)}
            </button>
          ))}
        </div>
        <section className="locale-switcher" aria-label={t('locale.label')}>
          <span><Icon name="translate" /> {t('locale.label')}</span>
          <div className="segmented compact">
            <button className={locale === 'zh' ? 'active' : ''} onClick={() => changeLocale('zh')}>
              {t('locale.chinese')}
            </button>
            <button className={locale === 'en' ? 'active' : ''} onClick={() => changeLocale('en')}>
              {t('locale.english')}
            </button>
          </div>
        </section>
        <form className="create-doc" onSubmit={createDocument}>
          <input value={draftTitle} onChange={(event) => setDraftTitle(event.target.value)} placeholder={t('docs.titlePlaceholder')} />
          <textarea
            value={draftContent}
            onChange={(event) => setDraftContent(event.target.value)}
            placeholder={t('docs.contentPlaceholder')}
          />
          <ButtonWithIcon icon="upload_file" className="primary">
            {t('docs.createImport')}
          </ButtonWithIcon>
        </form>
        <div className="doc-list">
          {documents.map((document) => (
            <button
              key={document.id}
              className={snapshot?.document.id === document.id ? 'doc active' : 'doc'}
              onClick={() => void selectDocument(document.id)}
            >
              <strong>{document.title}</strong>
              <span>{formatDate(document.updated_at)}</span>
            </button>
          ))}
        </div>
      </aside>

      <section className="editor">
        <header className="toolbar">
          <div>
            <h2>{snapshot?.document.title ?? t('toolbar.noDocument')}</h2>
            <span>
              {snapshot
                ? `${t('toolbar.lineCount', { count: activeLines.length })} · ${dirty ? t('toolbar.unsaved') : t('toolbar.saved')}`
                : t('toolbar.openOrCreate')}
            </span>
          </div>
          <div className="toolbar-actions">
            <ButtonWithIcon icon="save" disabled={!snapshot || !dirty || identity.roleMode !== 'writer' || remoteConflict} onClick={() => void saveContent()}>
              {t('toolbar.save')}
            </ButtonWithIcon>
            <ButtonWithIcon icon="download" disabled={!snapshot} onClick={() => void exportDocument()}>
              {t('toolbar.export')}
            </ButtonWithIcon>
            <ButtonWithIcon icon="history" disabled={!snapshot} onClick={() => setHistoryOpen(true)}>
              {t('toolbar.history')}
            </ButtonWithIcon>
            <ButtonWithIcon
              icon={commentsOpen ? 'right_panel_close' : 'right_panel_open'}
              aria-controls="review-column"
              aria-expanded={commentsOpen}
              onClick={() => setCommentsOpen((open) => !open)}
            >
              {commentsOpen ? t('review.hidePanel') : t('review.showPanel')}
            </ButtonWithIcon>
          </div>
        </header>
        {remoteConflict ? (
          <div className="conflict-bar">
            {t('conflict.message')}
            <ButtonWithIcon icon="refresh" disabled={!snapshot} onClick={() => snapshot && void refreshSnapshot(snapshot.document.id)}>
              {t('conflict.refresh')}
            </ButtonWithIcon>
          </div>
        ) : null}
        <div className="editor-pane">
          {snapshot ? (
            <Suspense fallback={<div className="empty-state compact">{t('editor.loading')}</div>}>
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
            <div className="empty-state">{t('editor.empty')}</div>
          )}
        </div>
      </section>

      <aside id="review-column" className="review-column" aria-hidden={!commentsOpen} inert={!commentsOpen}>
        <section className="selection-card">
          <h3>{t('review.cursor')}</h3>
          <p className="line-indicator">{t('review.line', { number: selectedLine ? selectedLineIndex + 1 : 0 })}</p>
          <p>{selectedText || linePreview(selectedLine, t)}</p>
          {snapshot && (dirty || remoteConflict) ? <span className="note">{t('review.saveOrRefresh')}</span> : null}
          {snapshot && !selectedText && !dirty && !remoteConflict ? <span className="note">{t('review.selectText')}</span> : null}
        </section>
        <section>
          <h3>{t('review.comments')}</h3>
          <form onSubmit={createComment}>
            <textarea value={commentBody} onChange={(event) => setCommentBody(event.target.value)} placeholder={t('review.commentPlaceholder')} />
            <ButtonWithIcon icon="add_comment" disabled={annotationsDisabled || !commentBody.trim()}>
              {t('review.comment')}
            </ButtonWithIcon>
          </form>
          {snapshot?.comments.map((comment) => (
            <article
              key={comment.id}
              id={`comment-${comment.id}`}
              className={[
                comment.resolved ? 'muted item' : 'item',
                activeCommentId === comment.id ? 'active-comment' : '',
                flashingCommentId === comment.id ? 'flash-comment' : '',
              ]
                .filter(Boolean)
                .join(' ')}
              onClick={() => setActiveCommentId(comment.id)}
            >
              <strong>{comment.author_nickname}</strong>
              <p>{comment.body}</p>
              {snapshot.replies
                .filter((reply) => reply.comment_id === comment.id)
                .map((reply) => (
                  <p className="reply" key={reply.id}>
                    {reply.author_nickname}: {reply.body}
                  </p>
                ))}
            </article>
          ))}
        </section>
        {status ? <p className="status">{status}</p> : null}
      </aside>

      {historyOpen ? (
        <div className="history-drawer" role="dialog" aria-label={t('history.title')}>
          <button className="sheet-backdrop" aria-label={t('history.closeLabel')} onClick={closeHistoryDrawer} />
          <section className="history-panel">
            <div className="history-sticky">
              <header>
                <h3>{t('history.title')}</h3>
                <IconButton icon="close" label={t('history.close')} onClick={closeHistoryDrawer} />
              </header>
              <section className="history-filters">
                <div className="history-actions">
                  <button
                    type="button"
                    className="history-filter-toggle"
                    aria-expanded={historyFiltersOpen}
                    onClick={() => setHistoryFiltersOpen((open) => !open)}
                  >
                    <Icon name="filter_list" />
                    <span>{t('history.filters')}</span>
                  </button>
                  <button
                    type="button"
                    className={historyDiffMode ? 'history-diff-toggle active' : 'history-diff-toggle'}
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
                    <Icon name={historyDiffMode ? 'close' : 'compare_arrows'} />
                    <span>{historyDiffMode ? t('history.diff.cancel') : t('history.diff.button')}</span>
                  </button>
                </div>
                <div
                  className={historyFiltersOpen ? 'history-filter-fields open' : 'history-filter-fields'}
                  aria-hidden={!historyFiltersOpen}
                  inert={!historyFiltersOpen}
                >
                  <label>
                    <span>{t('history.category')}</span>
                    <select value={historyCategory} onChange={(event) => setHistoryCategory(event.target.value as AuditCategory)}>
                      {AUDIT_CATEGORIES.map((category) => (
                        <option key={category} value={category}>
                          {t(`history.categories.${category}`)}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label>
                    <span>{t('history.from')}</span>
                    <input type="datetime-local" value={historyFrom} onChange={(event) => setHistoryFrom(event.target.value)} />
                  </label>
                  <label>
                    <span>{t('history.to')}</span>
                    <input type="datetime-local" value={historyTo} onChange={(event) => setHistoryTo(event.target.value)} />
                  </label>
                  <button type="button" onClick={clearHistoryFilters}>
                    {t('history.clearFilters')}
                  </button>
                </div>
              </section>
              {historyDiffMode ? (
                <p className={historyDiffError ? 'history-diff-status error' : 'history-diff-status'}>
                  {historyDiffError ||
                    (historyDiffLoading
                      ? t('history.diff.loading')
                      : historyDiffFromId
                        ? t('history.diff.pickEnd')
                        : t('history.diff.pickStart'))}
                </p>
              ) : null}
            </div>
            {filteredAuditEvents.length === 0 ? <p className="empty-state compact">{t('history.empty')}</p> : null}
            {filteredAuditEvents.map((event) => {
              const formatted = formatAuditEvent(event, t, auditFormatContext)
              const expanded = expandedAuditIds.has(event.id)
              const expandable = formatted.body.length > AUDIT_COLLAPSE_LIMIT
              const body = expandable && !expanded ? truncateAuditText(formatted.body) : formatted.body
              const diffSelected = historyDiffFromId === event.id
              return (
                <article
                  key={event.id}
                  data-audit-id={event.id}
                  className={['audit-row', historyDiffMode ? 'diff-mode' : '', diffSelected ? 'diff-selected' : ''].filter(Boolean).join(' ')}
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
                  <div className="audit-title-row">
                    <strong>{formatted.title}</strong>
                    {editingAuditNoteId === event.id ? null : (
                      <button
                        type="button"
                        className="audit-note-link"
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
                  <span>
                    {event.actor_nickname} · {t(`role.${event.role_mode}`)} · {t(`history.categories.${formatted.category}`)} · {formatDate(event.created_at)}
                  </span>
                  {editingAuditNoteId === event.id ? (
                    <section className="audit-note-editor" onClick={(clickEvent) => clickEvent.stopPropagation()}>
                      <textarea
                        value={auditNoteDraft}
                        maxLength={2000}
                        onChange={(changeEvent) => setAuditNoteDraft(changeEvent.target.value)}
                        placeholder={t('history.note.placeholder')}
                      />
                      <div className="mini-actions">
                        <button type="button" className="primary" onClick={() => void saveAuditNote(event)}>
                          {t('history.note.save')}
                        </button>
                        <button type="button" onClick={() => setEditingAuditNoteId(null)}>
                          {t('history.note.cancel')}
                        </button>
                        <button type="button" onClick={() => void saveAuditNote(event, '')}>
                          {t('history.note.clear')}
                        </button>
                      </div>
                    </section>
                  ) : null}
                  {body ? <p className={expanded ? 'audit-body expanded' : 'audit-body'}>{body}</p> : null}
                  {expandable ? (
                    <button
                      type="button"
                      className="link-button"
                      onClick={(clickEvent) => {
                        clickEvent.stopPropagation()
                        toggleAuditEvent(event.id)
                      }}
                    >
                      {expanded ? t('history.showLess') : t('history.showMore')}
                    </button>
                  ) : null}
                </article>
              )
            })}
          </section>
        </div>
      ) : null}
      {historyDiff ? <HistoryDiffModal diff={historyDiff} formatDate={formatDate} closeLabel={t('history.diff.close')} onClose={closeHistoryDiff} /> : null}
    </main>
  )
}

export default App
