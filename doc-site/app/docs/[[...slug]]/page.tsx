import type { Metadata } from 'next';
import { notFound } from 'next/navigation';
import { ContentIndexView, ContentPageView } from '@/components/content-page';
import { pageMetadata } from '@/lib/metadata';
import { docsRoute } from '@/lib/site';
import { source } from '@/lib/source';

export const dynamicParams = false;

const indexTitle = 'Documentation';
const indexDescription = 'Installer, configurer et utiliser cmd_ai : toute la documentation, en français.';

export default async function Page(props: PageProps<'/docs/[[...slug]]'>) {
  const { slug = [] } = await props.params;
  const page = source.getPage(slug);
  if (page) return <ContentPageView page={page} root="doc" />;
  if (slug.length === 0) {
    return (
      <ContentIndexView
        title={indexTitle}
        description={indexDescription}
        pages={source.getPages()}
        empty="La documentation est en cours de rédaction."
      />
    );
  }
  notFound();
}

export function generateStaticParams() {
  const params = source.generateParams();
  return params.some((param) => param.slug.length === 0) ? params : [{ slug: [] }, ...params];
}

export async function generateMetadata(props: PageProps<'/docs/[[...slug]]'>): Promise<Metadata> {
  const { slug = [] } = await props.params;
  const page = source.getPage(slug);
  if (page) return pageMetadata({ title: page.data.title, description: page.data.description, path: page.url });
  if (slug.length === 0) return pageMetadata({ title: indexTitle, description: indexDescription, path: docsRoute });
  notFound();
}
