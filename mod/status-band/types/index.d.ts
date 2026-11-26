// The shared folder's files (docs/DESIGN.md, «Контракт v1») and the values
// the band draws from. One writer per file; readers survive a torn write.

export type SessionState = 'working' | 'waiting' | 'done' | 'idle'
export type WaitingReason = 'question' | 'permission' | 'decision'

export type AgentLine = {
  id: string
  /** The Agent call's description, or its type. */
  name: string
  type: string
  /** `opus-5.5`, `haiku-4.5`; null until known. */
  model: string | null
  status: 'running' | 'done' | 'failed'
  /** The tool in hand or «думает»; null between steps. */
  doing: string | null
  startedAt: number
  endedAt: number | null
  /** Its last step or tool call: three minutes of nothing while running is «застрял». */
  lastAt: number
}

/** `sessions/<key>.json`: written by the session's own mod only. */
export type SessionFile = {
  v: 1
  key: string
  desktopId: string | null
  cliId: string
  title: string | null
  cwd: string
  project: string
  link: string | null
  state: SessionState
  waitingReason: WaitingReason | null
  ask: string | null
  stateSince: number
  doing: string | null
  context: { percent: number | null; tokens: number | null } | null
  agents: AgentLine[]
  heartbeatAt: number
}

export type IndexEntry = {
  desktopId: string
  title: string
  cwd: string
  isRunning: boolean
  lastActivityAt: number
  link: string | null
  pinned: boolean
  /** The session that started this one (`startedBy`), when it is a helper the
   *  person never filed into a sidebar group or pinned; otherwise null. */
  parent: string | null
}

/** `index.json`: every session of the app, from `list_sessions`; written by the leader. */
export type IndexFile = { v: 1; updatedAt: number; sessions: IndexEntry[] }

export type Limit = { window: string; percent: number; resetsAt: number | null }

/** `account.json`: the plan's windows; any mod writes, the last one wins. */
export type AccountFile = { v: 1; updatedAt: number; limits: Limit[] }

/** `acks/<key>.json`: the person has seen the session's state since `ackAt`. */
export type AckFile = { v: 1; ackAt: number; by: string }

export type TaskGroup = 'backlog' | 'unstarted' | 'started' | 'completed' | 'cancelled'

/** `tasks.json`: what the tray needs of Plane; written by the leader. */
export type TasksFile = {
  v: 1
  updatedAt: number
  ok: boolean
  error: string | null
  counts: { todo: number; doing: number; doneToday: number }
  top: Array<{ id: string; title: string; group: TaskGroup; project: string; url: string }>
}

/** One session as the band and the tray draw it: the index and the file merged. */
export type Row = {
  key: string
  desktopId: string | null
  title: string
  project: string
  /** The session's folder: a new chat in the same project starts there. */
  cwd: string
  link: string | null
  state: SessionState
  waitingReason: WaitingReason | null
  ask: string | null
  /** When the state began (a file's `stateSince`, else the last activity). */
  since: number
  doing: string | null
  contextPercent: number | null
  agents: AgentLine[]
  /** This session: the band draws no «открыть» for it. */
  isSelf: boolean
  /** A fresh heartbeat stands behind the state (else the index's word). */
  isLive: boolean
  /** Waiting or done, and not seen since. */
  needsYou: boolean
  lastActivityAt: number
}

export type Shared = {
  rows: Row[]
  limits: Limit[]
  tasks: TasksFile | null
  readAt: number
}

/** A card of board.json: what claudibeat draws of a Plane task. */
export type BoardCard = {
  key: string
  title: string
  project: string
  group: TaskGroup
  stateName: string
  priority: string
  description: string
  url: string
}

/** The last board this session read or fetched: kept so a failed fetch keeps the cards. */
export type BoardView = {
  todo: BoardCard[]
  doing: BoardCard[]
  doneToday: BoardCard[]
  projects: string[]
  fetchedAt: number
  /** A short Russian line when the last fetch failed; the cards are then the last good ones. */
  error: string | null
}

// ─── claudibeat's board: requests, replies, assignments ─────────────────────

export type TaskMove = 'todo' | 'doing' | 'done' | 'cancelled'

export type RequestBase = { v: 1; id: string; at: number; by: string }

/** `requests/<id>.json`: written and removed by claudibeat; the mod reads and runs it. */
export type BoardRequest =
  | (RequestBase & { kind: 'assign'; task: string; target: { session: string } | { newChat: { cwd: string; project: string } } })
  | (RequestBase & { kind: 'move'; task: string; to: TaskMove; comment?: string })
  | (RequestBase & { kind: 'refresh' })

/**
 * `replies/<id>.json`: written by the mod that ran the request, removed by
 * claudibeat. An assign that went through also names the task and the chat,
 * so the leader can add it to assignments.json.
 */
export type BoardReply = {
  v: 1
  id: string
  at: number
  ok: boolean
  /** A short Russian line for the owner. */
  text: string
  task?: string
  /** `local_…`; empty when a new chat's id was not returned. */
  session?: string
  sessionTitle?: string
}

export type AssignmentItem = { task: string; session: string; sessionTitle: string; at: number }

/** `assignments.json`: who works on what; the leader alone writes it. */
export type AssignmentsFile = { v: 1; updatedAt: number; items: AssignmentItem[] }

declare module 'claude-code' {
  interface PluginState {
    'status-band': {
      shared: Shared
      board: BoardView
    }
  }
}
