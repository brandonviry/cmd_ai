import type { StructuredData } from 'fumadocs-core/mdx-plugins';
import { type AdvancedIndex, createSearchAPI } from 'fumadocs-core/search/server';
import { searchTokenizer } from '@/lib/search';
import { type ContentPage, bugSource, source } from '@/lib/source';

// Static search: this route is rendered once at build time into
// out/search-index.json, then downloaded and queried in the browser
// (components/search.tsx). No server is involved after the export.
export const revalidate = false;
export const dynamic = 'force-static';

async function toIndex(page: ContentPage, section: string): Promise<AdvancedIndex> {
  const data = page.data as ContentPage['data'] & {
    structuredData?: StructuredData | (() => StructuredData | Promise<StructuredData>);
  };
  const structuredData =
    typeof data.structuredData === 'function' ? await data.structuredData() : data.structuredData;

  return {
    id: page.url,
    url: page.url,
    title: page.data.title,
    description: page.data.description,
    breadcrumbs: [section],
    structuredData: structuredData ?? { headings: [], contents: [] },
  };
}

/** One index for both collections: the documentation and the bug sheets. */
const search = createSearchAPI('advanced', {
  tokenizer: searchTokenizer,
  indexes: async () =>
    Promise.all([
      ...source.getPages().map((page) => toIndex(page, 'Documentation')),
      ...bugSource.getPages().map((page) => toIndex(page, 'Bugs')),
    ]),
});

export const GET = search.staticGET;
