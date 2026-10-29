import type { Metadata } from 'next';
import { notFound } from 'next/navigation';
import { ContentIndexView, ContentPageView } from '@/components/content-page';
import { pageMetadata } from '@/lib/metadata';
import { bugsRoute } from '@/lib/site';
import { bugSource } from '@/lib/source';

export const dynamicParams = false;

const indexTitle = 'Bugs connus';
const indexDescription = 'Les bugs connus de cmd_ai : une fiche par bug, avec son état et son issue GitHub.';

export default async function Page(props: PageProps<'/bugs/[[...slug]]'>) {
  const { slug = [] } = await props.params;
  const page = bugSource.getPage(slug);
  if (page) return <ContentPageView page={page} root="bug" />;
  if (slug.length === 0) {
    return (
      <ContentIndexView
        title={indexTitle}
        description={indexDescription}
        pages={bugSource.getPages()}
        empty="Aucun bug connu pour l'instant."
      />
    );
  }
  notFound();
}

export function generateStaticParams() {
  const params = bugSource.generateParams();
  return params.some((param) => param.slug.length === 0) ? params : [{ slug: [] }, ...params];
}

export async function generateMetadata(props: PageProps<'/bugs/[[...slug]]'>): Promise<Metadata> {
  const { slug = [] } = await props.params;
  const page = bugSource.getPage(slug);
  if (page) return pageMetadata({ title: page.data.title, description: page.data.description, path: page.url });
  if (slug.length === 0) return pageMetadata({ title: indexTitle, description: indexDescription, path: bugsRoute });
  notFound();
}
