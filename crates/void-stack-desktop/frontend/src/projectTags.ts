// Project tags as stored by the core registry (see
// void-stack-core/src/global_config/tags.rs): canonical, lowercase, and the
// ownership pair spelled in English whatever the UI language.

export const TAG_OWN = 'own'
export const TAG_THIRD_PARTY = 'third-party'

export type Ownership = 'own' | 'third-party' | null

export function ownershipOf(tags: readonly string[] | undefined): Ownership {
  if (tags?.includes(TAG_THIRD_PARTY)) return TAG_THIRD_PARTY
  if (tags?.includes(TAG_OWN)) return TAG_OWN
  return null
}

/** Tags other than the ownership pair. */
export function labelTags(tags: readonly string[] | undefined): string[] {
  return (tags ?? []).filter(t => t !== TAG_OWN && t !== TAG_THIRD_PARTY)
}

/** "rust, mql4" → ['rust', 'mql4'] (the backend canonicalizes further). */
export function parseTagText(text: string): string[] {
  return text.split(',').map(t => t.trim()).filter(Boolean)
}

/** Full tag list from an ownership choice plus free labels. */
export function composeTags(ownership: Ownership, labels: string[]): string[] {
  return ownership ? [ownership, ...labels] : labels
}

export function sameTags(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((t, i) => t === b[i])
}
