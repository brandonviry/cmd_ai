import type { MetadataRoute } from 'next';
import { absoluteUrl, bugsRoute, docsRoute } from '@/lib/site';
import { bugSource, source } from '@/lib/source';

// Rendered once at build time into out/sitemap.xml. Submit
// <site URL>/sitemap.xml by hand in Google Search Console: a robots.txt
// below the host root (e.g. /cmd_ai/robots.txt) is ignored by crawlers.
export const dynamic = 'force-static';

export default function sitemap(): MetadataRoute.Sitemap {
  const entries: { path: string; priority: number }[] = [
    { path: '/', priority: 0.5 },
    { path: docsRoute, priority: 1 },
    ...source.getPages().map((page) => ({ path: page.url, priority: 0.8 })),
    { path: bugsRoute, priority: 0.4 },
    ...bugSource.getPages().map((page) => ({ path: page.url, priority: 0.3 })),
  ];

  const seen = new Set<string>();
  const sitemap: MetadataRoute.Sitemap = [];
  for (const { path, priority } of entries) {
    const url = absoluteUrl(path);
    if (seen.has(url)) continue;
    seen.add(url);
    sitemap.push({ url, priority });
  }
  return sitemap;
}
