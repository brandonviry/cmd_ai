'use client';
import {
  SearchDialog,
  SearchDialogClose,
  SearchDialogContent,
  SearchDialogFooter,
  SearchDialogHeader,
  SearchDialogIcon,
  SearchDialogInput,
  SearchDialogList,
  SearchDialogOverlay,
  type SharedProps,
} from 'fumadocs-ui/components/dialog/search';
import { useStaticSearch } from 'fumadocs-core/search/client';
import { create } from 'zbsearch';
import { searchTokenizer } from '@/lib/search';
import { basePath, searchIndexRoute } from '@/lib/site';

/** The exported index is a plain file: its URL must carry the base path. */
const indexUrl = `${basePath}${searchIndexRoute}`;

function initDB() {
  return create({
    schema: { _: 'string' },
    components: { tokenizer: searchTokenizer },
  });
}

/** Search dialog backed by the static index, queried in the browser. */
export default function StaticSearchDialog(props: SharedProps) {
  const search = useStaticSearch({ from: indexUrl, initDB });

  return (
    <SearchDialog {...search} {...props}>
      <SearchDialogOverlay />
      <SearchDialogContent>
        <SearchDialogHeader>
          <SearchDialogIcon />
          <SearchDialogInput />
          <SearchDialogClose />
        </SearchDialogHeader>
        <SearchDialogList />
        <SearchDialogFooter>
          <p className="text-xs text-fd-muted-foreground px-1">Recherche hors ligne, dans la documentation et les fiches de bugs.</p>
        </SearchDialogFooter>
      </SearchDialogContent>
    </SearchDialog>
  );
}
