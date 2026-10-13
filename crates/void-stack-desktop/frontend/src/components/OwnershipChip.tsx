import { useTranslation } from 'react-i18next'
import { ownershipOf } from '../projectTags'

/** "propio" / "tercero" chip for a project; renders nothing when unset. */
export default function OwnershipChip({ tags }: { tags: readonly string[] | undefined }) {
  const { t } = useTranslation()
  const ownership = ownershipOf(tags)
  if (!ownership) return null
  const own = ownership === 'own'
  return (
    <span
      className={`vs-tag ${own ? 'own' : 'third'}`}
      title={t(own ? 'sidebar.tagOwnTitle' : 'sidebar.tagThirdPartyTitle')}
    >
      {t(own ? 'sidebar.tagOwn' : 'sidebar.tagThirdParty')}
    </span>
  )
}
