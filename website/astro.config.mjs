// @ts-check
import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';

// https://astro.build/config
export default defineConfig({
  site: 'https://maicenta.com',
  output: 'static',
  trailingSlash: 'always',
  i18n: {
    defaultLocale: 'en',
    locales: ['en', 'de'],
    routing: {
      prefixDefaultLocale: true,
      // The root `/` is handled by `src/pages/index.astro`, which picks the
      // visitor's browser language and falls back to English.
      redirectToDefaultLocale: false,
    },
  },
  integrations: [
    sitemap({
      // The root page only redirects to a locale and is marked noindex.
      filter: (page) => page !== 'https://maicenta.com/',
      i18n: {
        defaultLocale: 'en',
        locales: { en: 'en', de: 'de' },
      },
    }),
  ],
});
