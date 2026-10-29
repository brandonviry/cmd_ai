import type { Metadata } from 'next';
import { siteName } from './site';

interface PageMetadataInput {
  title: string;
  description?: string;
  /** Site path of the page, e.g. `/docs/installation` (base path excluded). */
  path: string;
  type?: 'website' | 'article';
}

/** Canonical path with the trailing slash produced by the static export. */
export function canonicalPath(path: string): string {
  if (path === '/' || path === '') return '/';
  return path.endsWith('/') ? path : `${path}/`;
}

/**
 * Per-page metadata: title (completed by the "%s | cmd_ai" template),
 * description, canonical URL and Open Graph. URLs are relative to
 * `metadataBase` (root layout), which already contains the base path.
 */
export function pageMetadata({ title, description, path, type = 'article' }: PageMetadataInput): Metadata {
  const url = canonicalPath(path);
  return {
    title,
    description,
    alternates: { canonical: url },
    openGraph: {
      type,
      locale: 'fr_FR',
      siteName,
      title,
      description,
      url,
    },
    twitter: {
      card: 'summary',
      title,
      description,
    },
  };
}
