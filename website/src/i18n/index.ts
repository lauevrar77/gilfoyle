import type { Locale } from "../site";
import { en, type Dict } from "./en";
import { fr } from "./fr";

const DICTS: Record<Locale, Dict> = { en, fr };

/** The full copy for a locale. */
export function t(lang: Locale): Dict {
  return DICTS[lang];
}

export type { Dict };
