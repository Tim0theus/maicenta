# MAICENTA branding assets

This directory contains the canonical, versioned visual identity files used by
the repository and desktop clients.

| File | Purpose |
| --- | --- |
| `maicenta-symbol.svg` | Scalable source for the standalone application symbol |
| `maicenta-symbol.png` | Transparent high-resolution raster source |
| `maicenta-wordmark.svg` | Scalable symbol and MAICENTA wordmark |
| `maicenta-banner.png` | Repository and project-page banner |

The Flutter runtime copy at
`apps/client_flutter/assets/branding/maicenta-symbol.png` and the platform icon
sets are derived from the canonical symbol. They are intentionally committed so
normal builds do not require image-conversion tooling.

Temporary design experiments belong in `.work/` or `.exports/`; both folders
are ignored. Published source files and generated application icons remain
tracked.

The project license does not grant rights to the MAICENTA name, logos,
trademarks, or service marks. See the repository README and `LICENSE` for the
applicable terms.
