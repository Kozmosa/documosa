import { useEditor, EditorContent } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import Placeholder from '@tiptap/extension-placeholder'
import { useEffect } from 'react'
import { blocksToProseMirrorDoc, proseMirrorToBlocks } from '@/lib/converter'
import type { DocumosaBlock } from '@/lib/converter'

interface TiptapEditorProps {
  blocks: DocumosaBlock[]
  readOnly?: boolean
  onChange: (blocks: DocumosaBlock[], text: string) => void
  onSelectionChange?: (blockId: string | null) => void
}

export default function TiptapEditor({ blocks, readOnly, onChange, onSelectionChange }: TiptapEditorProps) {
  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: { levels: [1, 2, 3] },
        codeBlock: false,
      }),
      Placeholder.configure({ placeholder: 'Type / for commands...' }),
    ],
    content: blocksToProseMirrorDoc(blocks),
    editable: !readOnly,
    onUpdate: ({ editor }) => {
      const doc = editor.getJSON()
      const newBlocks = proseMirrorToBlocks(doc)
      const text = editor.getText()
      onChange(newBlocks, text)
    },
    onSelectionUpdate: ({ editor }) => {
      if (!onSelectionChange) return
      const { $anchor } = editor.state.selection
      const doc = editor.state.doc
      let result = -1
      doc.content.forEach((node, offset, index) => {
        if ($anchor.pos >= offset && $anchor.pos < offset + node.nodeSize) {
          result = index
        }
      })
      onSelectionChange(result >= 0 ? String(result) : null)
    },
  })

  useEffect(() => {
    if (!editor) return

    const currentJson = JSON.stringify(editor.getJSON())
    const nextDoc = blocksToProseMirrorDoc(blocks)
    const nextJson = JSON.stringify(nextDoc)
    if (currentJson !== nextJson) {
      editor.commands.setContent(nextDoc)
    }
  }, [editor, blocks])

  return (
    <div className="tiptap-editor prose prose-sm max-w-none h-full">
      <EditorContent editor={editor} className="h-full" />
      <style>{`
        .tiptap-editor .ProseMirror {
          outline: none;
          min-height: 200px;
          padding: 0.5rem 0;
        }
        .tiptap-editor .ProseMirror p.is-editor-empty:first-child::before {
          color: #adb5bd;
          content: attr(data-placeholder);
          float: left;
          height: 0;
          pointer-events: none;
        }
        .tiptap-editor .ProseMirror h1 { font-size: 1.875rem; font-weight: 700; line-height: 1.2; margin: 1rem 0 0.25rem; }
        .tiptap-editor .ProseMirror h2 { font-size: 1.5rem; font-weight: 600; line-height: 1.3; margin: 0.75rem 0 0.25rem; }
        .tiptap-editor .ProseMirror h3 { font-size: 1.25rem; font-weight: 600; line-height: 1.4; margin: 0.5rem 0 0.25rem; }
        .tiptap-editor .ProseMirror p { margin: 0.25rem 0; }
        .tiptap-editor .ProseMirror blockquote {
          border-left: 3px solid var(--color-border);
          margin: 0.5rem 0;
          padding-left: 1rem;
          color: var(--color-muted-foreground);
        }
        .tiptap-editor .ProseMirror hr {
          border: none;
          border-top: 1px solid var(--color-border);
          margin: 1rem 0;
        }
        .tiptap-editor .ProseMirror ul,
        .tiptap-editor .ProseMirror ol {
          padding-left: 1.5rem;
          margin: 0.25rem 0;
        }
        .tiptap-editor .ProseMirror code {
          background: var(--color-muted);
          border-radius: 0.25rem;
          padding: 0.125rem 0.25rem;
          font-size: 0.875em;
        }
        .tiptap-editor .ProseMirror pre {
          background: var(--color-muted);
          border-radius: 0.375rem;
          padding: 0.75rem 1rem;
          margin: 0.5rem 0;
          overflow-x: auto;
        }
        .tiptap-editor .ProseMirror pre code {
          background: none;
          padding: 0;
          border-radius: 0;
        }
      `}</style>
    </div>
  )
}
