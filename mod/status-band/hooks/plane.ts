// Клиент Plane (самохостинг) для мода status-band.
//
// Чистый модуль: никакого `$` движка, вся сеть — через переданную функцию
// `fetch`. Им пользуются полоса (колонки «Не взято / В работе / Готово
// сегодня»), писатель `tasks.json` и инструмент модели `backlog`.
//
// Эндпоинты — REST API v1 Plane (`/api/v1/workspaces/{slug}/…`, ключ в
// заголовке `X-API-Key`):
//   GET   /projects/                                   — проекты (страницами)
//   GET   /projects/{id}/states/                       — статусы проекта
//   GET   /projects/{id}/work-items/?expand=state      — задачи (страницами)
//   GET   /work-items/{WEB-281}/?expand=state          — задача по ключу
//   POST  /projects/{id}/work-items/                   — новая задача
//   PATCH /projects/{id}/work-items/{uuid}/            — смена статуса
//   POST  /projects/{id}/work-items/{uuid}/comments/   — комментарий
// Префикс `work-items/` появился в Plane 1.x; до него (≤ 0.28) был только
// `issues/` с теми же ответами, а нынешний сервер держит оба. Клиент сначала
// идёт в `work-items/`, а если сервер отвечает 404 «Page not found.»
// (маршрута нет, а не объекта), один раз переключается на `issues/` и
// запоминает выбор.
//
// Ключ живёт только в заголовке запроса: ни в адресе, ни в тексте ошибок,
// ни в возвращаемых данных его нет (тексты ошибок дополнительно чистятся).

export type Fetch = (
  url: string,
  init?: { method?: string; headers?: Record<string, string>; body?: string },
) => Promise<{ status: number; ok: boolean; text: string }>

export type PlaneConfig = {
  /** Адрес Plane без слеша в конце, например https://plane.example.com */
  url: string
  /** Slug рабочего пространства из адреса Plane */
  workspace: string
  apiKey: string
  /** Идентификатор проекта для задач без метки; по умолчанию INBOX */
  inboxProject?: string
}

export type Group = 'backlog' | 'unstarted' | 'started' | 'completed' | 'cancelled'
export type PlaneProject = { id: string; identifier: string; name: string }
export type PlaneState = { id: string; name: string; group: Group; sequence: number }

export type WorkItem = {
  id: string
  /** WEB-281 */
  key: string
  projectId: string
  /** Идентификатор проекта (WEB) */
  project: string
  title: string
  /** Простой текст из description_stripped / description_html, ≤ 2000 знаков */
  description: string
  group: Group
  stateId: string
  stateName: string
  /** urgent | high | medium | low | none */
  priority: string
  updatedAt: number
  completedAt: number | null
  /** Ссылка в веб-интерфейс Plane: {url}/{workspace}/browse/WEB-281/ */
  url: string
}

export type Board = {
  todo: WorkItem[]
  doing: WorkItem[]
  doneToday: WorkItem[]
  /** Все видимые ключу проекты (для фильтра), не только выбранные */
  projects: PlaneProject[]
  fetchedAt: number
  /** Сколько задач в колонке до обрезки по 50 */
  total?: { todo: number; doing: number; doneToday: number }
  /** Что не удалось показать: проект без доступа, неизвестная метка и т. п. */
  warnings?: string[]
}

export type Move = 'todo' | 'doing' | 'done' | 'cancelled'
export type Column = 'todo' | 'doing' | 'done'
export type PlaneErrorKind = 'auth' | 'network' | 'not_found' | 'bad_request' | 'server' | 'config'

export class PlaneError extends Error {
  kind: PlaneErrorKind
  status?: number
  constructor(kind: PlaneErrorKind, message: string, status?: number) {
    super(message)
    this.name = 'PlaneError'
    this.kind = kind
    if (status !== undefined) this.status = status
  }
}

export type TasksFile = {
  v: 1
  updatedAt: number
  ok: true
  error: null
  counts: { todo: number; doing: number; doneToday: number }
  top: Array<{ id: string; title: string; group: Group; project: string; url: string }>
}

export type PlaneClient = {
  /** Проекты; кэш на время жизни клиента, сброс — refresh() */
  projects(): Promise<PlaneProject[]>
  /** Статусы проекта по порядку Plane (sequence); тоже кэшируются */
  states(projectId: string): Promise<PlaneState[]>
  board(opts: { now: number; tzOffsetMin: number; projects?: string[] }): Promise<Board>
  /** Задача по ключу (WEB-281, web-281, ссылка на неё); null — такой нет */
  item(key: string): Promise<WorkItem | null>
  add(input: { title: string; project?: string; description?: string }): Promise<WorkItem>
  move(key: string, to: Move): Promise<WorkItem>
  comment(key: string, text: string): Promise<void>
  refresh(): void
}

// ---------- константы ----------

export const GROUPS: readonly Group[] = ['backlog', 'unstarted', 'started', 'completed', 'cancelled']
export const COLUMN_LIMIT = 50
export const DESCRIPTION_MAX = 2000
const PER_PAGE = 100 // потолок per_page в API v1
const MAX_PAGES = 20 // не больше 2000 задач на проект за один опрос
const TOP_LIMIT = 10
const TITLE_MAX = 255 // длина name в Plane
const DAY = 86_400_000

const PRIORITY_RANK: Record<string, number> = { urgent: 0, high: 1, medium: 2, low: 3, none: 4 }
const PRIORITY_RU: Record<string, string> = {
  urgent: 'срочный',
  high: 'высокий',
  medium: 'средний',
  low: 'низкий',
  none: 'без приоритета',
}
const GROUP_RU: Record<Group, string> = {
  backlog: 'бэклог',
  unstarted: 'не начата',
  started: 'в работе',
  completed: 'готово',
  cancelled: 'отменена',
}
/** Куда двигаем: группы статусов по убыванию предпочтения */
const MOVE_GROUPS: Record<Move, readonly Group[]> = {
  todo: ['unstarted', 'backlog'],
  doing: ['started'],
  done: ['completed'],
  cancelled: ['cancelled'],
}
const MOVE_RU: Record<Move, string> = {
  todo: '«Не взято»',
  doing: '«В работе»',
  done: '«Готово»',
  cancelled: '«Отменено»',
}
// Поля задачи, которые просим у списка: остальное (метки, исполнители,
// description_binary…) модулю не нужно, а ответ без них в разы короче.
const ITEM_FIELDS = [
  'id',
  'name',
  'sequence_id',
  'priority',
  'state',
  'project',
  'created_at',
  'updated_at',
  'completed_at',
  'archived_at',
  'description_html',
].join(',')

// ---------- мелкие помощники ----------

type Json = { [k: string]: unknown }
const isObj = (v: unknown): v is Json => typeof v === 'object' && v !== null && !Array.isArray(v)
const str = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined)
const idOf = (v: unknown): string | undefined =>
  typeof v === 'string' && v ? v : isObj(v) && typeof v.id === 'string' && v.id ? v.id : undefined
const isGroup = (v: unknown): v is Group => typeof v === 'string' && (GROUPS as readonly string[]).includes(v)
const enc = (s: string) => encodeURIComponent(s)
const squash = (s: string) => s.replace(/\s+/g, ' ').trim()

/** Обрезать строку до n знаков, с многоточием */
export function clip(s: string, n: number): string {
  if (s.length <= n) return s
  return s.slice(0, Math.max(0, n - 1)).trimEnd() + '…'
}

const ISO =
  /^(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2})(?::(\d{2})(?:[.,](\d+))?)?)?\s*(Z|[+-]\d{2}(?::?\d{2})?)?$/i

/**
 * Время Plane (`2026-11-03T09:14:49.256571Z`) → мс эпохи. Свой разбор, а не
 * Date.parse: микросекунды и смещения разные движки читают по-разному.
 */
export function parseTime(v: unknown): number | null {
  if (typeof v !== 'string') return null
  const m = ISO.exec(v.trim())
  if (!m) return null
  const frac = m[7] ? Number((m[7] + '00').slice(0, 3)) : 0
  let t = Date.UTC(
    Number(m[1]),
    Number(m[2]) - 1,
    Number(m[3]),
    Number(m[4] ?? 0),
    Number(m[5] ?? 0),
    Number(m[6] ?? 0),
    frac,
  )
  const z = m[8]
  if (z && z.toUpperCase() !== 'Z') {
    const digits = z.slice(1).replace(':', '')
    const minutes = Number(digits.slice(0, 2)) * 60 + Number(digits.slice(2, 4) || 0)
    t -= (z.startsWith('-') ? -1 : 1) * minutes * 60_000
  }
  return Number.isFinite(t) ? t : null
}

const NAMED_ENTITIES: Record<string, string> = {
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  apos: "'",
  nbsp: ' ',
  mdash: '—',
  ndash: '–',
  hellip: '…',
  laquo: '«',
  raquo: '»',
  copy: '©',
}

function decodeEntities(s: string): string {
  return s.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (all, body: string) => {
    if (body[0] === '#') {
      const code = body[1] === 'x' || body[1] === 'X' ? parseInt(body.slice(2), 16) : parseInt(body.slice(1), 10)
      return Number.isFinite(code) && code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : all
    }
    return NAMED_ENTITIES[body.toLowerCase()] ?? all
  })
}

/**
 * HTML описания Plane → простой текст: абзацы и пункты списков сохраняются.
 * Редактор Plane (tiptap) кладёт пункт как `<li><p>…</p></li>`, а пункт
 * чек-листа — `<li data-checked="true">…<div><p>…</p></div></li>`.
 */
export function htmlToText(html: string): string {
  const s = html
    .replace(/<\s*(script|style)\b[^>]*>[\s\S]*?<\s*\/\s*\1\s*>/gi, '')
    .replace(/(<\s*\/\s*(p|div)\s*>\s*)+(?=<\s*\/\s*li\s*>)/gi, '')
    .replace(/<\s*br\s*\/?\s*>/gi, '\n')
    .replace(/<\s*li\b([^>]*)>/gi, (_all, attrs: string) =>
      /data-checked\s*=\s*"true"/i.test(attrs) ? '\n☑ ' : /data-checked\s*=\s*"false"/i.test(attrs) ? '\n☐ ' : '\n• ',
    )
    .replace(/<\s*\/\s*li\s*>/gi, '')
    .replace(/<\s*\/?\s*(p|div|h[1-6]|ul|ol|blockquote|pre|tr|table|hr)\b[^>]*>/gi, '\n')
    .replace(/<[^>]*>/g, '')
  return decodeEntities(s)
    .replace(/\r\n?/g, '\n')
    .replace(/[ \t ]+/g, ' ')
    .split('\n')
    .map((l) => l.trim())
    .join('\n')
    .replace(/^([•☑☐])\n+/gm, '$1 ') // маркер пункта и его текст — в одну строку
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
}

/** Простой текст → HTML для Plane: пустая строка делит абзацы, перенос → <br> */
export function textToHtml(text: string): string {
  const paras = text
    .replace(/\r\n?/g, '\n')
    .split(/\n\s*\n/)
    .map((p) => p.trim())
    .filter(Boolean)
  if (!paras.length) return '<p></p>'
  return paras.map((p) => `<p>${escapeHtml(p).replace(/\n/g, '<br>')}</p>`).join('')
}

/**
 * Ключ задачи из того, что дал человек или модель: «WEB-281», «web-281»,
 * «WEB 281» (через пробел — только заглавными), ссылка …/browse/WEB-281/
 * (берётся последнее совпадение: хост тоже бывает с дефисом и цифрой).
 * null — не похоже на ключ.
 */
export function normalizeKey(input: string): { key: string; project: string; seq: number } | null {
  const s = String(input)
  let all = [...s.matchAll(/([\p{L}][\p{L}\p{N}_]*)-(\d+)/gu)]
  if (!all.length) all = [...s.matchAll(/(\p{Lu}[\p{Lu}\p{N}_]*)\s+(\d+)/gu)]
  const m = all[all.length - 1]
  if (!m || !m[1] || !m[2]) return null
  const project = m[1].toUpperCase()
  const seq = Number(m[2])
  return { key: `${project}-${seq}`, project, seq }
}

/** Границы текущих местных суток. tzOffsetMin = местное − UTC в минутах (Москва 180) */
export function dayBounds(now: number, tzOffsetMin: number): { start: number; end: number } {
  const off = tzOffsetMin * 60_000
  const start = Math.floor((now + off) / DAY) * DAY - off
  return { start, end: start + DAY }
}

/** В какую колонку полосы попадает группа статуса; cancelled — никуда */
export function columnOf(group: Group): Column | null {
  if (group === 'backlog' || group === 'unstarted') return 'todo'
  if (group === 'started') return 'doing'
  if (group === 'completed') return 'done'
  return null
}

const fresh = (i: WorkItem) => Math.max(i.updatedAt, i.completedAt ?? 0)

/** Порядок в колонке: приоритет (urgent → none), потом свежесть, потом ключ */
export function compareItems(a: WorkItem, b: WorkItem): number {
  const pa = PRIORITY_RANK[a.priority] ?? 4
  const pb = PRIORITY_RANK[b.priority] ?? 4
  if (pa !== pb) return pa - pb
  const fa = fresh(a)
  const fb = fresh(b)
  if (fa !== fb) return fb - fa
  return a.key.localeCompare(b.key, 'en', { numeric: true })
}

/** Разложить задачи по колонкам: сортировка, «сегодня» по местным суткам, лимит */
export function layoutBoard(
  items: WorkItem[],
  opts: { now: number; tzOffsetMin: number; limit?: number },
): Pick<Board, 'todo' | 'doing' | 'doneToday'> & { total: { todo: number; doing: number; doneToday: number } } {
  const { start, end } = dayBounds(opts.now, opts.tzOffsetMin)
  const limit = opts.limit ?? COLUMN_LIMIT
  const todo: WorkItem[] = []
  const doing: WorkItem[] = []
  const doneToday: WorkItem[] = []
  for (const i of items) {
    const col = columnOf(i.group)
    if (col === 'todo') todo.push(i)
    else if (col === 'doing') doing.push(i)
    else if (col === 'done') {
      const at = i.completedAt ?? i.updatedAt
      if (at >= start && at < end) doneToday.push(i)
    }
  }
  for (const list of [todo, doing, doneToday]) list.sort(compareItems)
  return {
    todo: todo.slice(0, limit),
    doing: doing.slice(0, limit),
    doneToday: doneToday.slice(0, limit),
    total: { todo: todo.length, doing: doing.length, doneToday: doneToday.length },
  }
}

/** Первый (по sequence) статус для переноса задачи; null — в проекте такого нет */
export function pickState(states: PlaneState[], to: Move): PlaneState | null {
  const sorted = [...states].sort((a, b) => a.sequence - b.sequence)
  for (const g of MOVE_GROUPS[to]) {
    const s = sorted.find((x) => x.group === g)
    if (s) return s
  }
  return null
}

/**
 * Быстрое добавление: «текст #WEB» или «#seo текст». Метка — первое слово
 * с # (с буквы, без учёта регистра → идентификатор в верхнем регистре);
 * без метки — fallbackProject.
 */
export function parseQuickAdd(text: string, fallbackProject: string): { title: string; project: string } {
  const m = /(^|\s)#([\p{L}][\p{L}\p{N}_]*)(?=$|\s|[.,;:!?)])/u.exec(text)
  if (!m || !m[2]) {
    return { title: squash(text), project: fallbackProject.trim().replace(/^#/, '').toUpperCase() }
  }
  const title = squash(text.slice(0, m.index) + ' ' + text.slice(m.index + m[0].length)).replace(
    /\s+([.,;:!?)])/g,
    '$1',
  )
  return { title, project: m[2].toUpperCase() }
}

function normalizeBase(url: string): string {
  let u = String(url ?? '').trim()
  if (!u) return ''
  if (!/^https?:\/\//i.test(u)) u = 'https://' + u
  u = u.replace(/\/+$/, '').replace(/\/api(\/v1)?$/i, '')
  return u
}

// ---------- разбор ответов Plane ----------

function toProject(raw: unknown): PlaneProject | null {
  if (!isObj(raw)) return null
  const id = str(raw.id)
  const identifier = str(raw.identifier)
  if (!id || !identifier) return null
  if (raw.archived_at) return null
  return { id, identifier, name: str(raw.name) ?? identifier }
}

function toState(raw: unknown): PlaneState | null {
  if (!isObj(raw)) return null
  const id = str(raw.id)
  if (!id || !isGroup(raw.group)) return null // triage и прочее служебное мимо
  if (raw.is_triage === true) return null
  const seq = typeof raw.sequence === 'number' ? raw.sequence : Number(raw.sequence)
  return { id, name: str(raw.name) ?? raw.group, group: raw.group, sequence: Number.isFinite(seq) ? seq : 0 }
}

function descriptionOf(r: Json): string {
  const stripped = str(r.description_stripped)
  let text = ''
  if (stripped && stripped.trim()) text = stripped.trim()
  else if (typeof r.description_html === 'string') text = htmlToText(r.description_html)
  else if (typeof r.description === 'string') text = htmlToText(r.description)
  return clip(text, DESCRIPTION_MAX)
}

// ---------- клиент ----------

type Seg = 'work-items' | 'issues'

// Ошибки 404, где у сервера нет самого маршрута (а не объекта).
const ROUTE_MISSING = new WeakSet<PlaneError>()

function routeMissing(text: string): boolean {
  const t = text.trim()
  if (!t.startsWith('{')) return true // HTML-страница 404 старого сервера или прокси
  try {
    const j: unknown = JSON.parse(t)
    return isObj(j) && j.error === 'Page not found.'
  } catch {
    return true
  }
}

function serverMessage(text: string): string {
  const t = text.trim()
  if (!t) return ''
  let j: unknown
  try {
    j = JSON.parse(t)
  } catch {
    return '' // HTML-страницу ошибки не пересказываем
  }
  if (typeof j === 'string') return j
  if (!isObj(j)) return ''
  for (const k of ['error', 'detail', 'message', 'error_message']) {
    const v = j[k]
    if (typeof v === 'string' && v) return v
  }
  // ошибки полей: { name: ["This field is required."] }
  return Object.entries(j)
    .map(([k, v]) => `${k}: ${Array.isArray(v) ? v.filter((x) => typeof x === 'string').join(' ') : String(v)}`)
    .join('; ')
}

export function createPlane(fetch: Fetch, cfg: PlaneConfig): PlaneClient {
  const base = normalizeBase(cfg.url)
  const workspace = String(cfg.workspace ?? '').trim()
  const apiKey = String(cfg.apiKey ?? '').trim()
  const inbox = (String(cfg.inboxProject ?? '').trim() || 'INBOX').replace(/^#/, '')
  const host = base.replace(/^https?:\/\//i, '')
  const wsUrl = `${base}/api/v1/workspaces/${enc(workspace)}`

  const configError = !base
    ? 'Не задан адрес Plane (настройка planeUrl).'
    : !workspace
      ? 'Не задан workspace Plane (настройка planeWorkspace).'
      : !apiKey
        ? 'Нет API-ключа Plane: заполните planeApiKey или PLANE_API_KEY сервера plane в ~/.claude.json.'
        : null

  let seg: Seg = 'work-items'
  let segSure = false
  let projectsCache: Promise<PlaneProject[]> | null = null
  const statesCache = new Map<string, Promise<PlaneState[]>>()

  /** Ключ не должен попасть ни в один текст наружу */
  const scrub = (s: string) => (apiKey.length >= 4 ? s.split(apiKey).join('***') : s)

  function fail(kind: PlaneErrorKind, message: string, status?: number): PlaneError {
    return new PlaneError(kind, scrub(message), status)
  }

  function httpError(status: number, text: string, method: string, path: string): PlaneError {
    const what = `${method} ${path.split('?')[0]}`
    const msg = scrub(clip(serverMessage(text), 200))
    const tail = msg ? ` Ответ: ${msg}` : ''
    if (status === 401) {
      return fail(
        'auth',
        `Plane не принял API-ключ (401). Проверьте planeApiKey или PLANE_API_KEY сервера plane в ~/.claude.json.${tail}`,
        status,
      )
    }
    if (status === 403) {
      return fail('auth', `Plane запретил доступ (403) к ${what}: у владельца ключа нет прав.${tail}`, status)
    }
    if (status === 404) {
      if (routeMissing(text)) {
        const e = fail(
          'not_found',
          `На ${host} нет такого адреса API Plane (404, ${what}). Проверьте planeUrl и planeWorkspace.`,
          status,
        )
        ROUTE_MISSING.add(e)
        return e
      }
      return fail('not_found', `В Plane не найдено (404, ${what}).${tail}`, status)
    }
    if (status === 400 || status === 409 || status === 422) {
      return fail('bad_request', `Plane отклонил запрос (${status}, ${what}).${tail}`, status)
    }
    if (status === 429) {
      return fail('server', `Plane ограничил частоту запросов (429): ключу можно 60 запросов в минуту, повторите позже.`, status)
    }
    if (status >= 500) {
      return fail('server', `Сбой на стороне Plane (${status}, ${what}).${tail}`, status)
    }
    return fail('server', `Plane ответил неожиданно (${status}, ${what}).${tail}`, status)
  }

  async function request(method: string, path: string, body?: unknown): Promise<unknown> {
    if (configError) throw new PlaneError('config', configError)
    const headers: Record<string, string> = { 'X-API-Key': apiKey, Accept: 'application/json' }
    const init: { method: string; headers: Record<string, string>; body?: string } = { method, headers }
    if (body !== undefined) {
      headers['Content-Type'] = 'application/json'
      init.body = JSON.stringify(body)
    }
    let res: { status: number; ok: boolean; text: string }
    try {
      res = await fetch(wsUrl + path, init)
    } catch (e) {
      const why = e instanceof Error ? e.message : String(e)
      throw fail('network', `Plane недоступен (${host}): ${clip(why, 160) || 'нет ответа'}.`)
    }
    const text = typeof res.text === 'string' ? res.text : ''
    if (!res.status) throw fail('network', `Plane недоступен (${host}): нет ответа.`)
    if (res.status >= 200 && res.status < 300) {
      if (!text.trim()) return null
      try {
        return JSON.parse(text)
      } catch {
        throw fail('server', `Plane ответил не JSON (${res.status}, ${method} ${path.split('?')[0]}).`, res.status)
      }
    }
    throw httpError(res.status, text, method, path)
  }

  /** Вызов к задачам с выбором префикса work-items / issues (см. шапку) */
  async function viaItems<T>(run: (s: Seg) => Promise<T>): Promise<T> {
    const first = seg
    try {
      const r = await run(first)
      segSure = true
      return r
    } catch (e) {
      if (segSure || !(e instanceof PlaneError) || !ROUTE_MISSING.has(e)) throw e
      const alt: Seg = first === 'work-items' ? 'issues' : 'work-items'
      const r = await run(alt)
      seg = alt
      segSure = true
      return r
    }
  }

  async function listAll(path: string, query: Record<string, string>): Promise<{ rows: unknown[]; truncated: boolean }> {
    const rows: unknown[] = []
    const seen = new Set<string>()
    let cursor: string | null = null
    for (let page = 0; page < MAX_PAGES; page++) {
      const q = { ...query, per_page: String(PER_PAGE), ...(cursor ? { cursor } : {}) }
      const qs = Object.entries(q)
        .map(([k, v]) => `${enc(k)}=${enc(v)}`)
        .join('&')
      const data = await request('GET', `${path}?${qs}`)
      if (Array.isArray(data)) {
        rows.push(...data) // старые ответы без обёртки
        return { rows, truncated: false }
      }
      if (!isObj(data) || !Array.isArray(data.results)) {
        throw fail('server', `Plane вернул список в незнакомом виде (GET ${path}).`)
      }
      rows.push(...data.results)
      const next = str(data.next_cursor)
      if (data.next_page_results !== true || !next || seen.has(next)) return { rows, truncated: false }
      seen.add(next)
      cursor = next
    }
    return { rows, truncated: true }
  }

  function projects(): Promise<PlaneProject[]> {
    if (!projectsCache) {
      const p = listAll('/projects/', {}).then(({ rows }) =>
        rows.map(toProject).filter((x): x is PlaneProject => x !== null),
      )
      projectsCache = p
      p.catch(() => {
        if (projectsCache === p) projectsCache = null
      })
    }
    return projectsCache
  }

  function states(projectId: string): Promise<PlaneState[]> {
    let p = statesCache.get(projectId)
    if (!p) {
      const made = listAll(`/projects/${enc(projectId)}/states/`, {}).then(({ rows }) =>
        rows
          .map(toState)
          .filter((x): x is PlaneState => x !== null)
          .sort((a, b) => a.sequence - b.sequence),
      )
      statesCache.set(projectId, made)
      made.catch(() => {
        if (statesCache.get(projectId) === made) statesCache.delete(projectId)
      })
      p = made
    }
    return p
  }

  async function findProject(ref: string): Promise<PlaneProject> {
    const want = ref.trim().replace(/^#/, '')
    const all = await projects()
    const hit = all.find((p) => p.identifier.toUpperCase() === want.toUpperCase() || p.id === want)
    if (hit) return hit
    const known = all.map((p) => p.identifier).join(', ') || 'нет ни одного'
    throw fail('not_found', `В Plane нет проекта «${want}». Доступные: ${known}.`)
  }

  function itemUrl(key: string): string {
    return `${base}/${enc(workspace)}/browse/${enc(key)}/`
  }

  async function normalize(raw: unknown, hint?: PlaneProject, identifierHint?: string): Promise<WorkItem> {
    if (!isObj(raw)) throw fail('server', 'Plane вернул задачу в незнакомом виде.')
    const id = str(raw.id)
    if (!id) throw fail('server', 'Plane вернул задачу без id.')
    const projectId = idOf(raw.project) ?? idOf(raw.project_id) ?? hint?.id ?? ''
    let project = hint && hint.id === projectId ? hint : undefined
    if (!project && projectId) project = (await projects()).find((p) => p.id === projectId)
    const identifier =
      project?.identifier ?? (isObj(raw.project_detail) ? str(raw.project_detail.identifier) : undefined) ?? identifierHint ?? ''
    const seq = typeof raw.sequence_id === 'number' ? raw.sequence_id : Number(raw.sequence_id)
    const hasKey = Boolean(identifier) && raw.sequence_id != null && Number.isFinite(seq)
    const key = hasKey ? `${identifier}-${seq}` : id

    // статус: объект при expand=state, иначе только id — тогда ищем в статусах проекта
    let stateId = idOf(raw.state) ?? idOf(raw.state_id) ?? ''
    let stateName = isObj(raw.state) ? (str(raw.state.name) ?? '') : ''
    let group: Group | undefined = isObj(raw.state) && isGroup(raw.state.group) ? raw.state.group : undefined
    if (!group && isObj(raw.state_detail) && isGroup(raw.state_detail.group)) {
      group = raw.state_detail.group
      stateName = str(raw.state_detail.name) ?? stateName
      stateId = stateId || (str(raw.state_detail.id) ?? '')
    }
    if (!group && stateId && projectId) {
      let found = (await states(projectId)).find((s) => s.id === stateId)
      if (!found) {
        statesCache.delete(projectId) // статус могли завести только что
        found = (await states(projectId)).find((s) => s.id === stateId)
      }
      if (found) {
        group = found.group
        stateName = found.name
      }
    }

    const priority = str(raw.priority) ?? 'none'
    return {
      id,
      key,
      projectId,
      project: identifier,
      title: squash(str(raw.name) ?? ''),
      description: descriptionOf(raw),
      group: group ?? 'backlog',
      stateId,
      stateName: stateName || GROUP_RU[group ?? 'backlog'],
      priority: priority in PRIORITY_RANK ? priority : 'none',
      updatedAt: parseTime(raw.updated_at) ?? parseTime(raw.created_at) ?? 0,
      completedAt: parseTime(raw.completed_at),
      url: hasKey ? itemUrl(key) : `${base}/${enc(workspace)}/`,
    }
  }

  async function projectItems(p: PlaneProject): Promise<{ items: WorkItem[]; truncated: boolean }> {
    const { rows, truncated } = await viaItems((s) =>
      listAll(`/projects/${enc(p.id)}/${s}/`, { expand: 'state', fields: ITEM_FIELDS }),
    )
    const items: WorkItem[] = []
    for (const r of rows) {
      if (isObj(r) && (r.archived_at || r.is_draft === true || r.deleted_at)) continue
      items.push(await normalize(r, p))
    }
    return { items, truncated }
  }

  async function board(opts: { now: number; tzOffsetMin: number; projects?: string[] }): Promise<Board> {
    const all = await projects()
    const wanted = (opts.projects ?? []).map((s) => s.trim().replace(/^#/, '').toUpperCase()).filter(Boolean)
    const chosen = wanted.length ? all.filter((p) => wanted.includes(p.identifier.toUpperCase())) : all
    const warnings: string[] = []
    for (const w of wanted) {
      if (!all.some((p) => p.identifier.toUpperCase() === w)) warnings.push(`Проекта ${w} в Plane нет.`)
    }
    const lists = await Promise.all(
      chosen.map(async (p) => {
        try {
          const { items, truncated } = await projectItems(p)
          if (truncated) warnings.push(`${p.identifier}: задач больше ${MAX_PAGES * PER_PAGE}, показаны не все.`)
          return items
        } catch (e) {
          // один закрытый проект не должен гасить всю доску; неверный ключ — должен
          if (e instanceof PlaneError && (e.kind === 'not_found' || e.status === 403)) {
            warnings.push(`${p.identifier}: ${e.message}`)
            return []
          }
          throw e
        }
      }),
    )
    const laid = layoutBoard(lists.flat(), opts)
    return { ...laid, projects: all, fetchedAt: opts.now, warnings }
  }

  async function item(key: string): Promise<WorkItem | null> {
    const k = normalizeKey(key)
    if (!k) throw fail('bad_request', `Не похоже на ключ задачи: «${clip(String(key), 60)}». Нужен вид WEB-281.`)
    try {
      const raw = await viaItems((s) => request('GET', `/${s}/${enc(k.key)}/?expand=state`))
      return await normalize(raw, undefined, k.project)
    } catch (e) {
      if (e instanceof PlaneError && e.kind === 'not_found' && !ROUTE_MISSING.has(e)) return null
      throw e
    }
  }

  async function mustItem(key: string): Promise<WorkItem> {
    const it = await item(key)
    if (!it) throw fail('not_found', `Задачи ${normalizeKey(key)?.key ?? key} в Plane нет.`)
    return it
  }

  async function projectOf(it: WorkItem): Promise<PlaneProject | undefined> {
    return (await projects()).find((p) => p.id === it.projectId)
  }

  async function add(input: { title: string; project?: string; description?: string }): Promise<WorkItem> {
    const title = squash(String(input.title ?? ''))
    if (!title) throw fail('bad_request', 'Пустое название задачи.')
    const project = await findProject(String(input.project ?? '').trim() || inbox)
    const body: Json = { name: clip(title, TITLE_MAX) }
    const desc = String(input.description ?? '').trim()
    if (desc) body.description_html = textToHtml(desc)
    const raw = await viaItems((s) => request('POST', `/projects/${enc(project.id)}/${s}/`, body))
    return normalize(raw, project)
  }

  async function move(key: string, to: Move): Promise<WorkItem> {
    if (!(to in MOVE_GROUPS)) {
      throw fail('bad_request', `Некуда двигать: «${String(to)}». Можно todo, doing, done или cancelled.`)
    }
    const cur = await mustItem(key)
    const target = pickState(await states(cur.projectId), to)
    if (!target) {
      throw fail(
        'bad_request',
        `В проекте ${cur.project} нет статуса для ${MOVE_RU[to]} (группы ${MOVE_GROUPS[to].join(' / ')}).`,
      )
    }
    // уже в нужной группе — свой статус внутри группы не трогаем
    if (cur.stateId === target.id || cur.group === target.group) return cur
    const raw = await viaItems((s) =>
      request('PATCH', `/projects/${enc(cur.projectId)}/${s}/${enc(cur.id)}/`, { state: target.id }),
    )
    return normalize(raw, await projectOf(cur), cur.project)
  }

  async function comment(key: string, text: string): Promise<void> {
    const body = String(text ?? '').trim()
    if (!body) throw fail('bad_request', 'Пустой комментарий.')
    const cur = await mustItem(key)
    await viaItems((s) =>
      request('POST', `/projects/${enc(cur.projectId)}/${s}/${enc(cur.id)}/comments/`, {
        comment_html: textToHtml(body),
      }),
    )
  }

  function refresh(): void {
    projectsCache = null
    statesCache.clear()
  }

  return { projects, states, board, item, add, move, comment, refresh }
}

// ---------- представления для файлов и модели ----------

/** Содержимое tasks.json (контракт v1 из DESIGN.md) */
export function tasksFile(board: Board, now: number): TasksFile {
  const total = board.total ?? {
    todo: board.todo.length,
    doing: board.doing.length,
    doneToday: board.doneToday.length,
  }
  const top = [...board.doing, ...board.todo].slice(0, TOP_LIMIT).map((i) => ({
    id: i.key,
    title: clip(i.title, 100),
    group: i.group,
    project: i.project,
    url: i.url,
  }))
  return {
    v: 1,
    updatedAt: now,
    ok: true,
    error: null,
    counts: { todo: total.todo, doing: total.doing, doneToday: total.doneToday },
    top,
  }
}

const COLUMN_TITLE: Array<[Column | 'cancelled', string]> = [
  ['doing', 'В работе'],
  ['todo', 'Не взято'],
  ['done', 'Готово'],
  ['cancelled', 'Отменено'],
]

function line(i: WorkItem): string {
  const mark = i.priority === 'urgent' ? ' · !срочно' : i.priority === 'high' ? ' · !высокий' : ''
  return `${i.key} · ${i.stateName} · ${clip(i.title, 100)}${mark}`
}

/** Компактный список для модели, по колонкам: «WEB-281 · В работе · Реестр сайтов…» */
export function backlogText(items: WorkItem[], title = 'Задачи Plane'): string {
  if (!items.length) return `${title}: задач нет.`
  const out = [`${title} — ${items.length}`]
  for (const [col, name] of COLUMN_TITLE) {
    const part = items.filter((i) => (columnOf(i.group) ?? 'cancelled') === col).sort(compareItems)
    if (!part.length) continue
    out.push('', `${name} (${part.length}):`, ...part.map(line))
  }
  return out.join('\n')
}

function fmtTime(ms: number, tzOffsetMin: number): string {
  const d = new Date(ms + tzOffsetMin * 60_000).toISOString().slice(0, 16).replace('T', ' ')
  if (!tzOffsetMin) return `${d} UTC`
  const sign = tzOffsetMin > 0 ? '+' : '−'
  const h = Math.floor(Math.abs(tzOffsetMin) / 60)
  const m = Math.abs(tzOffsetMin) % 60
  return `${d} UTC${sign}${h}${m ? ':' + String(m).padStart(2, '0') : ''}`
}

/** Карточка задачи для модели: ключ, статус, проект, ссылка, описание */
export function itemText(item: WorkItem, tzOffsetMin = 0): string {
  const prio = PRIORITY_RU[item.priority] ?? item.priority
  const when = [
    item.updatedAt ? `обновлена ${fmtTime(item.updatedAt, tzOffsetMin)}` : '',
    item.completedAt ? `готова ${fmtTime(item.completedAt, tzOffsetMin)}` : '',
  ]
    .filter(Boolean)
    .join(' · ')
  return [
    `${item.key} · ${item.title}`,
    `Статус: ${item.stateName} (${GROUP_RU[item.group]}) · приоритет: ${prio}`,
    `Проект: ${item.project}${when ? ' · ' + when : ''}`,
    `Ссылка: ${item.url}`,
    '',
    item.description || 'Описания нет.',
  ].join('\n')
}
