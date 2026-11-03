import { defineCollection, z } from "astro:content";
import { docsLoader } from "@astrojs/starlight/loaders";
import { docsSchema } from "@astrojs/starlight/schema";

export const collections = {
  docs: defineCollection({
    loader: docsLoader(),
    // Every page needs a description: it is the search snippet and the
    // social preview text, so the build fails without one.
    schema: docsSchema({ extend: z.object({ description: z.string().min(20) }) }),
  }),
};
