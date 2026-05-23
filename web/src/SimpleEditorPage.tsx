import React, { Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Download, FileUp, RotateCcw } from 'lucide-react'

import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { blocksToMarkdown, markdownToBlocks } from '@/lib/simpleEditorApi'
import type { DocumosaBlock } from '@/lib/converter'

const TiptapEditor = React.lazy(() => import('@/TiptapEditor'))

const DRAFT_KEY = 'documosa.simple_editor.draft'
const DEFAULT_FILENAME = 'documosa-simple-editor.md'
const AUTOSAVE_DELAY_MS = 500

type SimpleEditorDraft = {
  filename: string
  blocks: DocumosaBlock[]
  updatedAt: string
}

function emptyDraft(): SimpleEditorDraft {
  return {
    filename: DEFAULT_FILENAME,
    blocks: [],
    updatedAt: new Date().toISOString(),
  }
}

function isValidBlock(value: unknown): value is DocumosaBlock {
  if (!value || typeof value !== 'object') return false
  const block = value as Partial<DocumosaBlock>
  return typeof block.block_type === 'string' && typeof block.content_json === 'string'
}

function normalizeDraft(value: unknown): SimpleEditorDraft | null {
  if (!value || typeof value !== 'object') return null
  const draft = value as Partial<SimpleEditorDraft>
  if (!Array.isArray(draft.blocks) || !draft.blocks.every(isValidBlock)) return null

  return {
    filename: typeof draft.filename === 'string' && draft.filename.trim() ? draft.filename : DEFAULT_FILENAME,
    blocks: draft.blocks,
    updatedAt: typeof draft.updatedAt === 'string' && !Number.isNaN(new Date(draft.updatedAt).getTime())
      ? draft.updatedAt
      : new Date().toISOString(),
  }
}

function loadDraft(): SimpleEditorDraft {
  try {
    const raw = localStorage.getItem(DRAFT_KEY)
    if (!raw) return emptyDraft()
    return normalizeDraft(JSON.parse(raw)) ?? emptyDraft()
  } catch {
    return emptyDraft()
  }
}

function hasContent(blocks: DocumosaBlock[]) {
  return blocks.some((block) => {
    if (block.block_type === 'divider') return true

    try {
      const tokens = JSON.parse(block.content_json) as unknown
      if (!Array.isArray(tokens)) return false
      return tokens.some((token) => {
        if (!token || typeof token !== 'object') return false
        const plainText = (token as { plain_text?: unknown }).plain_text
        return typeof plainText === 'string' && plainText.trim().length > 0
      })
    } catch {
      return false
    }
  })
}

function saveDraft(draft: SimpleEditorDraft) {
  localStorage.setItem(DRAFT_KEY, JSON.stringify(draft))
}

function formatSavedAt(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return 'Not saved yet'
  return `Saved ${date.toLocaleString()}`
}

function downloadText(filename: string, text: string) {
  const blob = new Blob([text], { type: 'text/markdown;charset=utf-8' })
  const href = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = href
  anchor.download = filename.trim() || DEFAULT_FILENAME
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
  URL.revokeObjectURL(href)
}

export default function SimpleEditorPage() {
  const [draft, setDraft] = useState<SimpleEditorDraft>(() => loadDraft())
  const [error, setError] = useState('')
  const [saveError, setSaveError] = useState('')
  const [isConverting, setIsConverting] = useState(false)
  const [hasPendingSave, setHasPendingSave] = useState(false)
  const saveTimer = useRef<number | null>(null)
  const pendingDraft = useRef<SimpleEditorDraft | null>(null)
  const fileInputRef = useRef<HTMLInputElement | null>(null)

  const savedLabel = useMemo(() => {
    if (saveError) return saveError
    if (hasPendingSave) return 'Saving...'
    return formatSavedAt(draft.updatedAt)
  }, [draft.updatedAt, hasPendingSave, saveError])

  const flushPendingDraft = useCallback(() => {
    if (saveTimer.current !== null) {
      window.clearTimeout(saveTimer.current)
      saveTimer.current = null
    }

    if (!pendingDraft.current) return

    try {
      saveDraft(pendingDraft.current)
      pendingDraft.current = null
      setSaveError('')
    } catch {
      setSaveError('Auto-save unavailable')
    } finally {
      setHasPendingSave(false)
    }
  }, [])

  useEffect(() => {
    const handleBeforeUnload = () => flushPendingDraft()
    const handleVisibilityChange = () => {
      if (document.visibilityState === 'hidden') flushPendingDraft()
    }

    window.addEventListener('beforeunload', handleBeforeUnload)
    document.addEventListener('visibilitychange', handleVisibilityChange)

    return () => {
      window.removeEventListener('beforeunload', handleBeforeUnload)
      document.removeEventListener('visibilitychange', handleVisibilityChange)
      flushPendingDraft()
    }
  }, [flushPendingDraft])

  const persistDraft = useCallback((nextDraft: SimpleEditorDraft) => {
    try {
      saveDraft(nextDraft)
      pendingDraft.current = null
      setSaveError('')
    } catch {
      setSaveError('Auto-save unavailable')
    } finally {
      setHasPendingSave(false)
    }
  }, [])

  const updateDraft = useCallback((updater: (current: SimpleEditorDraft) => SimpleEditorDraft, immediate = false) => {
    setDraft((current) => {
      const next = updater(current)
      if (saveTimer.current !== null) window.clearTimeout(saveTimer.current)
      if (immediate) {
        pendingDraft.current = null
        persistDraft(next)
      } else {
        pendingDraft.current = next
        setHasPendingSave(true)
        saveTimer.current = window.setTimeout(() => persistDraft(next), AUTOSAVE_DELAY_MS)
      }
      return next
    })
  }, [persistDraft])

  const handleEditorChange = useCallback((blocks: DocumosaBlock[]) => {
    updateDraft((current) => ({
      ...current,
      blocks,
      updatedAt: new Date().toISOString(),
    }))
  }, [updateDraft])

  const handleFilenameChange = useCallback((filename: string) => {
    updateDraft((current) => ({
      ...current,
      filename,
      updatedAt: new Date().toISOString(),
    }))
  }, [updateDraft])

  const handleUploadClick = useCallback(() => {
    fileInputRef.current?.click()
  }, [])

  async function uploadFile(file: File) {
    if (hasContent(draft.blocks) && !window.confirm('Uploading will replace the current local draft. Continue?')) {
      if (fileInputRef.current) fileInputRef.current.value = ''
      return
    }

    setError('')
    setIsConverting(true)
    try {
      const markdown = await file.text()
      const blocks = await markdownToBlocks(markdown)
      const nextDraft = {
        filename: file.name || DEFAULT_FILENAME,
        blocks,
        updatedAt: new Date().toISOString(),
      }
      if (saveTimer.current !== null) {
        window.clearTimeout(saveTimer.current)
        saveTimer.current = null
      }
      pendingDraft.current = null
      setDraft(nextDraft)
      persistDraft(nextDraft)
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error))
    } finally {
      setIsConverting(false)
      if (fileInputRef.current) fileInputRef.current.value = ''
    }
  }

  async function downloadDraft() {
    setError('')
    setIsConverting(true)
    try {
      const markdown = await blocksToMarkdown(draft.blocks)
      downloadText(draft.filename || DEFAULT_FILENAME, markdown)
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error))
    } finally {
      setIsConverting(false)
    }
  }

  function clearDraft() {
    if (hasContent(draft.blocks) && !window.confirm('Clear the current local draft?')) return

    const nextDraft = emptyDraft()
    if (saveTimer.current !== null) {
      window.clearTimeout(saveTimer.current)
      saveTimer.current = null
    }
    pendingDraft.current = null
    setDraft(nextDraft)
    setHasPendingSave(false)
    setError('')
    setSaveError('')

    try {
      localStorage.removeItem(DRAFT_KEY)
    } catch {
      setSaveError('Auto-save unavailable')
    }
  }

  return (
    <main
      data-testid="simple-editor-shell"
      className="min-h-screen flex flex-col items-center relative"
      style={{ backgroundColor: '#ece9e0' }}
    >
      <div className="absolute inset-0 pointer-events-none overflow-hidden" aria-hidden="true">
        <div
          className="absolute top-[-15%] left-[-5%] w-[55%] h-[55%] rounded-full"
          style={{ background: 'radial-gradient(circle, rgba(217,119,87,0.05) 0%, transparent 70%)' }}
        />
        <div
          className="absolute bottom-[-10%] right-[-5%] w-[45%] h-[45%] rounded-full"
          style={{ background: 'radial-gradient(circle, rgba(160,140,120,0.07) 0%, transparent 70%)' }}
        />
      </div>

      <div className="w-full max-w-3xl px-4 sm:px-6 lg:px-8 pt-10 sm:pt-14 pb-8 relative z-10">
        <div className="flex flex-col sm:flex-row sm:items-end sm:justify-between gap-2 mb-8">
          <div>
            <p className="text-xs uppercase tracking-[0.15em] text-stone-400 font-medium mb-2">Documosa</p>
            <h1 className="text-3xl sm:text-4xl font-serif font-normal text-stone-800 leading-tight">
              Simple Markdown Editor
            </h1>
            <p className="text-sm text-stone-500 mt-2 max-w-md">
              A quiet local draft space for quick Markdown edits.
            </p>
          </div>
          <span className="text-xs text-stone-400 whitespace-nowrap">{savedLabel}</span>
        </div>

        {error ? (
          <Alert variant="destructive" className="mb-4">
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : null}

        <div
          data-testid="simple-editor-paper"
          className="bg-white rounded-xl shadow-sm border border-stone-200 overflow-hidden"
        >
          <div className="flex flex-wrap items-center gap-2 px-4 py-3 border-b border-stone-100 bg-stone-50/50">
            <Input
              value={draft.filename}
              onChange={(event) => handleFilenameChange(event.target.value)}
              aria-label="Filename"
              className="flex-1 min-w-0 sm:max-w-xs"
            />
            <input
              ref={fileInputRef}
              type="file"
              accept=".md,.markdown,text/markdown,text/plain"
              className="hidden"
              onChange={(event) => {
                const file = event.target.files?.[0]
                if (file) void uploadFile(file)
              }}
            />
            <Button type="button" variant="outline" disabled={isConverting} onClick={handleUploadClick}>
              <FileUp className="h-4 w-4 mr-1.5" />
              Upload
            </Button>
            <Button
              type="button"
              disabled={isConverting}
              onClick={() => void downloadDraft()}
              style={{ backgroundColor: '#d97757', borderColor: '#d97757' }}
            >
              <Download className="h-4 w-4 mr-1.5" />
              Download
            </Button>
            <Button
              type="button"
              variant="ghost"
              disabled={isConverting}
              onClick={clearDraft}
              style={{ color: '#c0453a' }}
            >
              <RotateCcw className="h-4 w-4 mr-1.5" />
              Clear
            </Button>
            {isConverting ? (
              <span className="text-xs text-stone-400">Converting…</span>
            ) : null}
          </div>

          <div data-testid="simple-editor-writing-surface" className="px-6 py-5">
            <Suspense fallback={<div className="text-sm text-stone-400">Loading editor…</div>}>
              <TiptapEditor
                key={isConverting ? 'read-only' : 'editable'}
                blocks={draft.blocks}
                readOnly={isConverting}
                onChange={handleEditorChange}
              />
            </Suspense>
          </div>
        </div>
      </div>
    </main>
  )
}
