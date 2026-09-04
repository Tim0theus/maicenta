/**
 * People and companies supporting MAICENTA through Patreon.
 *
 * Add an entry when a member joins and asks to be listed. Only list people
 * who explicitly agreed to be named. Keep the arrays sorted by join date,
 * newest last. `logo` paths point into `public/supporters/`.
 */
export type SupporterTier = 'sponsor' | 'backer' | 'supporter';

export interface Supporter {
  name: string;
  tier: SupporterTier;
  /** Optional website or profile link. */
  url?: string;
  /** Optional logo for sponsors, relative to `public/`, e.g. `/supporters/acme.svg`. */
  logo?: string;
  /** ISO date of joining, used for ordering only. */
  since: string;
}

export const supporters: Supporter[] = [
  // { name: 'Example Corp', tier: 'sponsor', url: 'https://example.com', logo: '/supporters/example.svg', since: '2026-09-01' },
  // { name: 'Jane Doe', tier: 'backer', url: 'https://example.org', since: '2026-09-02' },
  // { name: 'John Doe', tier: 'supporter', since: '2026-09-03' },
];

export function byTier(tier: SupporterTier): Supporter[] {
  return supporters
    .filter((s) => s.tier === tier)
    .sort((a, b) => a.since.localeCompare(b.since));
}
