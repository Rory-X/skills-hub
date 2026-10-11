import { memo, useCallback, useEffect, useMemo, useState } from 'react'
import {
  ChevronDown,
  ChevronRight,
  CircleAlert,
  File,
  Folder,
  FolderOpen,
  Image as ImageIcon,
  RefreshCw,
  X,
} from 'lucide-react'
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter'
import { oneDark, oneLight } from 'react-syntax-highlighter/dist/esm/styles/prism'
import Markdown from 'react-markdown'
import remarkFrontmatter from 'remark-frontmatter'
import remarkGfm from 'remark-gfm'
import { openUrl } from '@tauri-apps/plugin-opener'
import type { TFunction } from 'i18next'
import type {
  SkillPreviewContent,
  SkillPreviewFile,
  SkillPreviewManifest,
  SkillPreviewSource,
} from './types'

type SkillPreviewRequest = {
  title: string
  description?: string | null
  source: SkillPreviewSource
  refreshable?: boolean
}

type SkillPreviewDrawerProps = {
  open: boolean
  request: SkillPreviewRequest | null
  isDark: boolean
  invokeTauri: <T>(command: string, args?: Record<string, unknown>) => Promise<T>
  onClose: () => void
  t: TFunction
}

type TreeNode = {
  name: string
  path: string
  isDir: boolean
  size: number
  children: TreeNode[]
}

const EXT_LANG: Record<string, string> = {
  ts: 'typescript', tsx: 'tsx', js: 'javascript', jsx: 'jsx', py: 'python', rs: 'rust',
  go: 'go', rb: 'ruby', java: 'java', kt: 'kotlin', swift: 'swift', c: 'c', cpp: 'cpp',
  h: 'c', hpp: 'cpp', cs: 'csharp', css: 'css', scss: 'scss', less: 'less', html: 'html',
  xml: 'xml', json: 'json', yaml: 'yaml', yml: 'yaml', toml: 'toml', sh: 'bash', bash: 'bash',
  zsh: 'bash', sql: 'sql', graphql: 'graphql', dockerfile: 'docker', lua: 'lua', r: 'r',
  dart: 'dart', php: 'php', pl: 'perl', ex: 'elixir', exs: 'elixir', erl: 'erlang',
  hs: 'haskell', vim: 'vim', ini: 'ini', cfg: 'ini', diff: 'diff', patch: 'diff',
}

function getLanguage(filename: string): string {
  const lower = filename.toLowerCase()
  if (lower === 'dockerfile' || lower.startsWith('dockerfile.')) return 'docker'
  if (lower === 'makefile' || lower === 'gnumakefile') return 'makefile'
  return EXT_LANG[lower.split('.').pop() ?? ''] ?? ''
}

function isMarkdown(filename: string): boolean {
  return /\.(md|mdx|markdown)$/i.test(filename)
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function buildTree(files: SkillPreviewFile[]): TreeNode[] {
  const root: TreeNode[] = []
  for (const file of files) {
    const parts = file.path.split('/').filter(Boolean)
    let nodes = root
    parts.forEach((name, index) => {
      const isLast = index === parts.length - 1
      if (isLast) {
        nodes.push({ name, path: file.path, isDir: false, size: file.size, children: [] })
        return
      }
      let dir = nodes.find((item) => item.isDir && item.name === name)
      if (!dir) {
        dir = { name, path: parts.slice(0, index + 1).join('/'), isDir: true, size: 0, children: [] }
        nodes.push(dir)
      }
      nodes = dir.children
    })
  }
  const sort = (nodes: TreeNode[]) => {
    nodes.sort((a, b) => {
      if (a.isDir !== b.isDir) return a.isDir ? -1 : 1
      if (!a.isDir && !b.isDir) {
        const aSkill = a.name.toLowerCase() === 'skill.md'
        const bSkill = b.name.toLowerCase() === 'skill.md'
        if (aSkill !== bSkill) return aSkill ? -1 : 1
      }
      return a.name.localeCompare(b.name)
    })
    nodes.forEach((node) => node.isDir && sort(node.children))
  }
  sort(root)
  return root
}

function toDataUrl(content: SkillPreviewContent): string | null {
  if (content.kind !== 'image' || !content.bytes || !content.mediaType) return null
  let binary = ''
  for (let index = 0; index < content.bytes.length; index += 8192) {
    binary += String.fromCharCode(...content.bytes.slice(index, index + 8192))
  }
  return `data:${content.mediaType};base64,${btoa(binary)}`
}

function safeHref(href: string | undefined): string | undefined {
  if (!href) return undefined
  try {
    const parsed = new URL(href, window.location.href)
    if (parsed.protocol === 'http:' || parsed.protocol === 'https:' || parsed.protocol === 'mailto:') {
      return parsed.href
    }
  } catch {
    return undefined
  }
  return undefined
}

function resolveRelativePath(baseFile: string, imagePath: string): string | null {
  if (!imagePath || /^(data:|https?:|\/\/|mailto:|#)/i.test(imagePath)) return null
  const base = baseFile.split('/').slice(0, -1)
  for (const part of imagePath.split('/')) {
    if (!part || part === '.') continue
    if (part === '..') base.pop()
    else base.push(part)
  }
  return base.join('/')
}

const PreviewLink = ({ href, children }: { href?: string; children?: React.ReactNode }) => {
  const safe = safeHref(href)
  if (!safe) return <span>{children}</span>
  const handleClick = async (event: React.MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault()
    try {
      await openUrl(safe)
    } catch {
      window.open(safe, '_blank', 'noopener,noreferrer')
    }
  }
  return <a href={safe} onClick={(event) => void handleClick(event)}>{children}</a>
}

type PreviewImageProps = {
  src?: string
  alt?: string
  currentFile: string
  source: SkillPreviewSource
  invokeTauri: SkillPreviewDrawerProps['invokeTauri']
  t: TFunction
}

const PreviewImage = ({ src, alt, currentFile, source, invokeTauri, t }: PreviewImageProps) => {
  const [loaded, setLoaded] = useState(false)
  const [dataUrl, setDataUrl] = useState<string | null>(null)
  const relativePath = resolveRelativePath(currentFile, src ?? '')
  useEffect(() => {
    let cancelled = false
    if (!relativePath) return
    void invokeTauri<SkillPreviewContent>('read_skill_preview_file', {
      source,
      filePath: relativePath,
    }).then((result) => {
      if (!cancelled) setDataUrl(toDataUrl(result))
    }).catch(() => {
      if (!cancelled) setDataUrl(null)
    })
    return () => { cancelled = true }
  }, [invokeTauri, relativePath, source])

  if (relativePath) {
    return dataUrl ? <img className="skill-preview-markdown-image" src={dataUrl} alt={alt ?? ''} /> : <span className="skill-preview-image-placeholder">{t('skillPreview.imageUnavailable')}</span>
  }
  if (!src) return null
  if (!loaded) {
    return <button className="skill-preview-image-load" type="button" onClick={() => setLoaded(true)}>{t('skillPreview.loadRemoteImage')}</button>
  }
  const remoteSrc = src.startsWith('//') ? `${window.location.protocol}${src}` : src
  return <img className="skill-preview-markdown-image" src={remoteSrc} alt={alt ?? ''} />
}

const FileTreeNode = memo(({ node, depth, activeFile, expanded, onToggle, onSelect }: {
  node: TreeNode
  depth: number
  activeFile: string | null
  expanded: Set<string>
  onToggle: (path: string) => void
  onSelect: (path: string) => void
}) => {
  if (node.isDir) {
    const open = expanded.has(node.path)
    return <>
      <button type="button" className="skill-preview-tree-item skill-preview-tree-dir" style={{ paddingLeft: 12 + depth * 16 }} onClick={() => onToggle(node.path)}>
        {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        {open ? <FolderOpen size={15} /> : <Folder size={15} />}
        <span>{node.name}</span>
      </button>
      {open ? node.children.map((child) => <FileTreeNode key={child.path} node={child} depth={depth + 1} activeFile={activeFile} expanded={expanded} onToggle={onToggle} onSelect={onSelect} />) : null}
    </>
  }
  return <button type="button" className={`skill-preview-tree-item skill-preview-tree-file${activeFile === node.path ? ' active' : ''}`} style={{ paddingLeft: 30 + depth * 16 }} onClick={() => onSelect(node.path)}><File size={14} /><span>{node.name}</span></button>
})

const SkillPreviewDrawer = ({ open, request, isDark, invokeTauri, onClose, t }: SkillPreviewDrawerProps) => {
  const [manifest, setManifest] = useState<SkillPreviewManifest | null>(null)
  const [activeFile, setActiveFile] = useState<string | null>(null)
  const [content, setContent] = useState<SkillPreviewContent | null>(null)
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [loadingManifest, setLoadingManifest] = useState(false)
  const [loadingContent, setLoadingContent] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const loadManifest = useCallback(async (refresh = false) => {
    if (!request) return
    setLoadingManifest(true)
    setError(null)
    try {
      const next = await invokeTauri<SkillPreviewManifest>('list_skill_preview', { source: request.source, refresh })
      setManifest(next)
      const preferred = next.files.find((file) => file.path.toLowerCase() === 'skill.md')?.path ?? next.files[0]?.path ?? null
      setActiveFile((current) => current && next.files.some((file) => file.path === current) ? current : preferred)
      setExpanded(new Set())
    } catch (err) {
      setManifest(null)
      setActiveFile(null)
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoadingManifest(false)
    }
  }, [invokeTauri, request])

  useEffect(() => {
    if (!open || !request) return
    setManifest(null)
    setContent(null)
    setActiveFile(null)
    void loadManifest()
  }, [loadManifest, open, request])

  useEffect(() => {
    if (!open) return
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose() }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [onClose, open])

  useEffect(() => {
    if (!request || !activeFile) return
    let cancelled = false
    setLoadingContent(true)
    setContent(null)
    void invokeTauri<SkillPreviewContent>('read_skill_preview_file', { source: request.source, filePath: activeFile })
      .then((next) => { if (!cancelled) setContent(next) })
      .catch((err) => { if (!cancelled) setContent({ kind: 'unsupported', reason: err instanceof Error ? err.message : String(err) }) })
      .finally(() => { if (!cancelled) setLoadingContent(false) })
    return () => { cancelled = true }
  }, [activeFile, invokeTauri, request])

  const tree = useMemo(() => buildTree(manifest?.files ?? []), [manifest?.files])
  const activeEntry = manifest?.files.find((file) => file.path === activeFile)
  const toggleDir = useCallback((path: string) => setExpanded((current) => {
    const next = new Set(current)
    if (next.has(path)) next.delete(path)
    else next.add(path)
    return next
  }), [])
  const previewBody = content?.kind === 'text' && content.content != null ? (
    isMarkdown(activeFile ?? '') ? <div className="skill-preview-markdown"><Markdown remarkPlugins={[remarkGfm, remarkFrontmatter]} components={{ a: ({ href, children }) => <PreviewLink href={href}>{children}</PreviewLink>, img: ({ src, alt }) => <PreviewImage src={src} alt={alt} currentFile={activeFile ?? ''} source={request?.source ?? { kind: 'managed', skillId: '' }} invokeTauri={invokeTauri} t={t} /> }}>{content.content}</Markdown></div>
      : <SyntaxHighlighter language={getLanguage(activeFile ?? '') || 'text'} style={isDark ? oneDark : oneLight} showLineNumbers wrapLongLines>{content.content}</SyntaxHighlighter>
  ) : content?.kind === 'image' ? <img className="skill-preview-image" src={toDataUrl(content) ?? undefined} alt={activeFile ?? ''} /> : content?.kind === 'unsupported' ? <div className="skill-preview-empty"><CircleAlert size={20} /><span>{t(`skillPreview.unsupported.${content.reason === 'binary' || content.reason === 'too_large' ? content.reason : 'read_error'}`)}</span></div> : null

  if (!open || !request) return null
  return <div className="skill-preview-drawer" role="dialog" aria-modal="true" aria-label={t('skillPreview.title')}>
    <div className="skill-preview-header">
      <div className="skill-preview-heading"><div className="skill-preview-title">{request.title}</div>{request.description ? <div className="skill-preview-description">{request.description}</div> : null}</div>
      <div className="skill-preview-header-actions">
        {request.refreshable ? <button className="icon-btn" type="button" onClick={() => void loadManifest(true)} disabled={loadingManifest} aria-label={t('skillPreview.refresh')} title={t('skillPreview.refresh')}><RefreshCw size={16} className={loadingManifest ? 'spin' : undefined} /></button> : null}
        <button className="icon-btn" type="button" onClick={onClose} aria-label={t('close')} title={t('close')}><X size={18} /></button>
      </div>
    </div>
    <div className="skill-preview-layout">
      <aside className="skill-preview-sidebar"><div className="skill-preview-sidebar-title">{t('skillPreview.files')} {manifest ? <span>{manifest.files.length}</span> : null}</div>{manifest?.truncated ? <div className="skill-preview-truncated">{t('skillPreview.truncated')}</div> : null}{loadingManifest ? <div className="skill-preview-state">{t('skillPreview.loadingFiles')}</div> : error ? <div className="skill-preview-state error"><CircleAlert size={16} />{error}</div> : tree.length === 0 ? <div className="skill-preview-state">{t('skillPreview.noFiles')}</div> : <div className="skill-preview-tree">{tree.map((node) => <FileTreeNode key={node.path} node={node} depth={0} activeFile={activeFile} expanded={expanded} onToggle={toggleDir} onSelect={setActiveFile} />)}</div>}</aside>
      <section className="skill-preview-content"><div className="skill-preview-file-header">{activeFile ? <><span><File size={14} />{activeFile}</span>{activeEntry ? <small>{formatSize(activeEntry.size)}</small> : null}</> : <span>{t('skillPreview.selectFile')}</span>}</div><div className="skill-preview-file-body">{loadingContent ? <div className="skill-preview-state">{t('skillPreview.loadingContent')}</div> : previewBody ?? <div className="skill-preview-state"><ImageIcon size={20} />{t('skillPreview.selectFile')}</div>}</div></section>
    </div>
  </div>
}

export type { SkillPreviewRequest }
export default memo(SkillPreviewDrawer)
