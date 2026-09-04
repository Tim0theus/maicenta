/**
 * Site-wide settings. Everything that is likely to change when the project
 * evolves (URLs, launch state of the paid sync service, contact details) lives
 * here so the page templates never need to be touched for it.
 */
export const site = {
  url: 'https://maicenta.com',
  name: 'MAICENTA',
  github: 'https://github.com/Tim0theus/maicenta',
  releases: 'https://github.com/Tim0theus/maicenta/releases',
  issues: 'https://github.com/Tim0theus/maicenta/issues',
  security: 'https://github.com/Tim0theus/maicenta/blob/main/SECURITY.md',
  license: 'https://github.com/Tim0theus/maicenta/blob/main/LICENSE',
  contactEmail: 'beckmann.timm@gmx.de',
} as const;

/**
 * State of the optional paid sync service ("MAICENTA Sync").
 *
 * The desktop app is and stays free. The sync subscription only stores
 * end-to-end encrypted vault objects. Until the service launches, the pricing
 * page shows the plan as "planned" and hides checkout buttons.
 *
 * Checkout and customer portal URLs are meant to point to a merchant-of-record
 * provider (Paddle, Lemon Squeezy, ...) so VAT, invoices and cancellations are
 * handled there and the website itself needs no backend.
 */
export const sync = {
  available: false,
  /** Monthly price in EUR. `null` shows "price to be announced". */
  priceMonthlyEur: null as number | null,
  /** Yearly price in EUR. `null` hides the yearly option. */
  priceYearlyEur: null as number | null,
  checkoutUrl: null as string | null,
  customerPortalUrl: null as string | null,
} as const;

export type Locale = 'en' | 'de';
export const locales: readonly Locale[] = ['en', 'de'] as const;
export const defaultLocale: Locale = 'en';
