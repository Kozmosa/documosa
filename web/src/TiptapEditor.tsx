import { useEditor, EditorContent } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import CodeBlock from '@tiptap/extension-code-block'
import Placeholder from '@tiptap/extension-placeholder'
import { useEffect, useRef } from 'react'
import { blocksToProseMirrorDoc, proseMirrorToBlocks } from '@/lib/converter'
import type { DocumosaBlock } from '@/lib/converter'

const DocumosaCodeBlock = CodeBlock.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      documosaBlockType: {
        default: null,
        parseHTML: element => element.getAttribute('data-documosa-block-type'),
        renderHTML: attributes => {
          if (!attributes.documosaBlockType) return {}
          return { 'data-documosa-block-type': attributes.documosaBlockType }
        },
      },
    }
  },
})

interface TiptapEditorProps {
  blocks: DocumosaBlock[]
  readOnly?: boolean
  onChange: (blocks: DocumosaBlock[], text: string) => void
  onSelectionChange?: (blockId: string | null) => void
}

export default function TiptapEditor({ blocks, readOnly, onChange, onSelectionChange }: TiptapEditorProps) {
  const lastEmittedBlocksJson = useRef<string | null>(null)

  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: { levels: [1, 2, 3] },
        codeBlock: false,
      }),
      DocumosaCodeBlock,
      Placeholder.configure({ placeholder: 'Paste or start writing Markdown…' }),
    ],
    content: blocksToProseMirrorDoc(blocks),
    editable: !readOnly,
    onUpdate: ({ editor }) => {
      const doc = editor.getJSON()
      const newBlocks = proseMirrorToBlocks(doc)
      lastEmittedBlocksJson.current = JSON.stringify(newBlocks)
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
    if (!editor || editor.isDestroyed) return

    const nextBlocksJson = JSON.stringify(blocks)
    if (lastEmittedBlocksJson.current === nextBlocksJson) return

    const currentJson = JSON.stringify(editor.getJSON())
    const nextDoc = blocksToProseMirrorDoc(blocks)
    const nextJson = JSON.stringify(nextDoc)
    if (currentJson !== nextJson && !editor.isDestroyed) {
      editor.commands.setContent(nextDoc, { emitUpdate: false })
    }
    lastEmittedBlocksJson.current = null
  }, [editor, blocks])

  return (
    <div className="tiptap-editor prose prose-sm max-w-none h-full">
      <EditorContent editor={editor} className="h-full" />
      <style>{`
        .tiptap-editor .ProseMirror {
          outline: none;
          min-height: 200px;
          padding: 0.5rem 0;
          font-family: Georgia, "Times New Roman", ui-serif, serif;
          font-size: 1.05rem;
          line-height: 1.75;
          color: #3d3929;
        }
        .tiptap-editor .ProseMirror p.is-editor-empty:first-child::before {
          color: #b8b0a0;
          content: attr(data-placeholder);
          float: left;
          height: 0;
          pointer-events: none;
          font-family: Inter, ui-sans-serif, system-ui, sans-serif;
        }
        .tiptap-editor .ProseMirror h1 {
          font-family: Georgia, "Times New Roman", ui-serif, serif;
          font-size: 1.75rem;
          font-weight: 400;
          line-height: 1.25;
          margin: 1.75rem 0 0.25rem;
          color: #2d2a1d;
          letter-spacing: -0.01em;
        }
        .tiptap-editor .ProseMirror h2 {
          font-family: Georgia, "Times New Roman", ui-serif, serif;
          font-size: 1.35rem;
          font-weight: 400;
          line-height: 1.3;
          margin: 1.5rem 0 0.2rem;
          color: #2d2a1d;
        }
        .tiptap-editor .ProseMirror h3 {
          font-family: Georgia, "Times New Roman", ui-serif, serif;
          font-size: 1.15rem;
          font-weight: 500;
          line-height: 1.35;
          margin: 1.25rem 0 0.15rem;
          color: #2d2a1d;
        }
        .tiptap-editor .ProseMirror p {
          margin: 0.4rem 0;
        }
        .tiptap-editor .ProseMirror blockquote {
          border-left: 3px solid #d97757;
          margin: 0.75rem 0;
          padding: 0.25rem 0 0.25rem 1rem;
          color: #7a7665;
          font-style: italic;
        }
        .tiptap-editor .ProseMirror hr {
          border: none;
          border-top: 1px solid #e0dcd0;
          margin: 1.25rem 0;
        }
        .tiptap-editor .ProseMirror ul,
        .tiptap-editor .ProseMirror ol {
          padding-left: 1.5rem;
          margin: 0.4rem 0;
        }
        .tiptap-editor .ProseMirror li {
          margin: 0.15rem 0;
        }
        .tiptap-editor .ProseMirror code {
          background: #f3efe6;
          border-radius: 0.25rem;
          padding: 0.125rem 0.3rem;
          font-size: 0.875em;
          font-family: "SF Mono", "Fira Code", "Fira Mono", ui-monospace, monospace;
          color: #8b5e3c;
        }
        .tiptap-editor .ProseMirror pre {
          background: #f7f4ec;
          border: 1px solid #e8e3d7;
          border-radius: 0.5rem;
          padding: 0.875rem 1.125rem;
          margin: 0.75rem 0;
          overflow-x: auto;
        }
        .tiptap-editor .ProseMirror pre code {
          background: none;
          padding: 0;
          border-radius: 0;
          color: #3d3929;
          font-size: 0.9em;
        }
      `}</style>
    </div>
  )
}
