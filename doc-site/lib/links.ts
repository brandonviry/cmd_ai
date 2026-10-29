import path from 'node:path';
import { type ContentRoot, contentRoots } from './source';
import { githubFileUrl, githubUrl, toPosixPath } from './site';

const posix = path.posix;
const INDEX_FILES = ['index.mdx', 'index.md', 'README.mdx', 'README.md', 'readme.md'];

function findPageUrl(root: ContentRoot, relPath: string): string | undefined {
  const pages = contentRoots[root].getPages();
  const rel = relPath.replace(/\/+$/, '');
  const byPath = (p: string) => pages.find((page) => toPosixPath(page.path) === p)?.url;

  if (/\.mdx?$/i.test(rel)) return byPath(rel);
  for (const name of INDEX_FILES) {
    const url = byPath(rel ? `${rel}/${name}` : name);
    if (url) return url;
  }
  return undefined;
}

/**
 * Resolve a link written in a Markdown file of `doc/` or `bug/` the way GitHub
 * does (relative to the file), so the same Markdown works on GitHub and here:
 *
 * - a link to another page of doc/ or bug/ becomes its site URL (anchor kept);
 * - a link to any other repository file (IDEE.md, specs/, src/…) points to
 *   that file on GitHub;
 * - absolute URLs, `mailto:`, site-absolute paths and `#anchors` are untouched.
 */
export function resolveContentHref(href: string | undefined, from: { root: ContentRoot; path: string }) {
  if (!href) return href;
  if (href.startsWith('#') || href.startsWith('/') || /^[a-z][a-z0-9+.-]*:/i.test(href)) return href;

  const hashIndex = href.indexOf('#');
  const rawPath = hashIndex === -1 ? href : href.slice(0, hashIndex);
  const hash = hashIndex === -1 ? '' : href.slice(hashIndex);
  const queryless = rawPath.split('?', 1)[0] ?? '';

  let decoded = queryless;
  try {
    decoded = decodeURI(queryless);
  } catch {
    // keep the raw path when it is not valid URI encoding
  }

  const fromDir = posix.dirname(posix.join(from.root, toPosixPath(from.path)));
  const target = posix.normalize(posix.join(fromDir, decoded || '.'));
  if (target === '..' || target.startsWith('../')) return githubUrl;

  for (const root of Object.keys(contentRoots) as ContentRoot[]) {
    if (target === root || target.startsWith(`${root}/`)) {
      const url = findPageUrl(root, target === root ? '' : target.slice(root.length + 1));
      if (url) return `${url}${hash}`;
    }
  }

  return target === '.' ? githubUrl : `${githubFileUrl(target)}${hash}`;
}
