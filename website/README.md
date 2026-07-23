# trustmailer.nesgarbo.com

The [trustmailer](https://github.com/nesgarbo/trustmailer) marketing landing page and
bilingual (English/Spanish) documentation site, built with [Astro](https://astro.build)
and [Starlight](https://starlight.astro.build). Live at
[trustmailer.nesgarbo.com](https://trustmailer.nesgarbo.com).

## Structure

```
website/
├── src/
│   ├── pages/index.astro       # the landing page (hero + summary, no code/tables)
│   ├── components/             # landing-page-only components (Hero, Header, Footer, Summary)
│   ├── layouts/Layout.astro    # landing-page HTML shell + meta/OG tags
│   ├── styles/
│   │   ├── global.css          # landing page tokens/typography (rust/teal/gold palette)
│   │   └── starlight-custom.css  # maps the same palette onto Starlight's docs pages
│   ├── assets/logo-mark.svg    # Starlight sidebar logo
│   └── content/docs/
│       ├── docs/               # English docs — routes at /docs/...
│       │   └── quickstart/, transports/
│       └── es/docs/            # Spanish docs — routes at /es/docs/...
│           └── quickstart/, transports/
├── public/                     # favicon, og.png, logo SVGs, robots.txt — served as-is
└── astro.config.mjs            # Starlight config: sidebar, locales, head tags, logo
```

The extra `docs/` folder nested inside `src/content/docs/` is deliberate, not a typo — it
puts every Starlight-generated route under `/docs/...` (and `/es/docs/...` for the
Spanish locale) instead of claiming the site root, which the landing page
(`src/pages/index.astro`) needs to occupy instead.

## Commands

| Command | Action |
| --- | --- |
| `npm install` | Install dependencies |
| `npm run dev` | Start the dev server at `localhost:4321` |
| `npm run build` | Build the production site to `./dist/` |
| `npm run preview` | Preview the build locally before deploying |
| `npm run astro check` | Type-check `.astro` files and validate content frontmatter |
| `npm run deploy` | Build, then `rsync` `dist/` to the production server (see below) |

## Adding or editing a docs page

1. Add the Markdown file under `src/content/docs/docs/<slug>.md` (English) and its
   translation under `src/content/docs/es/docs/<slug>.md` (Spanish) — same relative path
   in both, that's what keeps the language switcher and hreflang alternates correct.
2. Every page needs `title` and `description` frontmatter (Starlight requires the first,
   both matter for SEO — keep `description` between roughly 50 and 160 characters).
3. Add the new page to the `sidebar` array in `astro.config.mjs`, referencing
   `docs/<slug>` (the site-relative slug, without the language prefix — Starlight
   resolves the Spanish equivalent automatically per the active locale).
4. Run `npm run astro check` and `npm run build` before committing — this repo has zero
   tolerance for broken internal links or missing translations; both are meant to be
   mechanically verifiable, not just eyeballed.

## Deploying

`npm run deploy` builds the site and `rsync`s `dist/` to the production server. It reads
its target from a local `.env` file (gitignored — never commit real deploy credentials or
the server's address):

```sh
cp .env.example .env
# then edit .env with the real DEPLOY_USER / DEPLOY_HOST / DEPLOY_PATH
```

## Related

- [`../AUDIT.md`](../AUDIT.md) — the full security/correctness audit this site's
  [Security](https://trustmailer.nesgarbo.com/docs/security/) page summarizes.
- [`../benchmarks/`](../benchmarks) — the real, reproducible comparison scripts behind
  the [nodemailer vs trustmailer](https://trustmailer.nesgarbo.com/docs/comparison/) page.
