// Remembers the selected project across launches and the most recently
// used ones, so the picker can list them first. Storage can be blocked
// (private mode, cleared site data) — every access degrades to "no memory".

const LAST_KEY = 'void-last-project'
const RECENT_KEY = 'void-recent-projects'
const MAX_RECENT = 5

export function getLastProject(): string | null {
  try {
    return localStorage.getItem(LAST_KEY)
  } catch {
    return null
  }
}

export function getRecentProjects(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_KEY)
    const parsed: unknown = raw ? JSON.parse(raw) : []
    return Array.isArray(parsed) ? parsed.filter((n): n is string => typeof n === 'string') : []
  } catch {
    return []
  }
}

/** Record `name` as the current project and move it to the front of the recents. */
export function rememberProject(name: string): void {
  try {
    localStorage.setItem(LAST_KEY, name)
    const recent = [name, ...getRecentProjects().filter(n => n !== name)].slice(0, MAX_RECENT)
    localStorage.setItem(RECENT_KEY, JSON.stringify(recent))
  } catch {
    // Storage unavailable: selection still works for this session.
  }
}

/** Rename/remove bookkeeping so stale names don't linger in the recents. */
export function forgetProject(name: string, renamedTo?: string): void {
  try {
    const recent = getRecentProjects()
      .map(n => (n === name ? renamedTo ?? null : n))
      .filter((n): n is string => !!n)
    localStorage.setItem(RECENT_KEY, JSON.stringify([...new Set(recent)]))
    if (localStorage.getItem(LAST_KEY) === name) {
      if (renamedTo) localStorage.setItem(LAST_KEY, renamedTo)
      else localStorage.removeItem(LAST_KEY)
    }
  } catch {
    // Storage unavailable — nothing to clean up.
  }
}

/**
 * Picker order: the selected project first, then recently used ones (most
 * recent first), then the rest in config order.
 */
export function orderProjects<T extends { name: string }>(
  projects: T[],
  selected: string | null,
  recent: string[],
): T[] {
  const rank = (name: string): number => {
    if (name === selected) return -1
    const i = recent.indexOf(name)
    return i === -1 ? recent.length : i
  }
  return projects
    .map((p, i) => ({ p, i }))
    .sort((a, b) => rank(a.p.name) - rank(b.p.name) || a.i - b.i)
    .map(x => x.p)
}
