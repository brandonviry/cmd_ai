// Site-wide constants. Values that depend on where the site is deployed come
// from environment variables read at build time (see next.config.mjs).

export const siteName = 'cmd_ai';

export const siteDescription =
  'Documentation de cmd_ai : un agent en terminal qui comprend vos demandes en français et exécute les commandes PowerShell, Bash ou CMD à votre place, sous Windows, Linux et macOS.';

/** Public URL of the site, base path included, without trailing slash. */
export const siteUrl = (process.env.NEXT_PUBLIC_SITE_URL || 'https://brandonviry.github.io/cmd_ai').replace(
  /\/+$/,
  '',
);

/** Base path the site is served under (e.g. `/cmd_ai` on GitHub Pages), or ''. */
export const basePath = (process.env.NEXT_PUBLIC_BASE_PATH ?? '').replace(/\/+$/, '');

/** Google Search Console token (meta tag verification), when provided. */
export const googleSiteVerification = process.env.NEXT_PUBLIC_GOOGLE_SITE_VERIFICATION?.trim() || undefined;

export const github = {
  owner: 'brandonviry',
  repo: 'cmd_ai',
  branch: 'main',
};

export const githubUrl = `https://github.com/${github.owner}/${github.repo}`;

/** Forward slashes only, whatever the OS the site is built on. */
export function toPosixPath(value: string): string {
  return value.replace(/\\/g, '/');
}

/** GitHub URL of a repository file (`blob`) or folder (`tree`). */
export function githubFileUrl(repoPath: string): string {
  const clean = repoPath.replace(/^\/+/, '');
  const isFile = /\.[A-Za-z0-9]+$/.test(clean.split('/').pop() ?? '');
  return `${githubUrl}/${isFile ? 'blob' : 'tree'}/${github.branch}/${clean}`;
}

export const docsRoute = '/docs';
export const bugsRoute = '/bugs';

/** Static search index, generated at build time (see app/search-index.json). */
export const searchIndexRoute = '/search-index.json';

/** Absolute URL of a site path such as `/docs/installation`, with the trailing slash of the export. */
export function absoluteUrl(path: string): string {
  if (path === '/' || path === '') return `${siteUrl}/`;
  const [pathname, hash] = path.split('#', 2);
  const withSlash = pathname.endsWith('/') ? pathname : `${pathname}/`;
  return `${siteUrl}${withSlash}${hash ? `#${hash}` : ''}`;
}
