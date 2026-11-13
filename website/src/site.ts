/**
 * Single source of truth for everything that is not prose.
 * Change the domain or the repository here and the whole site follows.
 */
export const SITE = {
  /** Canonical origin. Must match `site` in astro.config.mjs. */
  url: "https://gilfoyle.evrard.online",
  name: "Gilfoyle",
  repo: "https://github.com/lauevrar77/gilfoyle",
  releases: "https://github.com/lauevrar77/gilfoyle/releases",
  mcp: "https://modelcontextprotocol.io",
} as const;

export const LOCALES = ["en", "fr"] as const;
export type Locale = (typeof LOCALES)[number];
export const DEFAULT_LOCALE: Locale = "en";

/** Where a locale's landing page lives. */
export function homePath(lang: Locale): string {
  return lang === "en" ? "/" : "/fr/";
}

/** Where a locale's docs index lives. */
export function docsIndexPath(lang: Locale): string {
  return lang === "en" ? "/docs/" : "/fr/docs/";
}

/** Where a given doc lives in a given locale. */
export function docPath(lang: Locale, slug: string): string {
  return lang === "en" ? `/docs/${slug}/` : `/fr/docs/${slug}/`;
}

/** The other locale. */
export function otherLocale(lang: Locale): Locale {
  return lang === "en" ? "fr" : "en";
}
