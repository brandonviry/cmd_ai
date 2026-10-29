import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import { bugsRoute, docsRoute, githubUrl, siteName } from './site';

/** Options shared by the home, docs and bugs layouts. */
export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: (
        <span className="inline-flex items-center gap-2 font-mono font-semibold">
          <span aria-hidden="true" className="text-fd-primary">
            &gt;_
          </span>
          {siteName}
        </span>
      ),
      url: '/',
    },
    githubUrl,
    links: [
      { text: 'Documentation', url: docsRoute, active: 'nested-url' },
      { text: 'Bugs', url: bugsRoute, active: 'nested-url' },
    ],
  };
}
