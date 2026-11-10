// Rasterise the social cards from SVG to PNG.
//
// Twitter, Facebook and most unfurlers do not accept SVG for og:image, so the
// build emits a PNG next to each SVG source.

import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const langs = ["en", "fr"];

for (const lang of langs) {
  const svg = await readFile(join(root, "public", `og-${lang}.svg`));
  await sharp(svg, { density: 150 })
    .resize(1200, 630)
    .png()
    .toFile(join(root, "public", `og-${lang}.png`));
  process.stdout.write(`[gilfoyle] wrote public/og-${lang}.png\n`);
}
