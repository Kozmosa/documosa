import { useCallback, useEffect, useRef, useState } from 'react'
import Cherry from 'cherry-markdown/dist/cherry-markdown.esm'
import type { EditorView } from '@codemirror/view'
import 'cherry-markdown/dist/cherry-markdown.css'

export type CherryInstance = {
  destroy: () => void
  switchModel: (model: 'editOnly' | 'edit&preview' | 'previewOnly', showToolbar?: boolean) => void
  getMarkdown: () => string
  setMarkdown: (content: string, keepCursor?: boolean) => void
  getCodeMirror: () => EditorView
}

export type EditorSelection = {
  startLineNumber: number
  endLineNumber: number
  startColumn: number
  endColumn: number
  text: string
}

export type CommentRange = {
  id: string
  from: number
  to: number
}

type UnderlineBox = {
  id: string
  left: number
  top: number
  width: number
}

declare global {
  interface Window {
    __documosaEditor?: {
      setMarkdown: (content: string) => void
      getMarkdown: () => string
      selectRange: (from: number, to: number) => void
    }
  }
}

function isEditableKey(event: KeyboardEvent) {
  if (event.metaKey || event.ctrlKey || event.altKey) return false
  return event.key.length === 1 || ['Backspace', 'Delete', 'Enter', 'Tab'].includes(event.key)
}

function selectionFromEditor(editor: EditorView): EditorSelection {
  const selected = editor.state.selection.main
  const from = Math.min(selected.from, selected.to)
  const to = Math.max(selected.from, selected.to)
  const startLine = editor.state.doc.lineAt(from)
  const endLine = editor.state.doc.lineAt(to)
  return {
    startLineNumber: startLine.number,
    endLineNumber: endLine.number,
    startColumn: from - startLine.from,
    endColumn: to - endLine.from,
    text: editor.state.doc.sliceString(from, to),
  }
}

export default function CherryEditor({
  documentId,
  value,
  resetToken,
  readOnly,
  commentRanges,
  onReady,
  onMarkdownChange,
  onDirtyChange,
  onSelectionChange,
  onCommentClick,
}: {
  documentId: string
  value: string
  resetToken: number
  readOnly: boolean
  commentRanges: CommentRange[]
  onReady: (instance: CherryInstance | null) => void
  onMarkdownChange: (markdown: string) => void
  onDirtyChange: (dirty: boolean) => void
  onSelectionChange: (selection: EditorSelection) => void
  onCommentClick: (commentId: string) => void
}) {
  const hostRef = useRef<HTMLDivElement | null>(null)
  const cherryRef = useRef<CherryInstance | null>(null)
  const baseValueRef = useRef(value)
  const [editorId] = useState(() => `documosa-editor-${crypto.randomUUID()}`)
  const readOnlyRef = useRef(readOnly)
  const [underlineBoxes, setUnderlineBoxes] = useState<UnderlineBox[]>([])
  const commentRangesRef = useRef(commentRanges)
  const onReadyRef = useRef(onReady)
  const onMarkdownChangeRef = useRef(onMarkdownChange)
  const onDirtyChangeRef = useRef(onDirtyChange)
  const onSelectionChangeRef = useRef(onSelectionChange)
  const onCommentClickRef = useRef(onCommentClick)

  useEffect(() => {
    readOnlyRef.current = readOnly
  }, [readOnly])

  useEffect(() => {
    commentRangesRef.current = commentRanges
  }, [commentRanges])

  useEffect(() => {
    onReadyRef.current = onReady
    onMarkdownChangeRef.current = onMarkdownChange
    onDirtyChangeRef.current = onDirtyChange
    onSelectionChangeRef.current = onSelectionChange
    onCommentClickRef.current = onCommentClick
  }, [onCommentClick, onDirtyChange, onMarkdownChange, onReady, onSelectionChange])

  const reportSelection = useCallback(() => {
    const editor = cherryRef.current?.getCodeMirror()
    if (!editor) {
      onSelectionChangeRef.current({ startLineNumber: 1, endLineNumber: 1, startColumn: 0, endColumn: 0, text: '' })
      return
    }
    onSelectionChangeRef.current(selectionFromEditor(editor))
  }, [])

  const updateUnderlines = useCallback(() => {
    const editor = cherryRef.current?.getCodeMirror()
    const host = hostRef.current
    if (!editor || !host) {
      setUnderlineBoxes([])
      return
    }
    const hostRect = host.getBoundingClientRect()
    const boxes: UnderlineBox[] = []
    for (const range of commentRangesRef.current) {
      const fromLine = editor.state.doc.lineAt(range.from)
      const toLine = editor.state.doc.lineAt(range.to)
      for (let lineNumber = fromLine.number; lineNumber <= toLine.number; lineNumber += 1) {
        const line = editor.state.doc.line(lineNumber)
        const from = Math.max(range.from, line.from)
        const to = Math.min(range.to, line.to)
        if (to <= from) continue
        const start = editor.coordsAtPos(from, 1)
        const end = editor.coordsAtPos(to, -1)
        if (!start || !end) continue
        const left = Math.min(start.left, end.left) - hostRect.left
        const width = Math.abs(end.left - start.left)
        if (width < 1) continue
        boxes.push({
          id: range.id,
          left,
          top: Math.max(start.bottom, end.bottom) - hostRect.top - 3,
          width,
        })
      }
    }
    setUnderlineBoxes(boxes)
  }, [])

  useEffect(() => {
    if (!hostRef.current) return undefined
    baseValueRef.current = value
    const cherry = new Cherry({
      id: editorId,
      value,
      engine: {
        syntax: {
          table: false,
          codeBlock: false,
        },
      },
      editor: {
        defaultModel: 'editOnly',
      },
      toolbars: {
        toolbar: ['undo', 'redo', '|', 'bold', 'italic', 'link', '|', 'ul', 'ol', 'quote', 'code'],
        bubble: false,
        float: false,
      },
      callback: {
        afterChange: () => {
          const markdown = cherry.getMarkdown()
          onMarkdownChangeRef.current(markdown)
          onDirtyChangeRef.current(markdown !== baseValueRef.current)
          reportSelection()
          window.requestAnimationFrame(updateUnderlines)
        },
      },
      event: {
        selectionChange: reportSelection,
      },
    })
    cherry.switchModel('editOnly')
    cherryRef.current = cherry
    window.requestAnimationFrame(updateUnderlines)
    const testHandle = {
      setMarkdown: (content: string) => {
        cherry.setMarkdown(content, true)
        const markdown = cherry.getMarkdown()
        onMarkdownChangeRef.current(markdown)
        onDirtyChangeRef.current(markdown !== baseValueRef.current)
        reportSelection()
        window.requestAnimationFrame(updateUnderlines)
      },
      getMarkdown: () => cherry.getMarkdown(),
      selectRange: (from: number, to: number) => {
        const editor = cherry.getCodeMirror()
        editor.dispatch({ selection: { anchor: from, head: to }, scrollIntoView: true })
        reportSelection()
        window.requestAnimationFrame(updateUnderlines)
      },
    }
    if (import.meta.env.DEV) {
      window.__documosaEditor = testHandle
    }
    onReadyRef.current(cherry)
    reportSelection()
    return () => {
      onReadyRef.current(null)
      if (window.__documosaEditor === testHandle) {
        delete window.__documosaEditor
      }
      cherry.destroy()
      cherryRef.current = null
      setUnderlineBoxes([])
    }
  }, [documentId, editorId, reportSelection, updateUnderlines, value])

  useEffect(() => {
    const host = hostRef.current
    if (!host) return undefined
    const preventEdit = (event: Event) => {
      if (readOnlyRef.current) event.preventDefault()
    }
    const preventKeyEdit = (event: KeyboardEvent) => {
      if (readOnlyRef.current && isEditableKey(event)) event.preventDefault()
    }
    host.addEventListener('beforeinput', preventEdit, true)
    host.addEventListener('paste', preventEdit, true)
    host.addEventListener('drop', preventEdit, true)
    host.addEventListener('keydown', preventKeyEdit, true)
    const handleClick = (event: MouseEvent) => {
      const target = event.target instanceof HTMLElement ? event.target.closest('.comment-underline') : null
      const commentId = target?.getAttribute('data-comment-id')
      if (commentId) onCommentClickRef.current(commentId)
      reportSelection()
    }
    host.addEventListener('click', handleClick, true)
    host.addEventListener('keyup', reportSelection, true)
    return () => {
      host.removeEventListener('beforeinput', preventEdit, true)
      host.removeEventListener('paste', preventEdit, true)
      host.removeEventListener('drop', preventEdit, true)
      host.removeEventListener('keydown', preventKeyEdit, true)
      host.removeEventListener('click', handleClick, true)
      host.removeEventListener('keyup', reportSelection, true)
    }
  }, [reportSelection])

  useEffect(() => {
    const editor = cherryRef.current?.getCodeMirror()
    const scrollDOM = editor?.scrollDOM
    if (!scrollDOM) return undefined
    const schedule = () => window.requestAnimationFrame(updateUnderlines)
    scrollDOM.addEventListener('scroll', schedule)
    window.addEventListener('resize', schedule)
    return () => {
      scrollDOM.removeEventListener('scroll', schedule)
      window.removeEventListener('resize', schedule)
    }
  }, [updateUnderlines, documentId])

  useEffect(() => {
    commentRangesRef.current = commentRanges
    const frame = window.requestAnimationFrame(updateUnderlines)
    return () => window.cancelAnimationFrame(frame)
  }, [commentRanges, updateUnderlines])

  useEffect(() => {
    const cherry = cherryRef.current
    if (!cherry) return
    baseValueRef.current = value
    if (cherry.getMarkdown() !== value) {
      cherry.setMarkdown(value, true)
    }
    onMarkdownChangeRef.current(value)
    onDirtyChangeRef.current(false)
    reportSelection()
    window.requestAnimationFrame(updateUnderlines)
  }, [reportSelection, resetToken, updateUnderlines, value])

  return (
    <div className={readOnly ? 'cherry-shell read-only' : 'cherry-shell'} ref={hostRef}>
      <div id={editorId} />
      <div className="comment-overlay">
        {underlineBoxes.map((box, index) => (
          <button
            key={`${box.id}-${index}`}
            type="button"
            className="comment-underline"
            data-comment-id={box.id}
            style={{ left: box.left, top: box.top, width: box.width }}
            tabIndex={-1}
            onClick={() => onCommentClickRef.current(box.id)}
          />
        ))}
      </div>
    </div>
  )
}
