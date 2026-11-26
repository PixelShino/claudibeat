import { expect, test } from 'claude-code/testing'

import type { AssignmentItem, BoardRequest, Row } from '../types'
import {
  addAssignment,
  assignComment,
  assignMessage,
  assignmentOf,
  chatTitle,
  closeByFile,
  helperOf,
  isDelivered,
  moveText,
  parseRequest,
  pendingNames,
  pruneAssignments,
  remember,
  sessionIdIn,
  sessionTitleOf,
  verdictFor,
} from '../hooks/board-requests'

const NOW = 1_794_930_000_000
const A = 'local_00000000-0000-0000-0000-00000000000a'
const B = 'local_00000000-0000-0000-0000-00000000000b'
const C = 'local_00000000-0000-0000-0000-00000000000c'

const row = (key: string, over: Partial<Row> = {}): Row => ({
  key,
  desktopId: key,
  title: `Сессия ${key.slice(-1)}`,
  project: 'my-app',
  cwd: 'D:\\Code\\Projects\\my-app',
  link: null,
  state: 'idle',
  waitingReason: null,
  ask: null,
  since: NOW,
  doing: null,
  contextPercent: null,
  agents: [],
  isSelf: false,
  isLive: true,
  needsYou: false,
  lastActivityAt: NOW,
  ...over,
})

const base = { v: 1, id: 'cb-1794930000000-a1f3', at: NOW, by: 'claudibeat' }

test('requests of the contract parse; anything else does not', () => {
  expect(parseRequest({ ...base, kind: 'assign', task: 'web-281', target: { session: A } })).toEqual({ ...base, kind: 'assign', task: 'WEB-281', target: { session: A } })
  expect(parseRequest({ ...base, kind: 'assign', task: 'WEB-281', target: { newChat: { cwd: 'D:\\Code\\Projects\\my-app', project: 'my-app' } } })).toEqual({
    ...base,
    kind: 'assign',
    task: 'WEB-281',
    target: { newChat: { cwd: 'D:\\Code\\Projects\\my-app', project: 'my-app' } },
  })
  expect(parseRequest({ ...base, kind: 'move', task: 'SEO-4', to: 'done' })).toEqual({ ...base, kind: 'move', task: 'SEO-4', to: 'done' })
  expect(parseRequest({ ...base, kind: 'refresh' })).toEqual({ ...base, kind: 'refresh' })

  expect(parseRequest(null)).toBeNull()
  expect(parseRequest({ ...base, v: 2, kind: 'refresh' })).toBeNull()
  expect(parseRequest({ ...base, id: '../evil', kind: 'refresh' })).toBeNull()
  expect(parseRequest({ ...base, at: 'вчера', kind: 'refresh' })).toBeNull()
  expect(parseRequest({ ...base, kind: 'move', task: 'SEO-4', to: 'archive' })).toBeNull()
  expect(parseRequest({ ...base, kind: 'move', task: 'не задача', to: 'done' })).toBeNull()
  expect(parseRequest({ ...base, kind: 'assign', task: 'WEB-281', target: { session: 'cli-123' } })).toBeNull()
  expect(parseRequest({ ...base, kind: 'assign', task: 'WEB-281', target: {} })).toBeNull()
  expect(parseRequest({ ...base, kind: 'delete', task: 'WEB-281' })).toBeNull()
})

const assignTo = (session: string): BoardRequest => ({ ...base, v: 1, kind: 'assign', task: 'WEB-281', target: { session } })
const refresh: BoardRequest = { ...base, v: 1, kind: 'refresh' }

test('the leader runs the requests, the others leave them', () => {
  const rows = [row(A), row(B), row(C)]
  expect(verdictFor(refresh, { selfKey: A, leaderKey: A, isLeader: true, rows })).toBe('run')
  expect(verdictFor(assignTo(B), { selfKey: A, leaderKey: A, isLeader: true, rows })).toBe('run')
  expect(verdictFor(refresh, { selfKey: B, leaderKey: A, isLeader: false, rows })).toBe('skip')
  expect(verdictFor(assignTo(C), { selfKey: B, leaderKey: A, isLeader: false, rows })).toBe('skip')
})

test('a task for the leader’s own chat: the live non-leader with the smallest key sends it', () => {
  // Leader C; B and A live: A has the smallest key.
  const rows = [row(C), row(B), row(A)]
  expect(verdictFor(assignTo(C), { selfKey: C, leaderKey: C, isLeader: true, rows })).toBe('skip')
  expect(verdictFor(assignTo(C), { selfKey: A, leaderKey: C, isLeader: false, rows })).toBe('run')
  expect(verdictFor(assignTo(C), { selfKey: B, leaderKey: C, isLeader: false, rows })).toBe('skip')
  // A has gone quiet: B takes it.
  const quiet = [row(C), row(B), row(A, { isLive: false })]
  expect(verdictFor(assignTo(C), { selfKey: B, leaderKey: C, isLeader: false, rows: quiet })).toBe('run')
  expect(helperOf(quiet, C)).toBe(B)
  // A terminal session cannot send to the app's chats.
  expect(helperOf([row(C), row('cli-1')], C)).toBeNull()
})

test('a task for the leader’s chat with nobody else live: the leader fills its own prompt', () => {
  const rows = [row(A), row(B, { isLive: false })]
  expect(verdictFor(assignTo(A), { selfKey: A, leaderKey: A, isLeader: true, rows })).toBe('fill')
  // Without a known leader a non-leader runs nothing.
  expect(verdictFor(assignTo(A), { selfKey: B, leaderKey: null, isLeader: false, rows })).toBe('skip')
})

test('the message to the agent follows the board spec word for word', () => {
  const text = assignMessage({ key: 'WEB-281', title: 'Чистка документации', description: 'Разобрать входящие\nи ответить', url: 'https://plane.example/io/browse/WEB-281/' })
  expect(text).toBe(
    'Задача из Plane, назначена тебе с доски claudibeat: WEB-281 — Чистка документации\n\n' +
      'Разобрать входящие\nи ответить\n\n' +
      'https://plane.example/io/browse/WEB-281/\n\n' +
      'Она уже в «В работе». Сделай её. По итогу: mcp__status-band__backlog action=move id=WEB-281 to=done\n' +
      'и action=comment с коротким итогом: что сделано и где. Застрял — comment с вопросом, статус не меняй.\n\n' +
      closeByFile('WEB-281'),
  )
  // No description: no empty paragraph.
  const bare = assignMessage({ key: 'SEO-4', title: 'Карта сайта', description: '  ', url: 'https://plane.example/io/browse/SEO-4/' })
  expect(bare.startsWith('Задача из Plane, назначена тебе с доски claudibeat: SEO-4 — Карта сайта\n\nhttps://')).toBe(true)
})

test('words around an assignment', () => {
  expect(assignComment('Чистка документации')).toBe('Назначена агенту «Чистка документации» с доски claudibeat')
  expect(chatTitle('WEB-281', 'Короткое')).toBe('WEB-281 Короткое')
  expect(chatTitle('WEB-281', 'Очень длинное название задачи, которое никак не влезает в заголовок чата').length).toBe(56)
  expect(moveText('WEB-281', 'done')).toBe('WEB-281: готово')
  expect(sessionTitleOf([row(A, { title: 'Shop backend' })], A)).toBe('Shop backend')
  expect(sessionTitleOf([], A)).toBe('сессия')
})

test('send_message: delivered or queued is success, anything else is not', () => {
  expect(isDelivered('Message delivered to session', false)).toBe(true)
  expect(isDelivered('{"status":"queued"}', undefined)).toBe(true)
  expect(isDelivered('Message delivered', true)).toBe(false)
  expect(isDelivered('Cannot send a message to the current session', false)).toBe(false)
  expect(sessionIdIn(`Started session ${B} in D:\\x`)).toBe(B)
  expect(sessionIdIn('no id')).toBeNull()
})

test('replies feed assignments.json; tasks gone from «В работе» leave it', () => {
  expect(assignmentOf({ v: 1, id: 'x', at: NOW, ok: true, text: 'ok', task: 'WEB-281', session: A, sessionTitle: 'A' })).toEqual({ task: 'WEB-281', session: A, sessionTitle: 'A', at: NOW })
  expect(assignmentOf({ v: 1, id: 'x', at: NOW, ok: false, text: 'нет', task: 'WEB-281', session: A, sessionTitle: 'A' })).toBeNull()
  expect(assignmentOf({ v: 1, id: 'x', at: NOW, ok: true, text: 'WEB-281: готово' })).toBeNull()

  const one: AssignmentItem = { task: 'WEB-281', session: A, sessionTitle: 'A', at: NOW - 10_000 }
  const again: AssignmentItem = { task: 'WEB-281', session: B, sessionTitle: 'B', at: NOW }
  const other: AssignmentItem = { task: 'SEO-4', session: A, sessionTitle: 'A', at: NOW - 60_000 }
  expect(addAssignment([one, other], again)).toEqual([other, again])

  // SEO-4 left «В работе»; WEB-281 was assigned after the fetch began, so it stays.
  expect(pruneAssignments([other, again], new Set(), NOW - 1_000)).toEqual([again])
  expect(pruneAssignments([other, again], new Set(['SEO-4']), NOW + 1)).toEqual([other])
})

test('requests already answered or handled are not read again; memory is bounded', () => {
  expect(pendingNames(['a', 'b', 'c'], new Set(['b']), new Set(['c']))).toEqual(['a'])
  const handled = new Set<string>()
  for (let i = 0; i < 10; i++) remember(handled, `id-${i}`, 4)
  expect([...handled]).toEqual(['id-6', 'id-7', 'id-8', 'id-9'])
})

test('an agent with a broken backlog closes its task with a request file', () => {
  const how = closeByFile('WEB-7')
  expect(how.includes("task='WEB-7'")).toBe(true)
  expect(how.includes("to='done'")).toBe(true)
  expect(how.includes('WriteAllText')).toBe(true)
  const req = parseRequest({ v: 1, id: 'ag-1794930000000', at: 1, by: 'agent', kind: 'move', task: 'web-7', to: 'done', comment: '  Сделано: ветка feat/x  ' })
  expect(req).toEqual({ v: 1, id: 'ag-1794930000000', at: 1, by: 'agent', kind: 'move', task: 'WEB-7', to: 'done', comment: 'Сделано: ветка feat/x' })
  const plain = parseRequest({ v: 1, id: 'ag-2', at: 1, kind: 'move', task: 'WEB-7', to: 'done' })
  expect(plain && 'comment' in plain).toBe(false)
})
