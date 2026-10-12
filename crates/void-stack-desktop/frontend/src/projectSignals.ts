// Live-refresh signals for the selected project.
//
// Views used to load once per project and never notice changes made outside
// the window (a task moved through the MCP server, a commit from the
// terminal, a re-index by the file watcher) — you had to switch module or
// project to see them. The app now polls a cheap fingerprint (a few `stat`s,
// no git process) and bumps a per-channel version when something changed;
// views add the version they care about to their effect deps.

import { createContext, useContext } from 'react'

export type SignalChannel = 'board' | 'git' | 'index' | 'graph'
export type ProjectSignals = Record<SignalChannel, number>

export interface Fingerprint {
  board: string
  git: string
  index: string
  graph: string
}

export const CHANNELS: SignalChannel[] = ['board', 'git', 'index', 'graph']
export const ZERO: ProjectSignals = { board: 0, git: 0, index: 0, graph: 0 }
/** Poll interval while the window is visible. */
export const POLL_MS = 2500

export const SignalsContext = createContext<ProjectSignals>(ZERO)

/** Version counter for one channel: changes whenever that part of the project changed on disk. */
export function useProjectSignal(channel: SignalChannel): number {
  return useContext(SignalsContext)[channel]
}
