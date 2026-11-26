import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'

import type { BoardView, Row, Shared } from '../types'

// The mod is invisible (owner, 17.11.2026): claudibeat shows the sessions,
// limits and tasks. Each drawing site must hand the engine's own tree through
// untouched, with as much data at hand as there ever is.

const NOW = 1_762_176_000_000

const row = (key: string, over: Partial<Row> = {}): Row => ({
  key,
  desktopId: key,
  title: `Сессия ${key}`,
  project: 'my-app',
  cwd: 'D:\\Code\\Projects\\my-app',
  link: `claude://claude.ai/epitaxy/${key}`,
  state: 'working',
  waitingReason: null,
  ask: null,
  since: NOW - 60_000,
  doing: 'читает app.ts',
  contextPercent: 38,
  agents: [],
  isSelf: false,
  isLive: true,
  needsYou: false,
  lastActivityAt: NOW - 1_000,
  ...over,
})

const SHARED: Shared = {
  rows: [
    row('q', { state: 'waiting', waitingReason: 'question', ask: 'Какой вариант?', needsYou: true, title: 'Shop backend' }),
    row('d', { state: 'done', needsYou: true, title: 'Typst отчёт' }),
    row('me', { isSelf: true, title: 'Панель статуса' }),
  ],
  limits: [
    { window: 'five_hour', percent: 41, resetsAt: NOW + 130 * 60_000 },
    { window: 'seven_day', percent: 18, resetsAt: null },
  ],
  tasks: null,
  readAt: Math.floor(NOW / 60_000),
}

const BOARD: BoardView = {
  todo: [{ key: 'WEB-281', title: 'Реестр сайтов: фильтр', project: 'WEB', group: 'unstarted', stateName: 'Не взято', priority: 'high', description: 'Фильтр по статусу', url: 'https://plane.example/web/browse/WEB-281/' }],
  doing: [],
  doneToday: [],
  projects: ['WEB', 'SEO'],
  fetchedAt: NOW - 30_000,
  error: null,
}

function seed(on: On) {
  on('state.get', (_, e) => {
    const key = (e as { key: string }).key
    if (key === 'shared') return { value: { value: SHARED, version: 1 } }
    if (key === 'board') return { value: { value: BOARD, version: 1 } }
    return { value: { value: null, version: 1 } }
  })
  on('clock.now', () => ({ value: NOW }))
  // An answered timer that never fires: the beat stays asleep, no env or fs is asked.
  on('clock.after', () => ({ value: { cancel: () => undefined } as unknown as void }))
  // Beneath the plugin this hook stands for the engine: one marked text, the modes it is handed.
  on('ui.render', ($, e) => {
    const { Text } = $.ui.resolve(e)
    const modes = (e.props as { modes?: readonly string[] }).modes
    return <Text key="engine">{modes ? modes.join(' & ') : 'engine'}</Text>
  })
}

/** The first mount boots the engine: a few seconds on a cold machine. */
const SLOW = { timeoutMs: 20_000 }

const SITES = [
  { plugin: 'status-band', component: 'AbovePrompt', props: { hasSurvey: false, isWorking: false, maxRows: 14, bodyColumns: 140, scroll: { offset: 0, bodyRows: 13 }, view: {} } },
  { plugin: 'status-band', component: 'SessionMode', props: { modes: ['Bypass permissions'] } },
  { plugin: 'status-band', component: 'PromptHint', props: { isDraft: false, isWorking: false, hint: '' } },
] as const

for (const site of SITES) {
  for (const surface of ['desktop', 'terminal'] as const) {
    test(`${site.component} on the ${surface}: the engine's own tree, nothing of ours`, SLOW, async ($, on) => {
      seed(on)
      const ui = await $.ui.mount({ ...site, surface } as never)
      const texts = await ui.findAll({ type: 'Text' })
      expect(texts).toHaveLength(1)
      expect(texts[0]?.text).toBe(site.component === 'SessionMode' ? 'Bypass permissions' : 'engine')
      expect(await ui.find({ type: 'Button' })).toBeFalsy()
      expect(await ui.find({ type: 'Input' })).toBeFalsy()
      await ui.unmount()
    })
  }
}
