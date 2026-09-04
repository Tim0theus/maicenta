# maicenta.com

Static marketing and documentation site for MAICENTA, built with
[Astro](https://astro.build). No backend, no cookies, no third-party scripts.

## Structure

```text
src/
  config.ts              Site URLs, contact address, launch state of MAICENTA Sync
  i18n/en.ts, de.ts      All UI copy, typed so both languages stay in sync
  layouts/Base.astro     Head, hreflang alternates, header and footer
  pages/index.astro      Root: picks the browser language, falls back to /en/
  pages/[lang]/          One template per page, rendered for every locale
  content/docs/<lang>/   Documentation articles (Markdown)
  content/legal/<lang>/  Imprint and privacy policy (Markdown)
public/
  branding/              Copies of assets/branding used by the site
  _headers               Security headers for Cloudflare Pages
```

Routes are `/en/...` and `/de/...`. Keep documentation slugs identical in both
language folders so the language switcher can swap the prefix.

## Development

```sh
npm install
npm run dev      # http://localhost:4321
npm run check    # type-check and validate content frontmatter
npm run build    # writes dist/
```

Requires Node 22 or newer.

## Adding a language

1. Add the locale to `locales` in `src/config.ts` and `astro.config.mjs`.
2. Create `src/i18n/<lang>.ts` typed as `Translations` and register it in
   `src/i18n/index.ts`.
3. Add `src/content/docs/<lang>/` and `src/content/legal/<lang>/` with the same
   file names as the English folder.

## Turning on MAICENTA Sync

Set `available`, prices and the checkout and customer-portal URLs in `sync` in
`src/config.ts`. The pricing page switches from "planned" to live buttons.

## Deployment

`.github/workflows/website.yml` builds on every change under `website/` and
deploys `main` to Cloudflare Pages. It needs the repository secrets
`CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` and a Pages project named
`maicenta`. Point the `maicenta.com` custom domain at that project in the
Cloudflare dashboard.

## Before going live

- Fill in the placeholders in `src/content/legal/*/imprint.md` and `privacy.md`.
- Set the real contact address in `src/config.ts`.
- Confirm the hosting provider paragraph in the privacy policy matches the
  actual deployment.

## License

Code in this directory is MPL 2.0 like the rest of the repository. Texts,
documentation and brand assets are reserved, see [LICENSE.md](LICENSE.md).
