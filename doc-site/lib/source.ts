import { bugs, docs } from '@/.source/server';
import { type InferPageType, loader } from 'fumadocs-core/source';
import { bugsRoute, docsRoute } from './site';

/**
 * `README.md` plays the role of `index.md`: `bug/README.md` is the /bugs
 * index page, and a `README.md` in doc/ would be its folder index.
 */
function readmeAsIndex(_file: unknown, next: () => string[]): string[] | undefined {
  const slugs = next();
  if (slugs.at(-1)?.toLowerCase() === 'readme') return slugs.slice(0, -1);
  return undefined;
}

/** User documentation, read from `<repo>/doc`. */
export const source = loader({
  baseUrl: docsRoute,
  source: docs.toFumadocsSource(),
  slugs: readmeAsIndex,
});

/** Bug sheets, read from `<repo>/bug`. */
export const bugSource = loader({
  baseUrl: bugsRoute,
  source: bugs.toFumadocsSource(),
  slugs: readmeAsIndex,
});

export type DocPage = InferPageType<typeof source>;
export type BugPage = InferPageType<typeof bugSource>;
export type ContentPage = DocPage | BugPage;

/** The two content roots, keyed by their folder name at the repository root. */
export const contentRoots = {
  doc: source,
  bug: bugSource,
} as const;

export type ContentRoot = keyof typeof contentRoots;
