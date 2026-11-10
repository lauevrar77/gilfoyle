// @ts-check
import { defineConfig } from "astro/config";
import sitemap from "@astrojs/sitemap";
import tailwindcss from "@tailwindcss/vite";

// https://astro.build/config
export default defineConfig({
  site: "https://gilfoyle.evrard.online",
  trailingSlash: "ignore",
  // Bind dev and preview servers to every interface (0.0.0.0), not just
  // localhost, so the site can be reached from outside the machine.
  server: { host: true },
  preview: { host: true },
  integrations: [sitemap()],
  vite: {
    plugins: [tailwindcss()],
  },
});
