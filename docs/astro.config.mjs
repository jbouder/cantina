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
  // Where the design notes lived before they became guides and references.
  redirects: {
    "/notes/designer": "/guides/designer/",
    "/notes/drums": "/guides/drums/",
    "/notes/latency": "/guides/latency/",
    "/notes/pedal-format": "/reference/pedal-format/",
    "/notes/pedals": "/guides/pedals/",
    "/notes/skins": "/guides/skins/",
    "/notes/stage": "/guides/stage/",
    "/notes/timeline": "/guides/recording/",
  },
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
      // A light / dark / system switch, dark by default as the app is.
      components: {
        ThemeProvider: "./src/components/ThemeProvider.astro",
        ThemeSelect: "./src/components/ThemeSelect.astro",
      },
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
      // The rustdoc under /api/ and the JSON schemas under /schemas/ are
      // files, not Starlight pages.
      plugins: [
        starlightLinksValidator({
          errorOnRelativeLinks: false,
          exclude: ["/api/**", "/schemas/**"],
        }),
      ],
      sidebar: [
        {
          label: "Start here",
          items: [
            { label: "Getting started", slug: "guides/getting-started" },
            { label: "Troubleshooting", slug: "guides/troubleshooting" },
            { label: "Latency", slug: "guides/latency" },
          ],
        },
        {
          label: "Guides",
          items: [
            { label: "Recording and editing", slug: "guides/recording" },
            { label: "Exporting and bouncing", slug: "guides/mixdown" },
            { label: "Pedals", slug: "guides/pedals" },
            { label: "Designing pedals", slug: "guides/designer" },
            { label: "Drums", slug: "guides/drums" },
            { label: "Tabs", slug: "guides/tabs" },
            { label: "The stage", slug: "guides/stage" },
            { label: "Skins", slug: "guides/skins" },
            { label: "Authoring with an agent", slug: "guides/agents" },
            { label: "The assistant", slug: "guides/assistant" },
          ],
        },
        { label: "Reference", items: [{ autogenerate: { directory: "reference" } }] },
        { label: "Architecture", items: [{ autogenerate: { directory: "architecture" } }] },
        { label: "API reference", slug: "api" },
      ],
    }),
  ],
});
