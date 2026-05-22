import type { JSONContent } from '@tiptap/core'

interface RichTextToken {
  type: string
  text?: { content: string; link?: { type: string; url: string } | null }
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

function richTextPlainText(tokens: RichTextToken[]): string {
  return tokens.map(t => t.plain_text).join('')
}

function textToProseMirrorInlineContent(text: string): JSONContent[] | undefined {
  if (!text) return undefined
  return [{ type: 'text', text }]
}

function nodeWithInlineText(type: string, text: string, attrs?: Record<string, unknown>): JSONContent {
  const content = textToProseMirrorInlineContent(text)
  return content ? { type, attrs, content } : { type, attrs }
}

// ProseMirror JSON -> RichText token
function proseMirrorTextToRichText(node: JSONContent): RichTextToken[] {
  if (node.type === 'text') {
    if (!node.text) return []
    const href = node.marks?.find(m => m.type === 'link')?.attrs?.href as string | null
    return [{
      type: 'text',
      text: { content: node.text, link: href ? { type: 'url', url: href } : null },
      annotations: marksToAnnotations(node.marks?.filter(m => m.type !== 'link') as { type: string; attrs?: Record<string, unknown> }[]),
      plain_text: node.text,
      href,
    }]
  }
  if (node.type === 'hardBreak') {
    return [{ type: 'text', text: { content: '\n' }, annotations: marksToAnnotations(), plain_text: '\n' }]
  }
  if (node.content) {
    return node.content.flatMap(proseMirrorTextToRichText)
  }
  return []
}

function proseMirrorNodeContentToRichText(node: JSONContent): RichTextToken[] {
  if (node.type === 'listItem') {
    const paragraph = node.content?.find(child => child.type === 'paragraph')
    return paragraph?.content?.flatMap(proseMirrorTextToRichText) || []
  }

  return node.content?.flatMap(proseMirrorTextToRichText) || []
}

const EQUATION_CODE_BLOCK_LANGUAGE = 'math'

// Block type -> ProseMirror node type
const BLOCK_TYPE_MAP: Record<string, string> = {
  'paragraph': 'paragraph',
  'heading_1': 'heading',
  'heading_2': 'heading',
  'heading_3': 'heading',
  'bulleted_list_item': 'listItem',
  'numbered_list_item': 'listItem',
  'code': 'codeBlock',
  'equation': 'codeBlock',
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
    return nodeWithInlineText('heading', richTextPlainText(tokens), { level })
  }

  if (block.block_type === 'code' || block.block_type === 'equation') {
    let lang: string | undefined
    if (block.block_type === 'equation') {
      lang = EQUATION_CODE_BLOCK_LANGUAGE
    } else {
      try { lang = JSON.parse(block.properties_json || '{}').language } catch { /* ignore */ }
    }
    return nodeWithInlineText('codeBlock', richTextPlainText(tokens), { language: lang })
  }

  if (block.block_type === 'bulleted_list_item' || block.block_type === 'numbered_list_item') {
    return listItemBlockToProseMirror(block)
  }

  const pmType = BLOCK_TYPE_MAP[block.block_type] || 'paragraph'
  return nodeWithInlineText(pmType, richTextPlainText(tokens))
}

function listItemBlockToProseMirror(block: DocumosaBlock): JSONContent {
  let tokens: RichTextToken[] = []
  try { tokens = JSON.parse(block.content_json) } catch { /* empty */ }
  return {
    type: 'listItem',
    content: [nodeWithInlineText('paragraph', richTextPlainText(tokens))],
  }
}

// ProseMirror JSON node -> Documosa Block
export function proseMirrorNodeToBlock(node: JSONContent): DocumosaBlock {
  const tokens = proseMirrorNodeContentToRichText(node)

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
    const language = (node.attrs as { language?: string })?.language
    if (language === EQUATION_CODE_BLOCK_LANGUAGE) {
      blockType = 'equation'
    } else {
      blockType = 'code'
      props.language = language || 'plain text'
    }
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
  const content: JSONContent[] = []
  let pendingList: JSONContent | null = null
  let pendingListType: 'bulletList' | 'orderedList' | null = null

  const flushList = () => {
    if (pendingList) {
      content.push(pendingList)
      pendingList = null
      pendingListType = null
    }
  }

  for (const block of blocks.filter(b => !b.id || b.block_type !== 'divider')) {
    const listType = block.block_type === 'bulleted_list_item'
      ? 'bulletList'
      : block.block_type === 'numbered_list_item'
        ? 'orderedList'
        : null

    if (!listType) {
      flushList()
      content.push(blockToProseMirror(block))
      continue
    }

    if (pendingListType !== listType) {
      flushList()
      pendingList = { type: listType, content: [] }
      pendingListType = listType
    }

    pendingList?.content?.push(listItemBlockToProseMirror(block))
  }

  flushList()
  return { type: 'doc', content }
}

export type { RichTextToken, Annotations, DocumosaBlock }
