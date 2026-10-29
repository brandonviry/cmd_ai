import { DocsLayout } from 'fumadocs-ui/layouts/docs';
import type { ReactNode } from 'react';
import { baseOptions } from '@/lib/layout.shared';
import { bugsRoute } from '@/lib/site';
import { bugSource } from '@/lib/source';

/** bug/README.md (the /bugs index) is listed first, before the sheets. */
function bugTree() {
  const tree = bugSource.getPageTree();
  const isIndex = (node: (typeof tree.children)[number]) => node.type === 'page' && node.url === bugsRoute;
  return {
    ...tree,
    name: 'Bugs',
    children: [...tree.children.filter(isIndex), ...tree.children.filter((node) => !isIndex(node))],
  };
}

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <DocsLayout tree={bugTree()} {...baseOptions()}>
      {children}
    </DocsLayout>
  );
}
