// Тесты клиента Plane: поддельный fetch отвечает так, как отвечает REST API v1
// Plane (обёртка страниц с next_cursor / next_page_results, задача с
// expand=state, 404 «Page not found.» на неизвестный маршрут). Данные и ключ
// выдуманы.
import { describe, expect, test } from 'claude-code/testing'
import {
  PlaneError,
  backlogText,
  createPlane,
  dayBounds,
  htmlToText,
  itemText,
  layoutBoard,
  normalizeKey,
  parseQuickAdd,
  parseTime,
  pickState,
  tasksFile,
} from '../hooks/plane'
import type { Board, Fetch, PlaneState, WorkItem } from '../hooks/plane'

const KEY = 'test-key-not-real-0001'
const BASE = 'https://plane.example.test'
const API = `${BASE}/api/v1/workspaces/web`

// ---------- фикстуры в форме ответов Plane ----------

type Raw = Record<string, unknown>

function page(results: Raw[], next?: string): Raw {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: next ?? `100:1:0`,
    prev_cursor: '100:-1:1',
    next_page_results: next !== undefined,
    prev_page_results: false,
    count: results.length,
    total_pages: next ? 2 : 1,
    total_results: results.length,
    extra_stats: null,
    results,
  }
}

const project = (id: string, identifier: string, extra: Raw = {}): Raw => ({
  id,
  identifier,
  name: `Проект ${identifier}`,
  network: 2,
  archived_at: null,
  is_member: true,
  workspace: 'ws-1',
  ...extra,
})

const state = (id: string, name: string, group: string, sequence: number): Raw => ({
  id,
  name,
  color: '#60646C',
  group,
  sequence,
  default: false,
  is_triage: group === 'triage',
  project: 'p-web',
})

// Порядок намеренно перемешан: модуль сам сортирует по sequence.
const WEB_STATES = [
  state('st-review', 'Ревью', 'started', 40000),
  state('st-ready', 'К работе', 'unstarted', 30000),
  state('st-progress', 'В работе', 'started', 35000),
  state('st-backlog', 'Бэклог', 'backlog', 15000),
  state('st-new', 'Новое', 'unstarted', 25000),
  state('st-done', 'Готово', 'completed', 45000),
  state('st-cancel', 'Отменено', 'cancelled', 55000),
  state('st-triage', 'Входящие', 'triage', 65000),
]
// В SEO нет групп unstarted и cancelled.
const SEO_STATES = [
  state('se-backlog', 'Бэклог', 'backlog', 15000),
  state('se-progress', 'Делаем', 'started', 35000),
  state('se-done', 'Сделано', 'completed', 45000),
]
const ALL_STATES = [...WEB_STATES, ...SEO_STATES]

function stateObj(id: string): Raw {
  const s = ALL_STATES.find((x) => x.id === id)
  if (!s) throw new Error(`нет статуса ${id}`)
  return { id: s.id, name: s.name, color: s.color, group: s.group }
}

function issue(projectId: string, seq: number, stateId: string, extra: Raw = {}): Raw {
  return {
    id: `wi-${projectId}-${seq}`,
    type_id: null,
    created_at: '2026-11-01T08:00:00.000000Z',
    updated_at: '2026-11-03T08:00:00.000000Z',
    deleted_at: null,
    point: null,
    name: `Задача ${seq}`,
    description_html: '<p></p>',
    priority: 'none',
    start_date: null,
    target_date: null,
    sequence_id: seq,
    sort_order: 65535,
    completed_at: null,
    archived_at: null,
    external_source: null,
    external_id: null,
    project: projectId,
    workspace: 'ws-1',
    parent: null,
    state: stateObj(stateId),
    assignees: [],
    labels: [],
    ...extra,
  }
}

// «Сейчас» = 3 ноября 2026, 21:30 UTC (в Москве уже 4 ноября, 00:30).
const NOW = Date.UTC(2026, 10, 3, 21, 30)

const WEB_PAGE_1 = [
  issue('p-web', 1, 'st-backlog', { updated_at: '2026-11-03T10:00:00Z' }),
  issue('p-web', 2, 'st-new', { priority: 'high', updated_at: '2026-11-03T09:00:00Z' }),
  issue('p-web', 3, 'st-ready', { priority: 'high', updated_at: '2026-11-03T11:00:00.5Z' }),
  issue('p-web', 4, 'st-review', { priority: 'urgent', name: 'Реестр сайтов' }),
  issue('p-web', 5, 'st-progress', { priority: 'low' }),
]
const WEB_PAGE_2 = [
  // вчера по Москве (23:00 3 ноября), сегодня по UTC
  issue('p-web', 6, 'st-done', { completed_at: '2026-11-03T20:00:00.123456Z', updated_at: '2026-11-03T20:00:00Z' }),
  // сегодня по Москве (00:10 4 ноября)
  issue('p-web', 7, 'st-done', {
    completed_at: '2026-11-04T00:10:00+03:00',
    updated_at: '2026-11-03T21:10:00Z',
    description_html: '<p>Шаг&nbsp;1 &amp; шаг 2</p><ul><li><p>раз</p></li><li>два</li></ul>',
  }),
  issue('p-web', 8, 'st-cancel'),
  // completed без completed_at — берётся updated_at
  issue('p-web', 9, 'st-done', { updated_at: '2026-11-03T21:20:00Z' }),
]
const SEO_ITEMS = [issue('p-seo', 1, 'se-progress', { priority: 'medium' })]

// ---------- поддельный сервер ----------

type Call = { method: string; url: string; path: string; headers: Record<string, string>; body?: string }
type Reply = { status: number; body?: unknown; raw?: string }
type Route = [method: string, path: RegExp, answer: (call: Call, m: RegExpExecArray) => Reply | Error]

function fakePlane(routes: Route[]) {
  const calls: Call[] = []
  const fetch: Fetch = async (url, init) => {
    const call: Call = {
      method: init?.method ?? 'GET',
      url,
      path: url.startsWith(API) ? url.slice(API.length) : url,
      headers: init?.headers ?? {},
      ...(init?.body !== undefined ? { body: init.body } : {}),
    }
    calls.push(call)
    for (const [method, re, answer] of routes) {
      if (method !== call.method) continue
      const m = re.exec(call.path)
      if (!m) continue
      const r = answer(call, m)
      if (r instanceof Error) throw r
      const text = r.raw ?? (r.body === undefined ? '' : JSON.stringify(r.body))
      return { status: r.status, ok: r.status >= 200 && r.status < 300, text }
    }
    // так отвечает Plane на неизвестный маршрут (handler404)
    return { status: 404, ok: false, text: JSON.stringify({ error: 'Page not found.' }) }
  }
  return { fetch, calls }
}

const ok = (body: unknown): Reply => ({ status: 200, body })

/** Сервер с двумя проектами; seg — какой префикс задач он знает */
function standardRoutes(seg: 'work-items' | 'issues' = 'work-items'): Route[] {
  return [
    ['GET', /^\/projects\/\?/, () => ok(page([project('p-web', 'WEB'), project('p-seo', 'SEO'), project('p-old', 'OLD', { archived_at: '2026-01-01T00:00:00Z' })]))],
    ['GET', /^\/projects\/p-web\/states\/\?/, () => ok(page(WEB_STATES))],
    ['GET', /^\/projects\/p-seo\/states\/\?/, () => ok(page(SEO_STATES))],
    [
      'GET',
      new RegExp(`^/projects/p-web/${seg}/\\?`),
      (c) => ok(c.path.includes('cursor=100%3A1%3A0') ? page(WEB_PAGE_2) : page(WEB_PAGE_1, '100:1:0')),
    ],
    ['GET', new RegExp(`^/projects/p-seo/${seg}/\\?`), () => ok(page(SEO_ITEMS))],
    [
      'GET',
      new RegExp(`^/${seg}/([A-Z]+)-(\\d+)/`),
      (_c, m) => {
        const all = [...WEB_PAGE_1, ...WEB_PAGE_2, ...SEO_ITEMS]
        const pid = m[1] === 'WEB' ? 'p-web' : m[1] === 'SEO' ? 'p-seo' : ''
        const hit = all.find((i) => i.project === pid && i.sequence_id === Number(m[2]))
        return hit ? ok(hit) : { status: 404, body: { error: 'The required object does not exist.' } }
      },
    ],
  ]
}

const cfg = { url: BASE, workspace: 'web', apiKey: KEY }

async function caught(p: Promise<unknown>): Promise<PlaneError> {
  try {
    await p
  } catch (e) {
    if (e instanceof PlaneError) return e
    throw e
  }
  throw new Error('ожидалась PlaneError, а вызов прошёл')
}

const keys = (list: WorkItem[]) => list.map((i) => i.key)

// ---------- разбор ----------

describe('разбор', () => {
  test('время Plane с микросекундами и смещением', () => {
    expect(parseTime('2026-11-03T20:00:00.123456Z')).toBe(Date.UTC(2026, 10, 3, 20, 0, 0, 123))
    expect(parseTime('2026-11-04T00:10:00+03:00')).toBe(Date.UTC(2026, 10, 3, 21, 10))
    expect(parseTime('2026-11-03')).toBe(Date.UTC(2026, 10, 3))
    expect(parseTime(null)).toBeNull()
    expect(parseTime('вчера')).toBeNull()
  })

  test('HTML описания → простой текст', () => {
    expect(htmlToText('<p>Шаг&nbsp;1 &amp; шаг 2</p><ul><li><p>раз</p></li><li>два</li></ul>')).toBe(
      'Шаг 1 & шаг 2\n\n• раз\n• два',
    )
    expect(htmlToText('<p>строка<br/>вторая</p><p>&#171;кавычки&#x00BB;</p>')).toBe('строка\nвторая\n\n«кавычки»')
    expect(htmlToText('<p></p>')).toBe('')
    expect(
      htmlToText(
        '<ul data-type="taskList"><li data-checked="true" data-type="taskItem"><label><input type="checkbox" checked="checked"><span></span></label><div><p>сделано</p></div></li>' +
          '<li data-checked="false" data-type="taskItem"><label><input type="checkbox"><span></span></label><div><p>осталось</p></div></li></ul>',
      ),
    ).toBe('☑ сделано\n☐ осталось')
  })

  test('ключ задачи из разных записей', () => {
    expect(normalizeKey('web-281')?.key).toBe('WEB-281')
    expect(normalizeKey('WEB 281')?.key).toBe('WEB-281')
    expect(normalizeKey('https://plane-2.example.test/web/browse/WEB-281/')?.key).toBe('WEB-281')
    expect(normalizeKey('просто текст')).toBeNull()
    expect(normalizeKey('задача 281')).toBeNull()
  })

  test('parseQuickAdd: метка в конце, в начале, без метки', () => {
    expect(parseQuickAdd('Починить форму #web', 'INBOX')).toEqual({ title: 'Починить форму', project: 'WEB' })
    expect(parseQuickAdd('#seo собрать семантику', 'INBOX')).toEqual({ title: 'собрать семантику', project: 'SEO' })
    expect(parseQuickAdd('Проверить выдачу #Seo.', 'INBOX')).toEqual({ title: 'Проверить выдачу.', project: 'SEO' })
    expect(parseQuickAdd('  без   метки ', 'inbox')).toEqual({ title: 'без метки', project: 'INBOX' })
    // C# и #12 — не метки
    expect(parseQuickAdd('переписать на C# пункт #12', 'WEB')).toEqual({
      title: 'переписать на C# пункт #12',
      project: 'WEB',
    })
  })

  test('сутки по часовому поясу', () => {
    const msk = dayBounds(NOW, 180)
    expect(msk.start).toBe(Date.UTC(2026, 10, 3, 21, 0))
    expect(msk.end - msk.start).toBe(86_400_000)
    expect(dayBounds(NOW, 0).start).toBe(Date.UTC(2026, 10, 3))
    expect(dayBounds(NOW, -300).start).toBe(Date.UTC(2026, 10, 3, 5, 0))
  })

  test('pickState: первый статус группы по sequence, запасная группа', () => {
    const web = WEB_STATES.filter((s) => s.group !== 'triage') as unknown as PlaneState[]
    expect(pickState(web, 'todo')?.id).toBe('st-new')
    expect(pickState(web, 'doing')?.id).toBe('st-progress')
    expect(pickState(web, 'done')?.id).toBe('st-done')
    expect(pickState(web, 'cancelled')?.id).toBe('st-cancel')
    const seo = SEO_STATES as unknown as PlaneState[]
    expect(pickState(seo, 'todo')?.id).toBe('se-backlog')
    expect(pickState(seo, 'cancelled')).toBeNull()
  })
})

// ---------- проекты и статусы ----------

describe('проекты и статусы', () => {
  test('проекты: страницы, архивные мимо, кэш до refresh(), ключ только в заголовке', async () => {
    const { fetch, calls } = fakePlane([
      [
        'GET',
        /^\/projects\/\?/,
        (c) =>
          ok(
            c.path.includes('cursor=')
              ? page([project('p-seo', 'SEO'), project('p-old', 'OLD', { archived_at: '2026-01-01T00:00:00Z' })])
              : page([project('p-web', 'WEB')], '100:1:0'),
          ),
      ],
    ])
    const plane = createPlane(fetch, { url: 'plane.example.test/', workspace: 'web', apiKey: KEY })
    const list = await plane.projects()
    expect(list).toEqual([
      { id: 'p-web', identifier: 'WEB', name: 'Проект WEB' },
      { id: 'p-seo', identifier: 'SEO', name: 'Проект SEO' },
    ])
    expect(calls[0]?.url).toBe(`${API}/projects/?per_page=100`)
    expect(calls[1]?.url).toBe(`${API}/projects/?per_page=100&cursor=100%3A1%3A0`)
    expect(calls[0]?.headers['X-API-Key']).toBe(KEY)
    expect(calls.some((c) => c.url.includes(KEY))).toBe(false)

    await plane.projects()
    expect(calls).toHaveLength(2)
    plane.refresh()
    await plane.projects()
    expect(calls).toHaveLength(4)
  })

  test('статусы: по sequence, triage мимо', async () => {
    const { fetch } = fakePlane(standardRoutes())
    const plane = createPlane(fetch, cfg)
    const list = await plane.states('p-web')
    expect(list.map((s) => s.id)).toEqual([
      'st-backlog',
      'st-new',
      'st-ready',
      'st-progress',
      'st-review',
      'st-done',
      'st-cancel',
    ])
    expect(list[0]).toEqual({ id: 'st-backlog', name: 'Бэклог', group: 'backlog', sequence: 15000 })
  })
})

// ---------- доска ----------

describe('доска', () => {
  test('колонки, сортировка, «готово сегодня» по Москве, пагинация', async () => {
    const { fetch, calls } = fakePlane(standardRoutes())
    const plane = createPlane(fetch, cfg)
    const board = await plane.board({ now: NOW, tzOffsetMin: 180 })

    expect(keys(board.todo)).toEqual(['WEB-3', 'WEB-2', 'WEB-1'])
    expect(keys(board.doing)).toEqual(['WEB-4', 'SEO-1', 'WEB-5'])
    expect(keys(board.doneToday)).toEqual(['WEB-9', 'WEB-7'])
    expect(board.total).toEqual({ todo: 3, doing: 3, doneToday: 2 })
    expect(board.projects.map((p) => p.identifier)).toEqual(['WEB', 'SEO'])
    expect(board.fetchedAt).toBe(NOW)
    expect(board.warnings).toEqual([])

    const items = calls.filter((c) => c.path.startsWith('/projects/p-web/work-items/'))
    expect(items).toHaveLength(2)
    expect(items[0]?.path).toContain('expand=state')
    expect(items[0]?.path).toContain('per_page=100')
    expect(items[1]?.path).toContain('cursor=100%3A1%3A0')
    // expand=state дал статус сразу: отдельные запросы статусов не нужны
    expect(calls.some((c) => c.path.includes('/states/'))).toBe(false)
  })

  test('карточка задачи: ключ, ссылка, статус, описание, время', async () => {
    const { fetch } = fakePlane(standardRoutes())
    const board = await createPlane(fetch, cfg).board({ now: NOW, tzOffsetMin: 180 })
    const done = board.doneToday.find((i) => i.key === 'WEB-7')
    expect(done).toEqual({
      id: 'wi-p-web-7',
      key: 'WEB-7',
      projectId: 'p-web',
      project: 'WEB',
      title: 'Задача 7',
      description: 'Шаг 1 & шаг 2\n\n• раз\n• два',
      group: 'completed',
      stateId: 'st-done',
      stateName: 'Готово',
      priority: 'none',
      updatedAt: Date.UTC(2026, 10, 3, 21, 10),
      completedAt: Date.UTC(2026, 10, 3, 21, 10),
      url: 'https://plane.example.test/web/browse/WEB-7/',
    })
  })

  test('«сегодня» по UTC шире, чем по Москве', async () => {
    const { fetch } = fakePlane(standardRoutes())
    const board = await createPlane(fetch, cfg).board({ now: NOW, tzOffsetMin: 0 })
    expect(keys(board.doneToday)).toEqual(['WEB-9', 'WEB-7', 'WEB-6'])
  })

  test('фильтр по проектам без учёта регистра и неизвестная метка', async () => {
    const { fetch, calls } = fakePlane(standardRoutes())
    const board = await createPlane(fetch, cfg).board({ now: NOW, tzOffsetMin: 180, projects: ['seo', 'NOPE'] })
    expect(keys(board.doing)).toEqual(['SEO-1'])
    expect(board.todo).toEqual([])
    expect(board.warnings).toEqual(['Проекта NOPE в Plane нет.'])
    expect(calls.some((c) => c.path.startsWith('/projects/p-web/'))).toBe(false)
  })

  test('лимит 50 на колонку, total — до обрезки', () => {
    const many: WorkItem[] = Array.from({ length: 60 }, (_, n) => ({
      id: `x${n}`,
      key: `WEB-${n + 1}`,
      projectId: 'p-web',
      project: 'WEB',
      title: `Задача ${n + 1}`,
      description: '',
      group: 'unstarted',
      stateId: 'st-new',
      stateName: 'Новое',
      priority: n === 59 ? 'urgent' : 'none',
      updatedAt: n,
      completedAt: null,
      url: '',
    }))
    const laid = layoutBoard(many, { now: NOW, tzOffsetMin: 180 })
    expect(laid.todo).toHaveLength(50)
    expect(laid.total.todo).toBe(60)
    expect(laid.todo[0]?.key).toBe('WEB-60') // срочная первой
    expect(laid.todo[1]?.key).toBe('WEB-59') // потом самая свежая
  })

  test('статус без expand и длинное описание', async () => {
    const long = '<p>' + 'а'.repeat(2500) + '</p>'
    // первый подходящий маршрут выигрывает: этот перекрывает стандартный
    const { fetch, calls } = fakePlane([
      [
        'GET',
        /^\/projects\/p-web\/work-items\/\?/,
        () => ok(page([{ ...issue('p-web', 11, 'st-progress'), state: 'st-progress', description_html: long }])),
      ],
      ...standardRoutes(),
    ])
    const board = await createPlane(fetch, cfg).board({ now: NOW, tzOffsetMin: 180, projects: ['WEB'] })
    const it = board.doing[0]
    expect(it?.stateName).toBe('В работе')
    expect(it?.group).toBe('started')
    expect(it?.description.length).toBe(2000)
    expect(it?.description.endsWith('…')).toBe(true)
    expect(calls.filter((c) => c.path.startsWith('/projects/p-web/states/'))).toHaveLength(1)
  })

  test('старый сервер без work-items: один раз переключаемся на issues', async () => {
    const { fetch, calls } = fakePlane(standardRoutes('issues'))
    const plane = createPlane(fetch, cfg)
    const first = await plane.board({ now: NOW, tzOffsetMin: 180, projects: ['WEB'] })
    expect(keys(first.doing)).toEqual(['WEB-4', 'WEB-5'])
    const tried = calls.filter((c) => c.path.includes('/work-items/')).length
    expect(tried).toBe(1)

    await plane.board({ now: NOW, tzOffsetMin: 180, projects: ['WEB'] })
    expect(calls.filter((c) => c.path.includes('/work-items/')).length).toBe(1)
    expect((await plane.item('WEB-4'))?.key).toBe('WEB-4')
    expect(calls.map((c) => c.path)).toContain('/issues/WEB-4/?expand=state')
    expect(calls.filter((c) => c.path.includes('/work-items/')).length).toBe(1)
  })
})

// ---------- задача, перенос, добавление, комментарий ----------

describe('действия', () => {
  test('item: по ключу, нет такой — null, мусор — bad_request', async () => {
    const { fetch, calls } = fakePlane(standardRoutes())
    const plane = createPlane(fetch, cfg)
    const it = await plane.item('web-4')
    expect(it?.key).toBe('WEB-4')
    expect(it?.title).toBe('Реестр сайтов')
    expect(it?.stateName).toBe('Ревью')
    expect(it?.url).toBe('https://plane.example.test/web/browse/WEB-4/')
    expect(calls.map((c) => c.path)).toContain('/work-items/WEB-4/?expand=state')
    expect(await plane.item('WEB-999')).toBeNull()
    expect((await caught(plane.item('без ключа'))).kind).toBe('bad_request')
  })

  function movable() {
    const patches: Array<{ id: string; body: Raw }> = []
    const routes: Route[] = [
      ...standardRoutes(),
      [
        'PATCH',
        /^\/projects\/(p-[a-z]+)\/work-items\/([^/]+)\/$/,
        (c, m) => {
          const body = JSON.parse(c.body ?? '{}') as Raw
          patches.push({ id: m[2] ?? '', body })
          const all = [...WEB_PAGE_1, ...WEB_PAGE_2, ...SEO_ITEMS]
          const cur = all.find((i) => i.id === m[2])
          if (!cur) return { status: 404, body: { error: 'The required object does not exist.' } }
          // ответ PATCH: статус — голый id, без expand
          const done = String(body.state).endsWith('done')
          return ok({ ...cur, state: body.state, completed_at: done ? '2026-11-03T21:29:00Z' : null, updated_at: '2026-11-03T21:29:00Z' })
        },
      ],
    ]
    const { fetch, calls } = fakePlane(routes)
    return { plane: createPlane(fetch, cfg), patches, calls }
  }

  test('move выбирает первый статус нужной группы', async () => {
    const { plane, patches } = movable()
    const todo = await plane.move('WEB-1', 'todo') // бэклог → первый unstarted
    expect(patches[0]).toEqual({ id: 'wi-p-web-1', body: { state: 'st-new' } })
    expect(todo.group).toBe('unstarted')
    expect(todo.stateName).toBe('Новое')

    await plane.move('WEB-2', 'doing')
    expect(patches[1]?.body).toEqual({ state: 'st-progress' })

    const done = await plane.move('WEB-4', 'done')
    expect(patches[2]?.body).toEqual({ state: 'st-done' })
    expect(done.group).toBe('completed')
    expect(done.completedAt).toBe(Date.UTC(2026, 10, 3, 21, 29))

    await plane.move('WEB-5', 'cancelled')
    expect(patches[3]?.body).toEqual({ state: 'st-cancel' })
  })

  test('move: уже в группе — без запроса; нет unstarted — бэклог; нет группы — ошибка', async () => {
    const { plane, patches } = movable()
    const same = await plane.move('WEB-4', 'doing') // «Ревью» уже started
    expect(same.stateId).toBe('st-review')
    expect(patches).toHaveLength(0)

    await plane.move('SEO-1', 'todo')
    expect(patches[0]?.body).toEqual({ state: 'se-backlog' })

    const err = await caught(plane.move('SEO-1', 'cancelled'))
    expect(err.kind).toBe('bad_request')
    expect(err.message).toContain('SEO')

    expect((await caught(plane.move('WEB-999', 'done'))).kind).toBe('not_found')
  })

  test('add: проект по метке без учёта регистра, описание в HTML', async () => {
    const posts: Raw[] = []
    const { fetch, calls } = fakePlane([
      ...standardRoutes(),
      [
        'POST',
        /^\/projects\/(p-[a-z]+)\/work-items\/$/,
        (c, m) => {
          const body = JSON.parse(c.body ?? '{}') as Raw
          posts.push(body)
          // ответ создания: 201, статус — id статуса по умолчанию
          return {
            status: 201,
            body: { ...issue(m[1] ?? '', 10, 'st-backlog'), name: body.name, description_html: body.description_html, state: m[1] === 'p-seo' ? 'se-backlog' : 'st-backlog' },
          }
        },
      ],
    ])
    const plane = createPlane(fetch, { ...cfg, inboxProject: 'SEO' })
    const made = await plane.add({ title: '  Починить   форму ', project: 'web', description: 'строка <1>\nвторая\n\nабзац' })
    expect(posts[0]).toEqual({ name: 'Починить форму', description_html: '<p>строка &lt;1&gt;<br>вторая</p><p>абзац</p>' })
    const post = calls.find((c) => c.method === 'POST')
    expect(post?.path).toBe('/projects/p-web/work-items/')
    expect(post?.headers['Content-Type']).toBe('application/json')
    expect(made.key).toBe('WEB-10')
    expect(made.group).toBe('backlog')
    expect(made.description).toBe('строка <1>\nвторая\n\nабзац')
    expect(made.url).toBe('https://plane.example.test/web/browse/WEB-10/')

    const inbox = await plane.add({ title: 'Без метки' })
    expect(inbox.key).toBe('SEO-10')

    expect((await caught(plane.add({ title: '   ' }))).kind).toBe('bad_request')
  })

  test('add в несуществующий проект перечисляет доступные', async () => {
    const { fetch } = fakePlane(standardRoutes())
    const err = await caught(createPlane(fetch, cfg).add({ title: 'Задача без INBOX' }))
    expect(err.kind).toBe('not_found')
    expect(err.message).toContain('INBOX')
    expect(err.message).toContain('WEB, SEO')
  })

  test('comment: HTML с экранированием', async () => {
    const bodies: string[] = []
    const { fetch } = fakePlane([
      ...standardRoutes(),
      [
        'POST',
        /^\/projects\/p-web\/work-items\/wi-p-web-4\/comments\/$/,
        (c) => {
          bodies.push(c.body ?? '')
          return { status: 201, body: { id: 'c1', comment_html: '…' } }
        },
      ],
    ])
    await createPlane(fetch, cfg).comment('WEB-4', 'Взята в сессию «x & y»')
    expect(bodies).toEqual([JSON.stringify({ comment_html: '<p>Взята в сессию «x &amp; y»</p>' })])
  })
})

// ---------- ошибки ----------

describe('ошибки', () => {
  test('401 → auth, русский текст без ключа', async () => {
    const { fetch } = fakePlane([['GET', /./, () => ({ status: 401, body: { detail: 'Given API token is not valid' } })]])
    const err = await caught(createPlane(fetch, cfg).board({ now: NOW, tzOffsetMin: 180 }))
    expect(err).toBeInstanceOf(PlaneError)
    expect(err.kind).toBe('auth')
    expect(err.status).toBe(401)
    expect(err.message).toContain('API-ключ')
    expect(err.message).not.toContain(KEY)
  })

  test('сеть → network, ключ из текста исключения вычищен', async () => {
    const { fetch } = fakePlane([['GET', /./, () => new Error(`connect ETIMEDOUT (X-API-Key: ${KEY})`)]])
    const err = await caught(createPlane(fetch, cfg).projects())
    expect(err.kind).toBe('network')
    expect(err.message).toContain('Plane недоступен')
    expect(err.message).toContain('plane.example.test')
    expect(err.message).not.toContain(KEY)
  })

  test('404 проекта, 400 с ошибками полей, 429, 500 HTML, эхо ключа', async () => {
    const { fetch } = fakePlane([
      ['GET', /^\/projects\/\?/, () => ok(page([project('p-web', 'WEB')]))],
      ['GET', /^\/projects\/p-web\/states\/\?/, () => ({ status: 500, raw: '<html><body>Bad Gateway</body></html>' })],
      ['POST', /^\/projects\/p-web\/work-items\/$/, () => ({ status: 400, body: { name: ['This field is required.'] } })],
      ['GET', /^\/work-items\/WEB-1\//, () => ({ status: 429, body: { detail: 'Request was throttled.' } })],
      ['GET', /^\/work-items\/WEB-2\//, () => ({ status: 403, body: { error: `key ${KEY} has no access` } })],
    ])
    const plane = createPlane(fetch, cfg)

    const e500 = await caught(plane.states('p-web'))
    expect(e500.kind).toBe('server')
    expect(e500.status).toBe(500)
    expect(e500.message).not.toContain('<html>')

    const e400 = await caught(plane.add({ title: 'x', project: 'WEB' }))
    expect(e400.kind).toBe('bad_request')
    expect(e400.message).toContain('name: This field is required.')

    const e429 = await caught(plane.item('WEB-1'))
    expect(e429.kind).toBe('server')
    expect(e429.message).toContain('429')

    const e403 = await caught(plane.item('WEB-2'))
    expect(e403.kind).toBe('auth')
    expect(e403.message).not.toContain(KEY)
    expect(e403.message).toContain('***')
  })

  test('нет маршрута вообще (неверный адрес) → not_found с подсказкой', async () => {
    const { fetch } = fakePlane([['GET', /./, () => ({ status: 404, raw: '<h1>Not Found</h1>' })]])
    const err = await caught(createPlane(fetch, cfg).projects())
    expect(err.kind).toBe('not_found')
    expect(err.message).toContain('planeUrl')
  })

  test('закрытый проект не гасит доску', async () => {
    const { fetch } = fakePlane([
      ['GET', /^\/projects\/p-seo\/work-items\/\?/, () => ({ status: 403, body: { error: 'You do not have permission' } })],
      ...standardRoutes(),
    ])
    const board = await createPlane(fetch, cfg).board({ now: NOW, tzOffsetMin: 180 })
    expect(keys(board.doing)).toEqual(['WEB-4', 'WEB-5'])
    expect(board.warnings?.[0]).toContain('SEO')
  })

  test('без ключа → config, в сеть не ходим', async () => {
    const { fetch, calls } = fakePlane(standardRoutes())
    const err = await caught(createPlane(fetch, { ...cfg, apiKey: '  ' }).projects())
    expect(err.kind).toBe('config')
    expect(err.message).toContain('planeApiKey')
    expect(calls).toHaveLength(0)
  })
})

// ---------- tasks.json и тексты для модели ----------

describe('представления', () => {
  async function sampleBoard(): Promise<Board> {
    const { fetch } = fakePlane(standardRoutes())
    return createPlane(fetch, cfg).board({ now: NOW, tzOffsetMin: 180 })
  }

  test('tasksFile: счётчики, в начале «в работе», форма контракта', async () => {
    const board = await sampleBoard()
    const file = tasksFile({ ...board, total: { todo: 70, doing: 3, doneToday: 2 } }, NOW)
    expect(file.v).toBe(1)
    expect(file.updatedAt).toBe(NOW)
    expect(file.ok).toBe(true)
    expect(file.error).toBeNull()
    expect(file.counts).toEqual({ todo: 70, doing: 3, doneToday: 2 })
    expect(file.top.map((t) => t.id)).toEqual(['WEB-4', 'SEO-1', 'WEB-5', 'WEB-3', 'WEB-2', 'WEB-1'])
    expect(file.top[0]).toEqual({
      id: 'WEB-4',
      title: 'Реестр сайтов',
      group: 'started',
      project: 'WEB',
      url: 'https://plane.example.test/web/browse/WEB-4/',
    })
    const bare = tasksFile({ todo: [], doing: [], doneToday: [], projects: [], fetchedAt: NOW }, NOW)
    expect(bare.counts).toEqual({ todo: 0, doing: 0, doneToday: 0 })
    expect(bare.top).toEqual([])
  })

  test('backlogText группирует по колонкам', async () => {
    const board = await sampleBoard()
    const text = backlogText([...board.todo, ...board.doing, ...board.doneToday])
    const lines = text.split('\n')
    expect(lines[0]).toBe('Задачи Plane — 8')
    expect(lines[2]).toBe('В работе (3):')
    expect(lines[3]).toBe('WEB-4 · Ревью · Реестр сайтов · !срочно')
    expect(text).toContain('Не взято (3):\nWEB-3 · К работе · Задача 3 · !высокий')
    expect(text).toContain('Готово (2):')
    expect(backlogText([], 'Задачи SEO')).toBe('Задачи SEO: задач нет.')
  })

  test('itemText: ключ, статус, проект, ссылка, описание', async () => {
    const board = await sampleBoard()
    const it = board.doneToday.find((i) => i.key === 'WEB-7')
    if (!it) throw new Error('нет WEB-7')
    const text = itemText(it, 180)
    expect(text).toContain('WEB-7 · Задача 7')
    expect(text).toContain('Статус: Готово (готово) · приоритет: без приоритета')
    expect(text).toContain('Проект: WEB · обновлена 2026-11-04 00:10 UTC+3')
    expect(text).toContain('Ссылка: https://plane.example.test/web/browse/WEB-7/')
    expect(text.endsWith('Шаг 1 & шаг 2\n\n• раз\n• два')).toBe(true)
  })
})
