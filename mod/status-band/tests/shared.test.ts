import { expect, test } from 'claude-code/testing'

import type { AckFile, IndexEntry, SessionFile } from '../types'
import {
  STALE_MS,
  attention,
  bar,
  indexFromList,
  limitsOf,
  mergeRows,
  plural,
  projectOf,
  questionOf,
  resetText,
  shortModel,
  tone,
  toolLine,
} from '../hooks/shared'

const NOW = 1_762_176_000_000

const entry = (id: string, over: Partial<IndexEntry> = {}): IndexEntry => ({
  desktopId: id,
  title: `Сессия ${id}`,
  cwd: 'D:\\Code\\Projects\\my-app',
  isRunning: false,
  lastActivityAt: NOW - 60_000,
  link: `claude://claude.ai/epitaxy/${id}`,
  pinned: false,
  ...over,
})

const file = (id: string, over: Partial<SessionFile> = {}): SessionFile => ({
  v: 1,
  key: id,
  desktopId: id,
  cliId: `cli-${id}`,
  title: `Сессия ${id}`,
  cwd: 'D:\\Code\\Projects\\my-site',
  project: 'my-site',
  link: `claude://claude.ai/epitaxy/${id}`,
  state: 'working',
  waitingReason: null,
  ask: null,
  stateSince: NOW - 30_000,
  doing: 'читает app.ts',
  context: { percent: 40, tokens: 400_000 },
  agents: [],
  heartbeatAt: NOW - 5_000,
  ...over,
})

const ackAt = (at: number): AckFile => ({ v: 1, ackAt: at, by: 'band' })

test('a session waiting for the person needs them until they ack it', () => {
  const files = [file('local_a', { state: 'waiting', waitingReason: 'question', ask: 'Какой вариант?' })]
  const before = mergeRows([entry('local_a', { isRunning: true })], files, {}, 'local_me', NOW)
  expect(before[0]?.state).toBe('waiting')
  expect(before[0]?.needsYou).toBe(true)
  expect(before[0]?.ask).toBe('Какой вариант?')
  expect(attention(before)).toEqual({ waiting: 1, done: 0 })

  const after = mergeRows([entry('local_a')], files, { local_a: ackAt(NOW - 1_000) }, 'local_me', NOW)
  expect(after[0]?.needsYou).toBe(false)

  // An ack older than the state does not count.
  const stale = mergeRows([entry('local_a')], files, { local_a: ackAt(NOW - 60_000) }, 'local_me', NOW)
  expect(stale[0]?.needsYou).toBe(true)
})

test('this session never needs the person: they are looking at it', () => {
  const rows = mergeRows([], [file('local_me', { state: 'done' })], {}, 'local_me', NOW)
  expect(rows[0]?.isSelf).toBe(true)
  expect(rows[0]?.needsYou).toBe(false)
})

test('a quiet file does not lie: running comes from the index, done stays done', () => {
  const quiet = NOW - STALE_MS - 1
  const running = mergeRows([entry('local_a', { isRunning: true })], [file('local_a', { state: 'idle', heartbeatAt: quiet })], {}, null, NOW)
  expect(running[0]?.state).toBe('working')
  expect(running[0]?.isLive).toBe(false)
  expect(running[0]?.doing).toBe(null)

  const closed = mergeRows([entry('local_b')], [file('local_b', { state: 'done', heartbeatAt: quiet })], {}, null, NOW)
  expect(closed[0]?.state).toBe('done')
  expect(closed[0]?.needsYou).toBe(true)
})

test('a session without the mod comes from the index alone', () => {
  const rows = mergeRows([entry('local_x', { isRunning: true }), entry('local_y')], [], {}, null, NOW)
  expect(rows.map(r => r.state)).toEqual(['working', 'idle'])
  expect(rows[0]?.project).toBe('my-app')
})

test('the order: waiting, done, working, idle', () => {
  const rows = mergeRows(
    [entry('i1', { lastActivityAt: NOW - 1_000 })],
    [file('w1'), file('d1', { state: 'done' }), file('q1', { state: 'waiting' })],
    {},
    null,
    NOW,
  )
  expect(rows.map(r => r.key)).toEqual(['q1', 'd1', 'w1', 'i1'])
})

test('a quiet file the app does not know waits for nobody', () => {
  const quiet = NOW - STALE_MS - 1
  const rows = mergeRows([], [file('cli-x', { key: 'cli-x', desktopId: null, state: 'done', heartbeatAt: quiet })], {}, null, NOW)
  expect(rows[0]?.state).toBe('idle')
  expect(rows[0]?.needsYou).toBe(false)
})

test('the old file under the engine id goes away once quiet and idle', () => {
  const tomb = file('cli-x', { desktopId: null, state: 'idle', heartbeatAt: 0 })
  expect(mergeRows([], [tomb], {}, null, NOW)).toEqual([])
})

test('idle sessions older than a day leave the list', () => {
  const rows = mergeRows([entry('old', { lastActivityAt: NOW - 25 * 3600_000 })], [], {}, null, NOW)
  expect(rows).toEqual([])
})

test('limits, bars and tones', () => {
  const limits = limitsOf([
    { kind: 'five_hour', percentUsed: 41.4, resetsAt: new Date(NOW + 130 * 60_000).toISOString() },
    { kind: 'seven_day', percentUsed: 18 },
  ])
  expect(limits).toEqual([
    { window: 'five_hour', percent: 41, resetsAt: NOW + 130 * 60_000 },
    { window: 'seven_day', percent: 18, resetsAt: null },
  ])
  expect(resetText(NOW + 130 * 60_000, NOW)).toBe('↻2:10')
  expect(resetText(NOW + 3 * 24 * 3600_000, NOW)).toBe('↻3д')
  expect(bar(41)).toBe('▰▰▱▱')
  expect(bar(1)).toBe('▰▱▱▱')
  expect(bar(0)).toBe('▱▱▱▱')
  expect(tone(59)).toBe('calm')
  expect(tone(60)).toBe('warn')
  expect(tone(85)).toBe('alarm')
})

test('words', () => {
  expect(plural(1, 'сессия', 'сессии', 'сессий')).toBe('сессия')
  expect(plural(3, 'сессия', 'сессии', 'сессий')).toBe('сессии')
  expect(plural(11, 'сессия', 'сессии', 'сессий')).toBe('сессий')
  expect(plural(22, 'сессия', 'сессии', 'сессий')).toBe('сессии')
  expect(projectOf('C:\\Users\\x\\AppData\\Local\\Claude-Data\\x\\scratch-workspaces\\a\\b')).toBe('без папки')
  expect(projectOf('D:/Code/Projects/my-shop')).toBe('my-shop')
  expect(shortModel('claude-opus-5-5')).toBe('opus-5.5')
  expect(shortModel('claude-haiku-4-5-20251001')).toBe('haiku-4.5')
  expect(toolLine('Bash', { command: 'npm test', description: 'Run the tests' })).toBe('запускает: Run the tests')
  expect(toolLine('Read', { file_path: 'D:\\Code\\a\\app.ts' })).toBe('читает app.ts')
  expect(toolLine('mcp__ccd_session_mgmt__list_sessions', {})).toBe('работает с приложением')
  expect(questionOf({ questions: [{ question: 'Что ставим?' }, { question: 'И ещё?' }] })).toBe('Что ставим? (+1)')
})

test('the index from list_sessions', () => {
  const text = JSON.stringify([
    { sessionId: 'local_1', title: 'A', cwd: 'D:\\x', isRunning: true, lastActivityAt: '2026-11-03T13:17:45.804Z', link: 'claude://1', isArchived: false },
    { sessionId: 'local_2', title: 'B', cwd: 'D:\\y', isRunning: false, lastActivityAt: '2026-11-03T13:00:00.000Z', isArchived: true },
    { sessionId: 'local_3', title: 'C', cwd: 'D:\\x', startedBy: 'local_1', group: null },
    { sessionId: 'local_4', title: 'D', cwd: 'D:\\x', startedBy: 'local_1', group: { id: 'g', name: 'SPA' } },
  ])
  const index = indexFromList(text)
  expect(index?.length).toBe(3)
  expect(index?.[1]?.parent).toBe('local_1')
  expect(index?.[2]?.parent).toBe(null)
  expect(index?.[0]).toEqual({
    desktopId: 'local_1',
    title: 'A',
    cwd: 'D:\\x',
    isRunning: true,
    lastActivityAt: Date.parse('2026-11-03T13:17:45.804Z'),
    link: 'claude://1',
    pinned: false,
    parent: null,
  })
  expect(indexFromList('not json')).toBe(null)
})
