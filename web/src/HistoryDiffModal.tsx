import { useEffect, useRef } from 'react'
import AceDiff from 'ace-diff'
import * as ace from 'ace-builds'
import 'ace-builds/src-noconflict/mode-markdown'
import 'ace-builds/src-noconflict/theme-textmate'
import 'ace-diff/styles.css'

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import type { AuditEvent } from '@/lib/api'

export type HistoryDiffResponse = {
  from_event: AuditEvent
  to_event: AuditEvent
  from_content: string
  to_content: string
}

type HistoryDiffModalProps = {
  diff: HistoryDiffResponse
  formatDate: (value: string) => string
  onClose: () => void
}

export default function HistoryDiffModal({
  diff,
  formatDate,
  onClose,
}: HistoryDiffModalProps) {
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
