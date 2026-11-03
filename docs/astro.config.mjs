// @ts-check
import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";
import starlightLinksValidator from "starlight-links-validator";

/**
 * The Cantina documentation site.
 *
 * Static output only, deployed to Cloudflare Workers as static assets
 * (see wrangler.jsonc). DOCS_SITE_URL overrides the address the site is
 * served from, which canonical links and social previews are built on.
 * CF_WEB_ANALYTICS_TOKEN, when set, adds Cloudflare Web Analytics: no
 * cookies and no personal data, so no consent banner.
 */
const site = process.env.DOCS_SITE_URL || "https://cantina-docs.vibeproject.workers.dev";
const analytics = process.env.CF_WEB_ANALYTICS_TOKEN;

export default defineConfig({
  site,
  output: "static",
  // The landing page imports the skin list from packages/skins.
  vite: { server: { fs: { allow: [".."] } } },
  integrations: [
    starlight({
      title: "Cantina",
      description:
        "A Mac desktop studio for guitar and bass players: pedals, tracks, drums, plugins and a stage of visualizer scenes.",
      logo: { src: "./src/assets/logo.svg", alt: "" },
      favicon: "/favicon.svg",
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/jbouder/cantina" }],
      editLink: { baseUrl: "https://github.com/jbouder/cantina/edit/main/docs/" },
      lastUpdated: true,
      customCss: [
        "@fontsource/vt323/400.css",
        "./src/generated/theme.css",
        "./src/styles/cantina.css",
      ],
      head: [
        { tag: "meta", attrs: { property: "og:image", content: `${site}/og.png` } },
        { tag: "meta", attrs: { property: "og:image:width", content: "1200" } },
        { tag: "meta", attrs: { property: "og:image:height", content: "630" } },
        { tag: "meta", attrs: { name: "twitter:image", content: `${site}/og.png` } },
        ...(analytics
          ? [
              {
                tag: /** @type {const} */ ("script"),
                attrs: {
                  defer: true,
                  src: "https://static.cloudflareinsights.com/beacon.min.js",
                  "data-cf-beacon": JSON.stringify({ token: analytics }),
                },
              },
            ]
          : []),
      ],
      // Fails the build on a broken internal link or heading anchor.
      plugins: [starlightLinksValidator({ errorOnRelativeLinks: false })],
      sidebar: [
        {
          label: "Start here",
          items: [{ label: "Getting started", slug: "guides/getting-started" }],
        },
        { label: "Design notes", items: [{ autogenerate: { directory: "notes" } }] },
      ],
    }),
  ],
});
