import type { JSONContent } from '@tiptap/core'

interface RichTextToken {
  type: string
  text?: { content: string; link?: string | null }
  annotations?: Annotations
  plain_text: string
  href?: string | null
}

interface Annotations {
  bold: boolean
  italic: boolean
  strikethrough: boolean
  underline: boolean
  code: boolean
  color: string
}

interface DocumosaBlock {
  id?: string
  block_type: string
  content_json: string
  properties_json?: string
}

// ProseMirror marks -> Annotations
function marksToAnnotations(marks?: { type: string; attrs?: Record<string, unknown> }[]): Annotations {
  const annotations: Annotations = { bold: false, italic: false, strikethrough: false, underline: false, code: false, color: 'default' }
  if (!marks) return annotations
  for (const mark of marks) {
    switch (mark.type) {
      case 'bold': annotations.bold = true; break
      case 'italic': annotations.italic = true; break
      case 'strike': annotations.strikethrough = true; break
      case 'underline': annotations.underline = true; break
      case 'code': annotations.code = true; break
      case 'link': break
    }
  }
  return annotations
}

// ProseMirror JSON -> RichText token
function proseMirrorTextToRichText(node: JSONContent): RichTextToken[] {
  if (node.type === 'text') {
    return [{
      type: 'text',
      text: { content: node.text || '', link: node.marks?.find(m => m.type === 'link')?.attrs?.href as string | null },
      annotations: marksToAnnotations(node.marks?.filter(m => m.type !== 'link') as { type: string; attrs?: Record<string, unknown> }[]),
      plain_text: node.text || '',
      href: node.marks?.find(m => m.type === 'link')?.attrs?.href as string | null,
    }]
  }
  if (node.type === 'hardBreak') {
    return [{ type: 'text', text: { content: '\n' }, annotations: marksToAnnotations(), plain_text: '\n' }]
  }
  return []
}

// Block type -> ProseMirror node type
const BLOCK_TYPE_MAP: Record<string, string> = {
  'paragraph': 'paragraph',
  'heading_1': 'heading',
  'heading_2': 'heading',
  'heading_3': 'heading',
  'bulleted_list_item': 'listItem',
  'numbered_list_item': 'listItem',
  'code': 'codeBlock',
  'quote': 'blockquote',
  'divider': 'horizontalRule',
  'to_do': 'taskItem',
}

const HEADING_LEVEL: Record<string, number> = {
  'heading_1': 1, 'heading_2': 2, 'heading_3': 3,
}

// Documosa Block -> ProseMirror JSON node
export function blockToProseMirror(block: DocumosaBlock): JSONContent {
  let tokens: RichTextToken[] = []
  try { tokens = JSON.parse(block.content_json) } catch { /* empty */ }

  if (block.block_type === 'divider') {
    return { type: 'horizontalRule' }
  }

  if (block.block_type.startsWith('heading_')) {
    const level = HEADING_LEVEL[block.block_type] || 1
    return { type: 'heading', attrs: { level }, content: [{ type: 'text', text: tokens.map(t => t.plain_text).join('') }] }
  }

  if (block.block_type === 'code') {
    let lang: string | undefined
    try { lang = JSON.parse(block.properties_json || '{}').language } catch { /* ignore */ }
    return { type: 'codeBlock', attrs: { language: lang }, content: [{ type: 'text', text: tokens.map(t => t.plain_text).join('') }] }
  }

  const pmType = BLOCK_TYPE_MAP[block.block_type] || 'paragraph'
  return { type: pmType, content: [{ type: 'text', text: tokens.map(t => t.plain_text).join('') }] }
}

// ProseMirror JSON node -> Documosa Block
export function proseMirrorNodeToBlock(node: JSONContent): DocumosaBlock {
  const tokens: RichTextToken[] = []

  if (node.content) {
    for (const child of node.content) {
      tokens.push(...proseMirrorTextToRichText(child))
    }
  }

  const nodeTypeToBlock: Record<string, string> = {
    'paragraph': 'paragraph',
    'blockquote': 'quote',
    'horizontalRule': 'divider',
    'listItem': 'bulleted_list_item',
    'taskItem': 'to_do',
  }

  let blockType: string
  const props: Record<string, unknown> = {}

  if (node.type === 'heading') {
    const level = (node.attrs as Record<string, number>)?.level || 1
    blockType = `heading_${level}`
  } else if (node.type === 'codeBlock') {
    blockType = 'code'
    props.language = (node.attrs as { language?: string })?.language || 'plain text'
  } else {
    blockType = nodeTypeToBlock[node.type || ''] || 'paragraph'
  }

  return {
    block_type: blockType,
    content_json: JSON.stringify(tokens),
    properties_json: Object.keys(props).length ? JSON.stringify(props) : '{}',
  }
}

// Convert ProseMirror document to flat blocks
export function proseMirrorToBlocks(doc: JSONContent): DocumosaBlock[] {
  const blocks: DocumosaBlock[] = []
  if (!doc.content) return blocks

  for (const node of doc.content) {
    if (node.type === 'orderedList' || node.type === 'bulletList' || node.type === 'taskList') {
      if (node.content) {
        for (const item of node.content) {
          const block = proseMirrorNodeToBlock(item)
          if (node.type === 'orderedList') block.block_type = 'numbered_list_item'
          if (node.type === 'bulletList') block.block_type = 'bulleted_list_item'
          if (node.type === 'taskList') block.block_type = 'to_do'
          blocks.push(block)
        }
      }
    } else {
      blocks.push(proseMirrorNodeToBlock(node))
    }
  }
  return blocks
}

// Convert flat blocks to ProseMirror document
export function blocksToProseMirrorDoc(blocks: DocumosaBlock[]): JSONContent {
  const content: JSONContent[] = blocks
    .filter(b => !b.id || b.block_type !== 'divider')
    .map(blockToProseMirror)
  return { type: 'doc', content }
}

export type { RichTextToken, Annotations, DocumosaBlock }
