import { defineCollection, z } from 'astro:content';
import { glob } from 'astro/loaders';

const docs = defineCollection({
  loader: glob({ pattern: '**/*.md', base: '../docs/handbook' }),
  schema: z.object({
    id: z.string(),
    track: z.string(),
    trackOrder: z.number(),
    chapterOrder: z.number(),
    kicker: z.string(),
    title: z.string(),
    subtitle: z.string().optional(),
    readingTime: z.number(),
    inApp: z.boolean().default(true),
    sinceVersion: z.string().default('v1.1.0'),
    aliases: z.array(z.string()).optional(),
  }),
});

export const collections = { docs };
