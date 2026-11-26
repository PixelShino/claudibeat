// Pure logic of the shared folder: merging the index with the sessions' own
// files, what needs the person, and the words and bars the band draws.
// No `$` here: register.tsx reads and writes, this decides.

import type { AckFile, AgentLine, IndexEntry, Limit, Row, SessionFile, SessionState } from '../types'

/** A file whose heartbeat is older than this no longer speaks for its session. */
export const STALE_MS = 90_000
/** Idle sessions quieter than this leave the list. */
export const IDLE_KEEP_MS = 24 * 3600_000
/** No step and no tool call for this long while running: «застрял». */
export const STUCK_MS = 180_000

const ORDER: Record<SessionState, number> = { waiting: 0, done: 1, working: 2, idle: 3 }

/** `D:\Code\Projects\my-app` → `my-app`; the app's own scratch folders → «без папки». */
export function projectOf(cwd: string): string {
  if (/scratch-workspaces/i.test(cwd)) return 'без папки'
  const parts = cwd.split(/[\\/]+/).filter(Boolean)
  return parts[parts.length - 1] ?? cwd
}

export function clip(text: string, n: number): string {
  const one = text.replace(/\s+/g, ' ').trim()
  return one.length > n ? `${one.slice(0, n - 1).trimEnd()}…` : one
}

/**
 * The sessions as the person sees them: every session of the index, each one
 * whose own file is fresh speaking for itself, and files without an index row
 * (a session before the leader's first listing) kept too.
 */
export function mergeRows(
  index: readonly IndexEntry[],
  files: readonly SessionFile[],
  acks: Readonly<Record<string, AckFile>>,
  selfKey: string | null,
  now: number,
): Row[] {
  const byDesktop = new Map<string, SessionFile>()
  const loose: SessionFile[] = []
  for (const f of files) {
    if (f.desktopId) {
      const held = byDesktop.get(f.desktopId)
      if (!held || held.heartbeatAt < f.heartbeatAt) byDesktop.set(f.desktopId, f)
    } else loose.push(f)
  }

  const rows: Row[] = []
  for (const s of index) {
    const f = byDesktop.get(s.desktopId)
    byDesktop.delete(s.desktopId)
    rows.push(rowOf(s, f, acks, selfKey, now))
  }
  for (const f of [...byDesktop.values(), ...loose]) rows.push(rowOf(null, f, acks, selfKey, now))

  return rows
    .filter(r => r.isSelf || r.state !== 'idle' || now - r.lastActivityAt < IDLE_KEEP_MS)
    .sort((a, b) => sortKey(a) - sortKey(b) || b.lastActivityAt - a.lastActivityAt)
}

const sortKey = (r: Row) => (r.needsYou ? ORDER[r.state] : r.state === 'working' ? 2 : 3)

function rowOf(
  s: IndexEntry | null,
  f: SessionFile | undefined,
  acks: Readonly<Record<string, AckFile>>,
  selfKey: string | null,
  now: number,
): Row {
  const isLive = f !== undefined && now - f.heartbeatAt < STALE_MS
  const key = f?.key ?? s?.desktopId ?? 'unknown'
  const lastActivityAt = Math.max(s?.lastActivityAt ?? 0, f?.heartbeatAt ?? 0)
  // A file gone quiet keeps its words but not its state: the index says whether it runs.
  // A quiet file the app does not know (a closed terminal session) cannot be opened: it waits for nobody.
  const keepsWord = f !== undefined && (s !== null || f.desktopId !== null) && (f.state === 'waiting' || f.state === 'done')
  const state: SessionState = isLive ? f.state : s?.isRunning ? 'working' : keepsWord ? f.state : 'idle'
  const since = isLive || (f && state === f.state) ? f!.stateSince : s?.lastActivityAt ?? 0
  const ack = acks[key] ?? (s ? acks[s.desktopId] : undefined)
  const isSelf = selfKey !== null && (key === selfKey || s?.desktopId === selfKey)
  const needsYou = !isSelf && (state === 'waiting' || state === 'done') && !(ack && ack.ackAt >= since)
  return {
    key,
    desktopId: s?.desktopId ?? f?.desktopId ?? null,
    title: s?.title || f?.title || projectOf(f?.cwd ?? s?.cwd ?? ''),
    project: projectOf(s?.cwd ?? f?.cwd ?? ''),
    cwd: s?.cwd || f?.cwd || '',
    link: s?.link ?? f?.link ?? null,
    state,
    waitingReason: state === 'waiting' ? f?.waitingReason ?? null : null,
    ask: state === 'waiting' ? f?.ask ?? null : null,
    since,
    doing: isLive && state === 'working' ? f.doing : null,
    contextPercent: isLive ? f.context?.percent ?? null : null,
    agents: isLive ? f.agents : [],
    isSelf,
    isLive,
    needsYou,
    lastActivityAt,
  }
}

export function attention(rows: readonly Row[]): { waiting: number; done: number } {
  let waiting = 0
  let done = 0
  for (const r of rows) {
    if (!r.needsYou) continue
    if (r.state === 'waiting') waiting += 1
    else done += 1
  }
  return { waiting, done }
}

export function runningCount(rows: readonly Row[]): number {
  return rows.filter(r => r.state === 'working').length
}

export function liveAgents(rows: readonly Row[]): Array<AgentLine & { session: string }> {
  return rows.flatMap(r => r.agents.filter(a => a.status === 'running').map(a => ({ ...a, session: r.title })))
}

export const isStuck = (a: AgentLine, now: number) => a.status === 'running' && a.doing === null && now - a.lastAt >= STUCK_MS

// ─── limits ─────────────────────────────────────────────────────────────────

/** The engine's windows (`five_hour`, `seven_day`, a gateway's own) as the contract keeps them. */
export function limitsOf(windows: ReadonlyArray<{ kind: string; percentUsed: number; resetsAt?: string }>): Limit[] {
  return windows.map(w => {
    const at = w.resetsAt ? Date.parse(w.resetsAt) : NaN
    return { window: w.kind, percent: Math.round(w.percentUsed), resetsAt: Number.isFinite(at) ? at : null }
  })
}

export function limitLabel(window: string): string {
  if (window === 'five_hour') return '5ч'
  if (window === 'seven_day') return '7д'
  if (/opus/i.test(window)) return 'Opus 7д'
  if (/sonnet/i.test(window)) return 'Sonnet 7д'
  return window.replace(/_/g, ' ')
}

export type Tone = 'calm' | 'warn' | 'alarm'

export function tone(percent: number, warnAt = 60, alarmAt = 85): Tone {
  return percent >= alarmAt ? 'alarm' : percent >= warnAt ? 'warn' : 'calm'
}

/** `▰▰▱▱`: `cells` cells, a part from 1 % on shows as one. */
export function bar(percent: number, cells = 4): string {
  const p = Math.max(0, Math.min(100, percent))
  const full = p === 0 ? 0 : Math.max(1, Math.round((p / 100) * cells))
  return '▰'.repeat(full) + '▱'.repeat(cells - full)
}

// ─── time ───────────────────────────────────────────────────────────────────

/** `0:47`, `12:04`, `1:02:03`: how long something has run. */
export function clockText(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = String(s % 60).padStart(2, '0')
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`
}

/** `сейчас`, `4 мин`, `2 ч`, `3 д`: how long ago, short. */
export function agoText(ms: number): string {
  const m = Math.floor(Math.max(0, ms) / 60_000)
  if (m < 1) return 'сейчас'
  if (m < 60) return `${m} мин`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h} ч`
  return `${Math.floor(h / 24)} д`
}

/** `↻2:10`, `↻3д`: until a window resets. */
export function resetText(at: number | null, now: number): string {
  if (at === null) return ''
  const m = Math.max(0, Math.round((at - now) / 60_000))
  if (m >= 48 * 60) return `↻${Math.round(m / 1440)}д`
  return `↻${Math.floor(m / 60)}:${String(m % 60).padStart(2, '0')}`
}

/** `1 сессия`, `2 сессии`, `5 сессий`. */
export function plural(n: number, one: string, few: string, many: string): string {
  const d = n % 10
  const dd = n % 100
  if (d === 1 && dd !== 11) return one
  if (d >= 2 && d <= 4 && (dd < 12 || dd > 14)) return few
  return many
}

// ─── what a session or an agent is doing ─────────────────────────────────────

/** `claude-opus-5-5` → `opus-5.5`, `claude-haiku-4-5-20251001` → `haiku-4.5`; an alias as it is. */
export function shortModel(model: string): string {
  const m = /^(?:claude-)?([a-z]+)-(\d+)(?:-(\d{1,2}))?(?:-\d{8})?(\[1m\])?$/i.exec(model.trim())
  if (!m) return model.replace(/^claude-/, '')
  return `${m[1]}-${m[2]}${m[3] ? `.${m[3]}` : ''}`
}

const fileName = (p: unknown) => (typeof p === 'string' ? p.split(/[\\/]/).pop() ?? p : '')

/** One short Russian line for a tool call: what the person would call it. */
export function toolLine(tool: string, input: Readonly<Record<string, unknown>>): string {
  const str = (k: string) => (typeof input[k] === 'string' ? (input[k] as string) : '')
  switch (tool) {
    case 'Bash':
    case 'PowerShell': {
      const what = str('description') || str('command')
      return clip(`запускает: ${what}`, 80)
    }
    case 'Read':
      return clip(`читает ${fileName(input.file_path)}`, 80)
    case 'Edit':
    case 'Write':
    case 'NotebookEdit':
      return clip(`правит ${fileName(input.file_path ?? input.notebook_path)}`, 80)
    case 'Grep':
    case 'Glob':
      return clip(`ищет ${str('pattern')}`, 80)
    case 'WebSearch':
    case 'WebFetch':
      return 'ищет в сети'
    case 'Agent':
      return clip(`запускает агента: ${str('description')}`, 80)
    case 'AskUserQuestion':
      return 'спрашивает тебя'
    case 'Skill':
      return clip(`берёт скилл ${str('skill')}`, 80)
    case 'ToolSearch':
      return 'подгружает инструменты'
  }
  const mcp = /^mcp__([^_]+(?:_[^_]+)*)__(.+)$/.exec(tool)
  if (mcp) {
    const server = mcp[1]!
    if (server.startsWith('ccd_')) return 'работает с приложением'
    if (/browser|playwright|chrome/i.test(server)) return 'работает в браузере'
    return clip(`зовёт ${server.replace(/^\w+-\w{8,}$/, 'сервис')}`, 80)
  }
  return clip(tool, 80)
}

/** The question an AskUserQuestion call puts, short. */
export function questionOf(input: Readonly<Record<string, unknown>>): string | null {
  const qs = input.questions
  if (!Array.isArray(qs) || qs.length === 0) return null
  const first = qs[0] as { question?: unknown }
  const text = typeof first.question === 'string' ? first.question : null
  if (!text) return null
  return clip(qs.length > 1 ? `${text} (+${qs.length - 1})` : text, 140)
}

// ─── reading files ──────────────────────────────────────────────────────────

/** JSON or null: a torn write, a missing field of the version, anything else. */
export function parseJson<T extends { v: number }>(text: string | null): T | null {
  if (!text) return null
  try {
    const value = JSON.parse(text) as T
    return value && typeof value === 'object' && value.v === 1 ? value : null
  } catch {
    return null
  }
}

/** A file name the contract allows for a key: `local_…`, `cli-…`. */
export const safeKey = (key: string) => key.replace(/[^A-Za-z0-9_.-]/g, '_')

/** The index from `list_sessions`' JSON text. */
export function indexFromList(text: string): IndexEntry[] | null {
  try {
    const list = JSON.parse(text) as Array<Record<string, unknown>>
    if (!Array.isArray(list)) return null
    return list
      .filter(s => typeof s.sessionId === 'string' && s.isArchived !== true)
      .map(s => ({
        desktopId: s.sessionId as string,
        title: typeof s.title === 'string' ? s.title : '',
        cwd: typeof s.cwd === 'string' ? s.cwd : '',
        isRunning: s.isRunning === true,
        lastActivityAt: typeof s.lastActivityAt === 'string' ? Date.parse(s.lastActivityAt) || 0 : 0,
        link: typeof s.link === 'string' ? s.link : null,
        pinned: s.pinned === true,
        // A chat another chat started is its helper, unless the person took it
        // in: filed it into a sidebar group or pinned it.
        parent: typeof s.startedBy === 'string' && !s.group && s.pinned !== true ? s.startedBy : null,
      }))
  } catch {
    return null
  }
}
