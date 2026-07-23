// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

// https://astro.build/config
export default defineConfig({
  site: "https://trustmailer.nesgarbo.com",
  integrations: [
    starlight({
      title: "trustmailer",
      logo: {
        src: "./src/assets/logo-mark.svg",
        replacesTitle: false,
      },
      favicon: "/favicon.svg",
      social: [
        { icon: "github", label: "GitHub", href: "https://github.com/nesgarbo/trustmailer" },
      ],
      head: [
        {
          tag: "meta",
          attrs: { property: "og:image", content: "https://trustmailer.nesgarbo.com/og.png" },
        },
        {
          tag: "meta",
          attrs: { property: "og:image:width", content: "1200" },
        },
        {
          tag: "meta",
          attrs: { property: "og:image:height", content: "630" },
        },
        {
          tag: "meta",
          attrs: { name: "twitter:image", content: "https://trustmailer.nesgarbo.com/og.png" },
        },
      ],
      customCss: ["./src/styles/starlight-custom.css"],
      editLink: {
        baseUrl: "https://github.com/nesgarbo/trustmailer/edit/main/website/",
      },
      defaultLocale: "root",
      locales: {
        root: { label: "English", lang: "en" },
        es: { label: "Español", lang: "es" },
      },
      sidebar: [
        {
          label: "Start here",
          translations: { es: "Empieza aquí" },
          items: [
            { slug: "docs" },
            { slug: "docs/why" },
            { slug: "docs/architecture" },
            { slug: "docs/comparison" },
          ],
        },
        {
          label: "Quickstart",
          translations: { es: "Guía rápida" },
          items: [
            { slug: "docs/quickstart/native-rust" },
            { slug: "docs/quickstart/nodejs" },
            { slug: "docs/quickstart/cloudflare-workers" },
            { slug: "docs/quickstart/browser-wasm" },
          ],
        },
        {
          label: "Guides",
          translations: { es: "Guías" },
          items: [
            { slug: "docs/message" },
            { slug: "docs/addresses" },
            { slug: "docs/attachments" },
            { slug: "docs/dkim" },
            { slug: "docs/gmail" },
          ],
        },
        {
          label: "Transports",
          translations: { es: "Transportes" },
          items: [
            { slug: "docs/transports/smtp" },
            { slug: "docs/transports/sendmail" },
            { slug: "docs/transports/ses" },
            { slug: "docs/transports/stream" },
            { slug: "docs/transports/json" },
            { slug: "docs/transports/cloudflare-workers" },
            { slug: "docs/transports/browser-wasm" },
          ],
        },
        {
          label: "Reference",
          translations: { es: "Referencia" },
          items: [
            { slug: "docs/runtime-matrix" },
            { slug: "docs/security" },
            { slug: "docs/errors" },
            { slug: "docs/building" },
            { slug: "docs/license" },
          ],
        },
      ],
    }),
  ],
});
