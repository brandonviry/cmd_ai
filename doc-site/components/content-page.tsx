import { Card, Cards } from 'fumadocs-ui/components/card';
import { DocsBody, DocsDescription, DocsPage, DocsTitle, EditOnGitHub } from 'fumadocs-ui/layouts/docs/page';
import defaultMdxComponents from 'fumadocs-ui/mdx';
import type { AnchorHTMLAttributes } from 'react';
import { getMDXComponents } from '@/components/mdx';
import { resolveContentHref } from '@/lib/links';
import { githubFileUrl, toPosixPath } from '@/lib/site';
import type { ContentPage, ContentRoot } from '@/lib/source';

const Anchor = defaultMdxComponents.a;

/** A Markdown page of `doc/` or `bug/`, rendered with the docs layout. */
export function ContentPageView({ page, root }: { page: ContentPage; root: ContentRoot }) {
  const MDX = page.data.body;
  const from = { root, path: page.path };

  function RelativeLink({ href, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) {
    return <Anchor href={resolveContentHref(href, from)} {...props} />;
  }

  return (
    <DocsPage toc={page.data.toc} full={page.data.full}>
      <DocsTitle>{page.data.title}</DocsTitle>
      {page.data.description ? <DocsDescription>{page.data.description}</DocsDescription> : null}
      <DocsBody>
        <MDX components={getMDXComponents({ a: RelativeLink })} />
      </DocsBody>
      <EditOnGitHub href={githubFileUrl(`${root}/${toPosixPath(page.path)}`)} />
    </DocsPage>
  );
}

interface IndexItem {
  url: string;
  data: { title: string; description?: string };
}

/** Fallback index (folder without index.md / README.md): one card per page. */
export function ContentIndexView({
  title,
  description,
  pages,
  empty,
}: {
  title: string;
  description: string;
  pages: IndexItem[];
  empty: string;
}) {
  return (
    <DocsPage>
      <DocsTitle>{title}</DocsTitle>
      <DocsDescription>{description}</DocsDescription>
      <DocsBody>
        {pages.length > 0 ? (
          <Cards>
            {pages.map((page) => (
              <Card key={page.url} href={page.url} title={page.data.title} description={page.data.description} />
            ))}
          </Cards>
        ) : (
          <p>{empty}</p>
        )}
      </DocsBody>
    </DocsPage>
  );
}
