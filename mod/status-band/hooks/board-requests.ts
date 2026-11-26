// Pure logic of claudibeat's board requests (docs/DESIGN.md, «Заявки доски»):
// reading a request, who runs it, the message an agent gets, the replies and
// assignments.json. No `$` here: register.tsx reads, calls and writes.

import type { AssignmentItem, BoardReply, BoardRequest, Row, TaskMove } from '../types'
import { clip } from './shared'

/** claudibeat drops a request without a reply after two minutes: older ones are not run. */
export const REQUEST_TTL_MS = 120_000
/** How many handled ids the mod keeps in memory. */
export const HANDLED_KEEP = 500

const MOVES: readonly TaskMove[] = ['todo', 'doing', 'done', 'cancelled']
/** The id names the reply's file: only what is safe in a file name. */
const ID_RE = /^[A-Za-z0-9_.-]{1,80}$/
const TASK_RE = /^[A-Z][A-Z0-9]*-\d+$/
const SESSION_RE = /^local_[0-9a-f-]{36}$/i

const str = (v: unknown): string => (typeof v === 'string' ? v.trim() : '')

/** A request file's JSON as the contract allows it; null for anything else. */
export function parseRequest(value: unknown): BoardRequest | null {
  if (!value || typeof value !== 'object') return null
  const r = value as Record<string, unknown>
  if (r.v !== 1 || !ID_RE.test(str(r.id)) || typeof r.at !== 'number' || !Number.isFinite(r.at)) return null
  const base = { v: 1 as const, id: str(r.id), at: r.at, by: str(r.by) || 'claudibeat' }
  const task = str(r.task).toUpperCase()
  switch (r.kind) {
    case 'refresh':
      return { ...base, kind: 'refresh' }
    case 'move': {
      const to = str(r.to) as TaskMove
      if (!TASK_RE.test(task) || !MOVES.includes(to)) return null
      // An agent closing its task says what it did in the same request.
      const comment = clip(str(r.comment), 4000)
      return { ...base, kind: 'move', task, to, ...(comment ? { comment } : {}) }
    }
    case 'assign': {
      if (!TASK_RE.test(task) || !r.target || typeof r.target !== 'object') return null
      const t = r.target as Record<string, unknown>
      const session = str(t.session)
      if (session) return SESSION_RE.test(session) ? { ...base, kind: 'assign', task, target: { session } } : null
      if (t.newChat && typeof t.newChat === 'object') {
        const n = t.newChat as Record<string, unknown>
        return { ...base, kind: 'assign', task, target: { newChat: { cwd: str(n.cwd), project: str(n.project) } } }
      }
      return null
    }
  }
  return null
}

export type Verdict = 'run' | 'fill' | 'skip'

/**
 * Who runs a request. The leader runs them all, but one: a task assigned to
 * the leader's own chat, which `send_message` refuses to send to itself. That
 * one goes to the live desktop session with the mod that is not the leader and
 * has the smallest key, so two of them never both run it; with none such, the
 * leader fills its own prompt.
 */
export function verdictFor(
  req: BoardRequest,
  ctx: { selfKey: string; leaderKey: string | null; isLeader: boolean; rows: readonly Row[] },
): Verdict {
  const leader = ctx.isLeader ? ctx.selfKey : ctx.leaderKey
  if (req.kind === 'assign' && 'session' in req.target && leader !== null && req.target.session === leader) {
    const runner = helperOf(ctx.rows, leader)
    if (runner === null) return ctx.isLeader ? 'fill' : 'skip'
    return runner === ctx.selfKey && !ctx.isLeader ? 'run' : 'skip'
  }
  return ctx.isLeader ? 'run' : 'skip'
}

/** The live desktop session with the mod, not the leader, with the smallest key. */
export function helperOf(rows: readonly Row[], leaderKey: string): string | null {
  const keys = rows.filter(r => r.isLive && SESSION_RE.test(r.key) && r.key !== leaderKey).map(r => r.key)
  keys.sort()
  return keys[0] ?? null
}

/** The session's name as the sidebar shows it. */
export function sessionTitleOf(rows: readonly Row[], session: string): string {
  const r = rows.find(x => x.key === session || x.desktopId === session)
  return r?.title || 'сессия'
}

/** A new chat's title: `WEB-281 Чистка документации`, at most 56 signs. */
export const chatTitle = (key: string, name: string) => clip(`${key} ${name}`, 56)

/** The message an agent gets: the template of the board's spec, word for word. */
export function assignMessage(item: { key: string; title: string; description: string; url: string }): string {
  const description = item.description.trim()
  return [
    `Задача из Plane, назначена тебе с доски claudibeat: ${item.key} — ${item.title}`,
    ...(description ? [description] : []),
    item.url,
    `Она уже в «В работе». Сделай её. По итогу: mcp__status-band__backlog action=move id=${item.key} to=done\n` +
      'и action=comment с коротким итогом: что сделано и где. Застрял — comment с вопросом, статус не меняй.',
    closeByFile(item.key),
  ].join('\n\n')
}

/**
 * The way to close a task when the chat's own backlog tool is broken (a chat
 * that loaded an old mod): a request file the leading mod runs within seconds.
 */
export function closeByFile(key: string): string {
  const ps =
    `$ms=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds(); $id='ag-'+$ms; ` +
    `$j=@{v=1;id=$id;at=$ms;by='agent';kind='move';task='${key}';to='done';comment='ИТОГ'} | ConvertTo-Json -Compress; ` +
    `[IO.File]::WriteAllText("$env:USERPROFILE\\.claude\\status-band\\requests\\$id.json", $j)`
  return (
    'Если backlog отвечает «Plane недоступен» (в чате старый мод), закрой задачу файлом-заявкой: её за пару секунд выполнит главная сессия. ' +
    'В PowerShell, вместо ИТОГ — что сделано и где:\n' +
    ps +
    '\nОтвет появится в ~/.claude/status-band/replies/<id>.json.'
  )
}

/** `send_message` delivered or queued the message: anything else is a failure. */
export const isDelivered = (text: string, isError: boolean | undefined) => !isError && /\b(delivered|queued)\b/i.test(text)

/** The `local_…` id a `start_session` reply names, if any. */
export const sessionIdIn = (text: string) => /local_[0-9a-f-]{36}/i.exec(text)?.[0] ?? null

/** The comment a task gets when the board assigns it. */
export const assignComment = (sessionTitle: string) => `Назначена агенту «${sessionTitle}» с доски claudibeat`

/** The line the owner reads for a move. */
export function moveText(key: string, to: TaskMove): string {
  const word = to === 'doing' ? 'в работе' : to === 'done' ? 'готово' : to === 'todo' ? 'в бэклоге' : 'отменена'
  return `${key}: ${word}`
}

/** A reply that carries an assignment: the leader adds it to assignments.json. */
export function assignmentOf(reply: unknown): AssignmentItem | null {
  if (!reply || typeof reply !== 'object') return null
  const r = reply as Partial<BoardReply>
  if (r.v !== 1 || r.ok !== true || typeof r.task !== 'string' || typeof r.session !== 'string' || typeof r.sessionTitle !== 'string') return null
  return { task: r.task, session: r.session, sessionTitle: r.sessionTitle, at: typeof r.at === 'number' ? r.at : 0 }
}

/** One assignment per task: a newer one replaces the old. */
export function addAssignment(items: readonly AssignmentItem[], item: AssignmentItem): AssignmentItem[] {
  const had = items.find(x => x.task === item.task)
  if (had && had.session === item.session && had.sessionTitle === item.sessionTitle && had.at >= item.at) return [...items]
  return [...items.filter(x => x.task !== item.task), item]
}

/**
 * Tasks that left «В работе» leave the list. One assigned after the board's
 * fetch began stays: that fetch could not see it moved yet.
 */
export function pruneAssignments(items: readonly AssignmentItem[], doing: ReadonlySet<string>, fetchStartedAt: number): AssignmentItem[] {
  return items.filter(x => doing.has(x.task) || x.at >= fetchStartedAt)
}

/** The request's file names worth reading: not answered, not handled, not ours to wait for. */
export function pendingNames(requests: readonly string[], replies: ReadonlySet<string>, handled: ReadonlySet<string>): string[] {
  return requests.filter(n => !replies.has(n) && !handled.has(n))
}

/** The handled ids, the oldest dropped past `keep`. */
export function remember(handled: Set<string>, id: string, keep = HANDLED_KEEP): void {
  handled.add(id)
  while (handled.size > keep) {
    const first = handled.values().next().value
    if (first === undefined) break
    handled.delete(first)
  }
}
