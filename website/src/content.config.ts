import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";
import { z } from "astro/zod";

const docSchema = z.object({
  title: z.string(),
  description: z.string(),
  order: z.number(),
  lang: z.enum(["en", "fr"]),
});

// One collection per locale, so slugs stay unique within a collection.
const docsEn = defineCollection({
  loader: glob({ pattern: "**/*.md", base: "./src/content/docs/en" }),
  schema: docSchema,
});

const docsFr = defineCollection({
  loader: glob({ pattern: "**/*.md", base: "./src/content/docs/fr" }),
  schema: docSchema,
});

export const collections = { docsEn, docsFr };
