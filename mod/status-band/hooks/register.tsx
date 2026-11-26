import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register, Timer } from 'claude-code'

import type {
  AccountFile,
  AgentLine,
  AssignmentItem,
  AssignmentsFile,
  BoardCard,
  BoardReply,
  BoardRequest,
  BoardView,
  IndexEntry,
  IndexFile,
  Limit,
  Row,
  SessionFile,
  SessionState,
  Shared,
  TasksFile,
  WaitingReason,
} from '../types'
import {
  REQUEST_TTL_MS,
  addAssignment,
  assignComment,
  assignMessage,
  assignmentOf,
  chatTitle,
  isDelivered,
  moveText,
  parseRequest,
  pendingNames,
  pruneAssignments,
  remember,
  sessionIdIn,
  sessionTitleOf,
  verdictFor,
} from './board-requests'
import type { Verdict } from './board-requests'
import { PlaneError, backlogText, createPlane, itemText, parseQuickAdd, tasksFile } from './plane'
import type { Board, Fetch, Move, WorkItem } from './plane'
import {
  clip,
  indexFromList,
  limitsOf,
  mergeRows,
  parseJson,
  projectOf,
  questionOf,
  safeKey,
  shortModel,
  toolLine,
} from './shared'

type $ = EngineInterface

// The mod draws nothing (owner, 17.11.2026): it feeds the shared folder that
// claudibeat shows, runs claudibeat's board requests and gives the model the
// backlog tool. Toasts, sounds and the sidebar's dot are claudibeat's too.

// ─── values kept between beats ──────────────────────────────────────────────

const EMPTY_SHARED: Shared = { rows: [], limits: [], tasks: null, readAt: 0 }
const EMPTY_BOARD: BoardView = { todo: [], doing: [], doneToday: [], projects: [], fetchedAt: 0, error: null }

const shared = atom({ plugin: 'status-band', key: 'shared' } as const, EMPTY_SHARED)
const board = atom({ plugin: 'status-band', key: 'board' } as const, EMPTY_BOARD)

// ─── timing ─────────────────────────────────────────────────────────────────

const TICK_MS = 2_000
const HEARTBEAT_MS = 15_000
const INDEX_MS = 10_000
const LOCK_TTL_MS = 20_000
/** This mod's generation: it serves the board's requests and keeps assignments.json. */
const LEAD_CODE = 3
const ACCOUNT_MS = 30_000
const PLANE_MS = 60_000
const IDENTIFY_MS = 30_000

const TOOL = 'backlog'
const TOOL_NAME = `mcp__status-band__${TOOL}`

// ─── this session ───────────────────────────────────────────────────────────

let dir = ''
let cliId = ''
let cwd = ''
let desktopId: string | null = null
let title: string | null = null
let link: string | null = null
let identifiedAt = 0
let isWindows = true

let own: { state: SessionState; waitingReason: WaitingReason | null; ask: string | null; stateSince: number; doing: string | null } = {
  state: 'idle',
  waitingReason: null,
  ask: null,
  stateSince: 0,
  doing: null,
}
/** What the turn's wrap-up asked of the person, read when the turn ends. */
let pendingAsk: { ask: string | null; isBlocked: boolean } | null = null
let agents: AgentLine[] = []
let context: SessionFile['context'] = null
let oldKey: string | null = null

let ticker: Timer | null = null
let isBeginning = false
let isBusy = false
let isDirty = true
let lastBeat = 0
let lastIndex = 0
let lastAccount = 0
let lastPlane = 0
let lastLimits = ''
let isLeader = false
let canList = true
/** mtimes of what the last read parsed, so a quiet file is not read again. */
const seen = new Map<string, { mtime: number; value: unknown }>()

let planeOptions = { url: '', workspace: '', apiKey: '', inbox: 'INBOX' }
let plane: ReturnType<typeof createPlane> | null = null
let planeKeyFailed = false

const myKey = () => (desktopId ? desktopId : `cli-${cliId}`)

// ─── the shared folder ──────────────────────────────────────────────────────

async function home($: $): Promise<string> {
  const h = (await $.env.get('USERPROFILE')) ?? (await $.env.get('HOME')) ?? ''
  return h.replace(/\\/g, '/')
}

async function readFile<T extends { v: number }>($: $, path: string, mtime?: number): Promise<T | null> {
  const cached = seen.get(path)
  if (mtime !== undefined && cached && cached.mtime === mtime) return cached.value as T | null
  let text: string | null = null
  try {
    text = await $.fs.read(path)
  } catch {
    text = null
  }
  const value = parseJson<T>(text)
  // A torn write keeps the last good value.
  if (value === null && cached) return cached.value as T | null
  if (mtime !== undefined) seen.set(path, { mtime, value })
  return value
}

async function readDir<T extends { v: number }>($: $, sub: string): Promise<Array<{ name: string; value: T }>> {
  let entries: Awaited<ReturnType<$['fs']['list']>> = []
  try {
    entries = await $.fs.list(`${dir}/${sub}`)
  } catch {
    return []
  }
  const out: Array<{ name: string; value: T }> = []
  for (const e of entries) {
    if (e.kind !== 'file' || !e.name.endsWith('.json')) continue
    const value = await readFile<T>($, `${dir}/${sub}/${e.name}`, e.mtimeMs)
    if (value) out.push({ name: e.name.slice(0, -5), value })
  }
  return out
}

/** The `.json` files of a subfolder, by name without the extension. */
async function namesIn($: $, sub: string): Promise<string[]> {
  try {
    const entries = await $.fs.list(`${dir}/${sub}`)
    return entries.filter(e => e.kind === 'file' && e.name.endsWith('.json')).map(e => e.name.slice(0, -5))
  } catch {
    return []
  }
}

async function writeJson($: $, path: string, value: unknown): Promise<void> {
  try {
    await $.fs.write(`${dir}/${path}`, JSON.stringify(value))
  } catch {
    // the folder speaks for itself next beat
  }
}

function ownFile(now: number): SessionFile {
  return {
    v: 1,
    key: myKey(),
    desktopId,
    cliId,
    title,
    cwd,
    project: projectOf(cwd),
    link,
    state: own.state,
    waitingReason: own.waitingReason,
    ask: own.ask,
    stateSince: own.stateSince,
    doing: own.state === 'working' ? own.doing : null,
    context,
    agents: agents.slice(-8),
    heartbeatAt: now,
  }
}

async function beat($: $, now: number) {
  if (oldKey) {
    // The file under the engine's id before the app's id was known: gone quiet for good.
    await writeJson($, `sessions/${safeKey(oldKey)}.json`, { ...ownFile(0), key: oldKey, desktopId: null, state: 'idle', heartbeatAt: 0 })
    oldKey = null
  }
  await writeJson($, `sessions/${safeKey(myKey())}.json`, ownFile(now))
  lastBeat = now
  isDirty = false
}

async function setState($: $, state: SessionState, extra: Partial<typeof own> = {}) {
  const now = await $.clock.now()
  const changed = own.state !== state
  own = {
    ...own,
    ...extra,
    state,
    stateSince: changed ? now : own.stateSince,
    ...(state !== 'waiting' ? { waitingReason: null, ask: null } : {}),
    ...(state !== 'working' ? { doing: null } : {}),
  }
  isDirty = true
  if (changed && dir) void tick($)
}

// ─── who this session is in the app ─────────────────────────────────────────

function textOf(result: Awaited<ReturnType<$['mcp']['call']>>): string {
  return result.content.map(c => (c as { text?: unknown }).text).filter((t): t is string => typeof t === 'string').join('')
}

async function identify($: $, now: number) {
  if (now - identifiedAt < (desktopId ? 300_000 : IDENTIFY_MS)) return
  identifiedAt = now
  try {
    const got = await $.mcp.call('ccd_session_mgmt', 'get_session', { session_id: 'self' })
    if (got.isError) return
    const s = JSON.parse(textOf(got)) as { sessionId?: string; title?: string; link?: string }
    if (typeof s.sessionId === 'string' && s.sessionId !== desktopId) {
      if (!desktopId) oldKey = `cli-${cliId}`
      desktopId = s.sessionId
    }
    if (typeof s.title === 'string' && s.title !== title) title = s.title
    if (typeof s.link === 'string') link = s.link
    isDirty = true
  } catch {
    // not under the desktop app: the engine's id stands
  }
}

// ─── the leader: one session lists the app's sessions and asks Plane ─────────

async function lead($: $, now: number) {
  if (!canList) return
  const lock = await readFile<{ v: 1; owner: string; at: number; code?: number }>($, `${dir}/index.lock`)
  // A leader on an older mod (it loaded before an update) cannot serve the
  // board's requests: a session with this code takes over from it. Older
  // code yields to any fresh lock not its own, so it does not fight back.
  if (lock && lock.owner !== myKey() && now - lock.at < LOCK_TTL_MS && (lock.code ?? 1) >= LEAD_CODE) {
    isLeader = false
    return
  }
  await writeJson($, 'index.lock', { v: 1, owner: myKey(), at: now, code: LEAD_CODE })
  seen.delete(`${dir}/index.lock`)
  isLeader = true
  if (now - lastIndex < INDEX_MS) return
  lastIndex = now
  try {
    const got = await $.mcp.call('ccd_session_mgmt', 'list_sessions', { limit: 40 })
    const sessions = got.isError ? null : indexFromList(textOf(got))
    if (!sessions) throw new Error('no list')
    const file: IndexFile = { v: 1, updatedAt: now, sessions }
    await writeJson($, 'index.json', file)
    await quietHelpers($, sessions, now)
  } catch {
    // This session cannot list: another one leads.
    canList = false
    isLeader = false
    await writeJson($, 'index.lock', { v: 1, owner: myKey(), at: 0, code: LEAD_CODE })
  }
}

/** Helper chats cleared already, by the activity time they were cleared at. */
const quieted = new Map<string, number>()

/**
 * claudibeat's «quiet helpers» setting (`tray.json`, `quietHelpers`, written
 * every 10 s): the app's blue dot is taken off a chat another chat started,
 * each time it has done something new. The person reads the parent instead.
 */
async function quietHelpers($: $, sessions: readonly IndexEntry[], now: number) {
  const tray = await readFile<{ v: 1; at: number; quietHelpers?: boolean }>($, `${dir}/tray.json`)
  if (!tray || tray.quietHelpers !== true || now - tray.at > 60_000) return
  for (const s of sessions) {
    if (!s.parent || s.isRunning || quieted.get(s.desktopId) === s.lastActivityAt) continue
    quieted.set(s.desktopId, s.lastActivityAt)
    try {
      await $.mcp.call('ccd_sidebar', 'set_unread', { session_id: s.desktopId, unread: false })
    } catch {
      // the sidebar tools are not there: the dot stays, nothing else depends on it
    }
  }
}

async function measure($: $, now: number) {
  if (now - lastAccount < ACCOUNT_MS) return
  lastAccount = now
  try {
    const usage = await $.session.usage()
    const percent = usage.context.percent ?? (usage.context.tokens && usage.context.window ? (usage.context.tokens / usage.context.window) * 100 : null)
    context = { percent: percent === null ? null : Math.round(percent), tokens: usage.context.tokens ?? null }
    isDirty = true
    const limits = limitsOf(usage.rateLimits)
    // An idle session keeps the windows of its last answer, hours old: it
    // writes only what it saw change, so the freshest word wins.
    const seenNow = JSON.stringify(limits)
    if (limits.length > 0 && seenNow !== lastLimits) {
      lastLimits = seenNow
      const file: AccountFile = { v: 1, updatedAt: now, limits }
      await writeJson($, 'account.json', file)
    }
  } catch {
    // the status line's figures are not there yet
  }
}

// ─── reading everyone ───────────────────────────────────────────────────────

async function gather($: $, now: number) {
  const [files, acks, index, account, tasks, boardFile] = await Promise.all([
    readDir<SessionFile>($, 'sessions'),
    readDir<{ v: 1; ackAt: number; by: string }>($, 'acks'),
    statRead<IndexFile>($, 'index.json'),
    statRead<AccountFile>($, 'account.json'),
    statRead<TasksFile>($, 'tasks.json'),
    statRead<BoardFile>($, 'board.json'),
  ])
  const ackMap: Record<string, { v: 1; ackAt: number; by: string }> = {}
  for (const a of acks) ackMap[a.name] = a.value
  // Our own row speaks from memory, not from a file a beat behind.
  const mine = ownFile(now)
  const others = files.map(f => f.value).filter(f => f.key !== mine.key && f.key !== `cli-${cliId}`)
  const rows = mergeRows(index?.sessions ?? [], [...others, mine], ackMap, myKey(), now)
  const limits: Limit[] = account?.limits ?? []

  const next: Shared = { rows, limits, tasks, readAt: Math.floor(now / 60_000) }
  const held = await read($, shared)
  if (JSON.stringify(held) !== JSON.stringify(next)) await update($, shared, () => next)

  if (boardFile) {
    const view = boardFromFile(boardFile)
    const heldBoard = await read($, board)
    if (heldBoard.fetchedAt !== view.fetchedAt || heldBoard.error !== view.error) await update($, board, () => view)
  }
}

async function statRead<T extends { v: number }>($: $, name: string): Promise<T | null> {
  const path = `${dir}/${name}`
  try {
    const st = await $.fs.stat(path)
    return await readFile<T>($, path, Math.round(st.mtimeMs))
  } catch {
    return null
  }
}

/** Opens a chat of the app by its `claude://` link; a failure stays silent. */
async function openSession($: $, url: string) {
  try {
    if (isWindows) await $.process.run(['rundll32.exe', 'url.dll,FileProtocolHandler', url], { timeoutMs: 10_000 })
    else await $.process.run(['open', url], { timeoutMs: 10_000 })
  } catch {
    // the chat is in the sidebar all the same
  }
}

// ─── the beat ───────────────────────────────────────────────────────────────

/** The app's id for this session when the desktop runs it (`local_…`); null elsewhere. */
async function hostId($: $): Promise<string | null> {
  if ((await $.env.get('CLAUDE_CODE_ENTRYPOINT')) !== 'claude-desktop') return null
  const id = (await $.env.get('CLAUDE_CODE_HOST_SESSION_ID')) ?? ''
  return /^local_[0-9a-f-]{36}$/i.test(id) ? id : null
}

/** A surface asked to draw: whoever draws is here, even if no start said so. */
function wake($: $) {
  if (ticker || isBeginning) return
  isBeginning = true
  try {
    $.clock.after(50, () => void begin($))
  } catch {
    isBeginning = false
  }
}

async function begin($: $) {
  if (ticker) return
  dir = `${await home($)}/.claude/status-band`
  const host = await hostId($)
  if (host && !desktopId) {
    desktopId = host
    link = `claude://claude.ai/epitaxy/${host}`
  }
  cliId = await $.session.id()
  cwd = await $.session.cwd()
  isWindows = ((await $.env.get('OS')) ?? '') === 'Windows_NT'
  own = { ...own, state: 'idle', stateSince: await $.clock.now() }
  ticker = $.clock.every(TICK_MS, () => void tick($))
  void tick($, true)
}

async function tick($: $, force = false) {
  if (isBusy || !dir) return
  isBusy = true
  try {
    const now = await $.clock.now()
    await identify($, now)
    await measure($, now)
    await settleAgents($, now)
    if (isDirty || force || now - lastBeat >= HEARTBEAT_MS) await beat($, now)
    await lead($, now)
    if (isLeader && now - lastPlane >= PLANE_MS) void refreshBoard($)
    await gather($, now)
    void serveRequests($, now)
  } catch {
    // one bad beat never stops the next
  } finally {
    isBusy = false
  }
}

// ─── Plane ─────────────────────────────────────────────────────────────────

type BoardFile = { v: 1; updatedAt: number; error: string | null; projects: string[]; todo: BoardCard[]; doing: BoardCard[]; doneToday: BoardCard[] }

function cardOf(w: WorkItem): BoardCard {
  return {
    key: w.key,
    title: w.title,
    project: w.project,
    group: w.group,
    stateName: w.stateName,
    priority: w.priority,
    description: clip(w.description, 600),
    url: w.url,
  }
}

function boardFromFile(f: BoardFile): BoardView {
  return { todo: f.todo, doing: f.doing, doneToday: f.doneToday, projects: f.projects, fetchedAt: f.updatedAt, error: f.error }
}

/**
 * Where Plane is and the key to it: the mod's own settings first, then
 * `~/.secrets/plane.json` (the file other tools may read, so one place holds
 * the server), then the plane MCP server's env in `~/.claude.json`.
 */
async function planeSettings($: $): Promise<{ url: string; workspace: string; apiKey: string }> {
  let url = planeOptions.url
  let workspace = planeOptions.workspace
  let apiKey = planeOptions.apiKey
  const h = await home($)
  if (!url || !workspace || !apiKey) {
    try {
      const file = JSON.parse(await $.fs.read(`${h}/.secrets/plane.json`)) as { base?: unknown; workspace?: unknown; key?: unknown }
      // A key belongs to its server: take the file's key only with the file's server.
      if (!url && typeof file.base === 'string') {
        url = file.base
        if (!apiKey && typeof file.key === 'string') apiKey = file.key
      }
      if (!workspace && typeof file.workspace === 'string') workspace = file.workspace
    } catch {
      // no such file on this machine
    }
  }
  if (!apiKey) {
    try {
      const config = JSON.parse(await $.fs.read(`${h}/.claude.json`)) as Record<string, unknown>
      apiKey = findPlaneKey(config, 0) ?? ''
    } catch {
      // no config to borrow from
    }
  }
  return { url: url.replace(/\/+$/, ''), workspace, apiKey }
}

function findPlaneKey(node: unknown, depth: number): string | null {
  if (!node || typeof node !== 'object' || depth > 6) return null
  const servers = (node as Record<string, unknown>).mcpServers as Record<string, { env?: Record<string, unknown> }> | undefined
  const key = servers?.plane?.env?.PLANE_API_KEY
  if (typeof key === 'string' && key) return key
  for (const value of Object.values(node as Record<string, unknown>)) {
    const k = findPlaneKey(value, depth + 1)
    if (k) return k
  }
  return null
}

async function planeOf($: $): Promise<ReturnType<typeof createPlane> | null> {
  if (plane) return plane
  if (planeKeyFailed) return null
  const { url, workspace, apiKey } = await planeSettings($)
  if (!apiKey || !url || !workspace) {
    planeKeyFailed = true
    return null
  }
  const fetch: Fetch = async (url, init) => {
    const r = await $.http.fetch(url, { method: init?.method ?? 'GET', headers: init?.headers ?? {}, ...(init?.body !== undefined ? { body: init.body } : {}) })
    return { status: r.status, ok: r.ok, text: r.text }
  }
  plane = createPlane(fetch, { url, workspace, apiKey })
  return plane
}

function planeMessage(err: unknown): string {
  if (err instanceof PlaneError) return err.message
  return 'Plane не ответил'
}

/** The same for a reply claudibeat shows: «Plane недоступен (host)» without the transport's words after it. */
const shortLine = (text: string) => clip(text.split(/:\s/)[0] || 'Plane не ответил', 90)
const planeShort = (err: unknown) => shortLine(planeMessage(err))

async function zoneOffset($: $): Promise<number> {
  // The engine's environment has no time zone of its own beyond the host's clock.
  return -new Date(await $.clock.now()).getTimezoneOffset()
}

let refreshing: Promise<Board | null> | null = null

/**
 * Asks Plane for the board and writes board.json and tasks.json. A call while
 * one runs gets that one; `isFresh` waits for it and asks again, for a change
 * made after it began.
 */
function refreshBoard($: $, isFresh = false): Promise<Board | null> {
  if (refreshing) return isFresh ? refreshing.then(() => refreshBoard($)) : refreshing
  const running = fetchBoard($).finally(() => {
    refreshing = null
  })
  refreshing = running
  return running
}

async function fetchBoard($: $): Promise<Board | null> {
  const now = await $.clock.now()
  lastPlane = now
  try {
    const p = await planeOf($)
    if (!p) {
      await writeBoardError($, now, 'Нет ключа Plane: впиши base, workspace и key в ~/.secrets/plane.json')
      return null
    }
    const got = await p.board({ now, tzOffsetMin: await zoneOffset($) })
    const file: BoardFile = {
      v: 1,
      updatedAt: now,
      error: null,
      projects: got.projects.map(x => x.identifier),
      todo: got.todo.map(cardOf),
      doing: got.doing.map(cardOf),
      doneToday: got.doneToday.map(cardOf),
    }
    await writeJson($, 'board.json', file)
    await writeJson($, 'tasks.json', tasksFile(got, now))
    await update($, board, () => boardFromFile(file))
    // Only what began before this fetch may look gone from «В работе».
    const doing = new Set(got.doing.map(w => w.key))
    await changeAssignments($, items => pruneAssignments(items, doing, now))
    return got
  } catch (err) {
    await writeBoardError($, now, planeMessage(err))
    return null
  }
}

async function writeBoardError($: $, now: number, error: string) {
  try {
    const held = await read($, board)
    const file: BoardFile = { v: 1, updatedAt: now, error, projects: held.projects, todo: held.todo, doing: held.doing, doneToday: held.doneToday }
    await writeJson($, 'board.json', file)
    const t: TasksFile = { v: 1, updatedAt: now, ok: false, error, counts: { todo: held.todo.length, doing: held.doing.length, doneToday: held.doneToday.length }, top: [] }
    await writeJson($, 'tasks.json', t)
    await update($, board, b => ({ ...b, error, fetchedAt: now }))
  } catch {
    // the next fetch says it again
  }
}

// ─── claudibeat's board: requests and replies ───────────────────────────────

/** Request ids this session ran or dropped: never run twice. */
const handled = new Set<string>()
/** Replies the leader has already taken into assignments.json. */
const merged = new Set<string>()
let isServing = false

/** assignments.json: the leader alone writes it. */
async function changeAssignments($: $, fn: (items: AssignmentItem[]) => AssignmentItem[]) {
  if (!isLeader) return
  // Read past the mtime cache: our own last write may share its millisecond.
  const file = await readFile<AssignmentsFile>($, `${dir}/assignments.json`)
  const items = file?.items ?? []
  const next = fn(items)
  if (file && JSON.stringify(next) === JSON.stringify(items)) return
  const out: AssignmentsFile = { v: 1, updatedAt: await $.clock.now(), items: next }
  await writeJson($, 'assignments.json', out)
}

/** The leader takes the assignments other sessions reported in their replies. */
async function collectAssignments($: $, replies: readonly string[]) {
  const fresh = replies.filter(n => !merged.has(n))
  if (fresh.length === 0) return
  const found: AssignmentItem[] = []
  for (const name of fresh) {
    let raw: unknown
    try {
      raw = JSON.parse(await $.fs.read(`${dir}/replies/${name}.json`))
    } catch {
      continue // torn or gone: next beat, or never
    }
    remember(merged, name)
    const a = assignmentOf(raw)
    if (a) found.push(a)
  }
  if (found.length > 0) await changeAssignments($, items => found.reduce(addAssignment, items))
}

/** Waiting this long, a request is anybody's who claims it first. */
const ORPHAN_MS = 8_000

/**
 * One runner per request: write `claims/<id>.json`, read it back a moment
 * later; whoever's name stands there runs it. claudibeat sweeps the claims.
 */
async function claim($: $, id: string): Promise<boolean> {
  const path = `${dir}/claims/${safeKey(id)}.json`
  try {
    const held = JSON.parse(await $.fs.read(path)) as { owner?: string }
    if (held.owner !== myKey()) return false
  } catch {
    // nobody yet
  }
  await writeJson($, `claims/${safeKey(id)}.json`, { v: 1, owner: myKey(), at: await $.clock.now() })
  await $.clock.sleep(300)
  try {
    return (JSON.parse(await $.fs.read(path)) as { owner?: string }).owner === myKey()
  } catch {
    return false
  }
}

async function serveRequests($: $, now: number) {
  if (isServing || !dir) return
  isServing = true
  try {
    const [requests, replies] = await Promise.all([namesIn($, 'requests'), namesIn($, 'replies')])
    if (isLeader) await collectAssignments($, replies)
    const pending = pendingNames(requests, new Set(replies), handled)
    if (pending.length === 0) return
    let leaderKey: string | null = isLeader ? myKey() : null
    if (!isLeader) {
      const lock = await readFile<{ v: 1; owner: string; at: number }>($, `${dir}/index.lock`)
      leaderKey = lock && lock.at > 0 && now - lock.at < LOCK_TTL_MS ? lock.owner : null
    }
    const rows = (await read($, shared)).rows
    for (const name of pending) {
      let raw: unknown
      try {
        raw = JSON.parse((await $.fs.read(`${dir}/requests/${name}.json`)).replace(/^﻿/, ''))
      } catch {
        continue // half written or already gone: the next beat sees
      }
      const req = parseRequest(raw)
      if (!req) {
        remember(handled, name)
        if (isLeader) await writeJson($, `replies/${name}.json`, { v: 1, id: name, at: await $.clock.now(), ok: false, text: 'Заявка не разобрана' } satisfies BoardReply)
        continue
      }
      if (now - req.at > REQUEST_TTL_MS) {
        // claudibeat has already given up on it.
        remember(handled, name)
        continue
      }
      let verdict = verdictFor(req, { selfKey: myKey(), leaderKey, isLeader, rows })
      // Nobody took it: the leader may be a chat still running the old mod.
      // After a while any session with this code may, under a claim.
      if (verdict === 'skip' && now - req.at > ORPHAN_MS) verdict = 'target' in req && 'session' in req.target && req.target.session === myKey() ? 'fill' : 'run'
      if (verdict === 'skip') continue
      if (!(await claim($, req.id))) {
        remember(handled, name)
        continue
      }
      remember(handled, name)
      remember(handled, req.id)
      let done: Omit<BoardReply, 'v' | 'id' | 'at'>
      try {
        done = await runRequest($, req, verdict, rows)
      } catch (err) {
        done = { ok: false, text: planeShort(err) }
      }
      const reply: BoardReply = { v: 1, id: req.id, at: await $.clock.now(), ...done }
      await writeJson($, `replies/${safeKey(req.id)}.json`, reply)
      const a = assignmentOf(reply)
      if (a && isLeader) {
        remember(merged, req.id)
        await changeAssignments($, items => addAssignment(items, a))
      }
    }
  } catch {
    // a request the next beat sees again
  } finally {
    isServing = false
  }
}

async function runRequest($: $, req: BoardRequest, verdict: Verdict, rows: readonly Row[]): Promise<Omit<BoardReply, 'v' | 'id' | 'at'>> {
  if (req.kind === 'refresh') {
    const got = await refreshBoard($, true)
    return got ? { ok: true, text: 'обновлено' } : { ok: false, text: shortLine((await read($, board)).error ?? '') }
  }
  const p = await planeOf($)
  if (!p) return { ok: false, text: 'Нет ключа Plane' }
  if (req.kind === 'move') {
    const item = await p.move(req.task, req.to)
    if (req.comment) await p.comment(req.task, req.comment)
    void refreshBoard($, true)
    return { ok: true, text: moveText(item.key, req.to) }
  }
  return assign($, p, req, verdict, rows)
}

/** A task to a chat: «В работе», a comment, then the message, or a new chat with it. */
async function assign(
  $: $,
  p: NonNullable<Awaited<ReturnType<typeof planeOf>>>,
  req: Extract<BoardRequest, { kind: 'assign' }>,
  verdict: Verdict,
  rows: readonly Row[],
): Promise<Omit<BoardReply, 'v' | 'id' | 'at'>> {
  const target = req.target
  const item = await p.item(req.task)
  if (!item) return { ok: false, text: `${req.task} не найдена в Plane` }
  const sessionTitle =
    'newChat' in target ? chatTitle(item.key, item.title) : verdict === 'fill' ? title ?? projectOf(cwd) : sessionTitleOf(rows, target.session)
  await p.move(item.key, 'doing')
  await p.comment(item.key, assignComment(sessionTitle))
  const message = assignMessage(item)
  let session = ''
  let text = `${item.key} → «${clip(sessionTitle, 60)}»`

  if (verdict === 'fill') {
    try {
      await $.prompt.fill({ text: message })
    } catch {
      return { ok: false, text: `${item.key}: поле ввода занято, задача не вставлена` }
    }
    session = myKey()
    text = `${item.key} вставлена в поле ввода этого чата`
  } else if ('session' in target) {
    // A peer message first: a busy chat reads it at its next step. The app's
    // send_message is the fallback, and it waits until the chat's turn ends.
    let sent = false
    try {
      const got = await $.session.send({ to: { sessionId: target.session }, text: message })
      sent = got.isDelivered
    } catch {
      // no peer route to that chat: the app's tool below
    }
    if (!sent) {
      try {
        const got = await $.mcp.call('ccd_session_mgmt', 'send_message', { session_id: target.session, message })
        const reply = textOf(got)
        if (!isDelivered(reply, got.isError)) return { ok: false, text: `${item.key}: сообщение в «${clip(sessionTitle, 40)}» не доставлено` }
        if (/queued/i.test(reply)) text += ' (в очереди: придёт, когда чат закончит текущую работу)'
      } catch {
        return { ok: false, text: `${item.key}: сообщение в «${clip(sessionTitle, 40)}» не доставлено` }
      }
    }
    session = target.session
  } else {
    const named = target.newChat.cwd !== ''
    let reply = ''
    try {
      const got = await $.mcp.call('ccd_session', 'start_session', {
        initiation: 'user_asked',
        context: 'fresh',
        title: sessionTitle,
        prompt: message,
        background:
          'Владелец перетащил задачу Plane на «Новый агент» на доске claudibeat: это новый чат под одну задачу, она — в prompt. ' +
          'Работай как в любом новом чате владельца, никакой связи с сессией, откуда он запущен, нет.',
        ...(named ? { cwd: target.newChat.cwd, use_worktree: false } : {}),
      })
      reply = textOf(got)
      if (got.isError) return { ok: false, text: `${item.key}: новый чат не запустился` }
    } catch {
      return { ok: false, text: `${item.key}: новый чат не запустился` }
    }
    session = sessionIdIn(reply) ?? ''
    if (session) await openSession($, `claude://claude.ai/epitaxy/${session}`)
  }
  void refreshBoard($, true)
  return { ok: true, text, task: item.key, session, sessionTitle }
}

// ─── the model's tool ───────────────────────────────────────────────────────

const TOOL_SPEC = {
  name: TOOL,
  description:
    'Задачник владельца в Plane: проекты Plane — метки задач (WEB, SEO…). ' +
    'action=list — задачи колонками «Не взято / В работе / Готово сегодня» (project — фильтр по метке); ' +
    'show — карточка задачи с описанием (id = WEB-281); add — новая задача (title, project?, description?); ' +
    'take — взять задачу в работу в этой сессии (переводит «В работу», пишет комментарий, возвращает описание); ' +
    'move — перевести (to: todo | doing | done | cancelled); comment — комментарий к задаче (text). ' +
    'На «чекни задачи», «что в задачах», «возьми задачу» сначала зови list.',
  inputSchema: {
    type: 'object',
    properties: {
      action: { type: 'string', enum: ['list', 'show', 'add', 'take', 'move', 'comment'] },
      id: { type: 'string', description: 'Ключ задачи: WEB-281' },
      project: { type: 'string', description: 'Метка проекта Plane: WEB, SEO. Для add без метки — проект для входящих.' },
      title: { type: 'string', description: 'Название новой задачи' },
      description: { type: 'string', description: 'Описание новой задачи, по желанию' },
      to: { type: 'string', enum: ['todo', 'doing', 'done', 'cancelled'] },
      text: { type: 'string', description: 'Текст комментария' },
    },
    required: ['action'],
  },
}

async function runTool($: $, input: Record<string, unknown>): Promise<{ text: string } | { error: string }> {
  const p = await planeOf($)
  if (!p) return { error: 'Нет ключа Plane: владелец вписывает base, workspace и key в ~/.secrets/plane.json (или в настройки мода status-band).' }
  const str = (k: string) => (typeof input[k] === 'string' ? (input[k] as string).trim() : '')
  const id = str('id').toUpperCase()
  try {
    switch (str('action')) {
      case 'list': {
        const now = await $.clock.now()
        const got = await p.board({ now, tzOffsetMin: await zoneOffset($), ...(str('project') ? { projects: [str('project').toUpperCase()] } : {}) })
        void refreshBoard($)
        return { text: backlogText([...got.doing, ...got.todo, ...got.doneToday], str('project') ? `Задачи ${str('project').toUpperCase()}` : 'Задачи') }
      }
      case 'show': {
        if (!id) return { error: 'Нужен id: WEB-281' }
        const item = await p.item(id)
        return item ? { text: itemText(item) } : { error: `${id} не найдена` }
      }
      case 'add': {
        const name = str('title')
        if (!name) return { error: 'Нужно title' }
        const parsed = parseQuickAdd(str('project') ? `${name} #${str('project')}` : name, planeOptions.inbox)
        const item = await p.add({ title: parsed.title, project: parsed.project, ...(str('description') ? { description: str('description') } : {}) })
        void refreshBoard($)
        return { text: `Добавлена ${item.key}: ${item.title}\n${item.url}` }
      }
      case 'take': {
        if (!id) return { error: 'Нужен id: WEB-281' }
        const item = await p.move(id, 'doing')
        await p.comment(id, `Взята в сессию «${title ?? projectOf(cwd)}»`)
        void refreshBoard($, true)
        return { text: `Взята в работу.\n\n${itemText(item)}` }
      }
      case 'move': {
        const to = str('to') as Move
        if (!id || !['todo', 'doing', 'done', 'cancelled'].includes(to)) return { error: 'Нужны id и to: todo | doing | done | cancelled' }
        const item = await p.move(id, to)
        void refreshBoard($, true)
        return { text: `${item.key} → ${item.stateName}` }
      }
      case 'comment': {
        if (!id || !str('text')) return { error: 'Нужны id и text' }
        await p.comment(id, str('text'))
        return { text: `Комментарий к ${id} добавлен` }
      }
    }
    return { error: 'action: list | show | add | take | move | comment' }
  } catch (err) {
    return { error: planeMessage(err) }
  }
}

const GUIDE =
  'Задачник владельца живёт в Plane: проекты Plane — метки задач (WEB, SEO…). Инструмент mcp__status-band__backlog: ' +
  'list / show / add / take / move / comment. На «чекни задачи», «что в задачах», «возьми задачу» сначала вызывай list. ' +
  'Взял задачу в работу — take; закончил — move to=done и короткий comment, что сделано и где. Новую мысль на будущее владелец просит записать — add.'

// ─── the hooks ──────────────────────────────────────────────────────────────

export const register: Register = (on, options) => {
  planeOptions = {
    url: String(options.planeUrl ?? ''),
    workspace: String(options.planeWorkspace ?? ''),
    apiKey: String(options.planeApiKey ?? ''),
    inbox: String(options.inboxProject ?? 'INBOX').toUpperCase(),
  }
  plane = null
  planeKeyFailed = false

  on('session.start', async ($, e, next) => {
    const started = await next(e)
    await $.tool.register(TOOL_SPEC)
    // The REPL has a person from the start; the desktop runs over the SDK and
    // names itself and its session in the environment. A `claude -p` run does
    // neither: it waits for nobody, so it neither shows nor counts.
    if (e.isInteractive || e.surface !== null || (await hostId($)) !== null) await begin($)
    return started
  })

  on('session.attach', async ($, e, next) => {
    const attached = await next(e)
    await begin($)
    return attached
  })

  on('session.end', async ($, e, next) => {
    ticker?.cancel()
    ticker = null
    if (isLeader) await writeJson($, 'index.lock', { v: 1, owner: myKey(), at: 0 })
    return next(e)
  })

  on('prompt.compose', async ($, e, next) => {
    const composed = await next(e)
    if (e.traits.includes('bare') || composed.sections.some(s => s.id === 'status-band:backlog')) return composed
    return { sections: [...composed.sections, { id: 'status-band:backlog', text: GUIDE, scope: 'session' as const }] }
  })

  // The person typed: whatever this session waited for, they are here.
  on('prompt.submit', async ($, e, next) => {
    if (!e.isMeta) await setState($, 'working', { doing: 'думает' })
    return next(e)
  })

  on('turn.start', async ($, e, next) => {
    if (e.agentId === undefined) {
      pendingAsk = null
      await setState($, 'working', { doing: 'думает' })
    }
    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    const done = await next(e)
    if (e.agentId === undefined) {
      if (e.reason === 'aborted') await setState($, 'idle')
      else if (pendingAsk?.ask || pendingAsk?.isBlocked) await setState($, 'waiting', { waitingReason: 'decision', ask: pendingAsk.ask })
      else await setState($, 'done')
      pendingAsk = null
    }
    return done
  })

  on('classic.Notification', async ($, e, next) => {
    if (e.notification_type === 'permission_prompt') await setState($, 'waiting', { waitingReason: 'permission', ask: clip(e.message, 140) })
    return next(e)
  })

  on('tool.call', { tool: TOOL_NAME }, async ($, e) => {
    const done = await runTool($, e as unknown as Record<string, unknown>)
    if ('error' in done) return { deny: done.error }
    return { result: done.text }
  })

  on('tool.call', async ($, e, next) => {
    const input = e as unknown as Record<string, unknown>
    const agentId = e.agentId
    if (agentId !== undefined) {
      const what = toolLine(e.tool, input)
      await touchAgent($, agentId, a => ({ ...a, doing: what }))
      try {
        return await next(e)
      } finally {
        await touchAgent($, agentId, a => ({ ...a, doing: null }))
      }
    }
    if (e.tool === 'mcp__ccd_turn__wrap_up') {
      const ask = typeof input.ask === 'string' ? clip(input.ask, 140) : null
      pendingAsk = { ask, isBlocked: input.blocked === true }
      return next(e)
    }
    if (e.tool === 'AskUserQuestion') {
      await setState($, 'waiting', { waitingReason: 'question', ask: questionOf(input) })
      try {
        return await next(e)
      } finally {
        await setState($, 'working', { doing: 'думает' })
      }
    }
    if (e.tool !== TOOL_NAME) {
      own = { ...own, doing: toolLine(e.tool, input) }
      isDirty = true
    }
    return next(e)
  })

  on('agent.spawn', async ($, e, next) => {
    const spawned = await next(e)
    const id = spawned.agentId
    if (spawned.deny === undefined && id) {
      const now = await $.clock.now()
      const line: AgentLine = {
        id,
        name: clip(e.description.trim() || e.subagentType, 60),
        type: e.subagentType,
        model: spawned.model ? shortModel(spawned.model) : null,
        status: 'running',
        doing: null,
        startedAt: now,
        endedAt: null,
        lastAt: now,
      }
      agents = [...agents.filter(a => a.id !== id && (a.status === 'running' || now - (a.endedAt ?? now) < 30 * 60_000)), line]
      isDirty = true
    }
    return spawned
  })

  on('turn.step', async function* ($, e, next) {
    const id = e.agentId
    if (id === undefined) return yield* next(e)
    await touchAgent($, id, a => ({ ...a, doing: 'думает', model: a.model ?? (e.model ? shortModel(e.model) : null) }))
    try {
      return yield* next(e)
    } finally {
      await touchAgent($, id, a => ({ ...a, doing: null }))
    }
  })

  // The mod draws nothing: claudibeat shows all of it. These sites stay only
  // to wake the beat in a session whose start no hook saw.
  on('ui.render', { component: 'PromptHint' }, async ($, e, next) => {
    wake($)
    return next(e)
  })

  on('ui.render', { component: 'SessionMode' }, async ($, e, next) => {
    wake($)
    return next(e)
  })

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    wake($)
    return next(e)
  })
}

// ─── agents of this session ─────────────────────────────────────────────────

async function touchAgent($: $, id: string, fn: (a: AgentLine) => AgentLine) {
  const now = await $.clock.now()
  const i = agents.findIndex(a => a.id === id)
  if (i < 0) return
  agents = agents.map((a, j) => (j === i ? { ...fn(a), lastAt: now } : a))
  isDirty = true
  await settleAgents($, now)
}

let lastAgentList = 0
async function settleAgents($: $, now: number) {
  if (now - lastAgentList < 5_000 || !agents.some(a => a.status === 'running')) return
  lastAgentList = now
  try {
    const engine = await $.agent.list()
    agents = agents.map(a => {
      if (a.status !== 'running') return a
      const info = engine.find(x => x.id === a.id)
      if (!info) return a
      if (info.status === 'completed' || info.status === 'idle') return { ...a, status: 'done', doing: null, endedAt: now }
      if (info.status === 'failed' || info.status === 'killed') return { ...a, status: 'failed', doing: null, endedAt: now }
      return a
    })
  } catch {
    // the engine's word next time
  }
}
