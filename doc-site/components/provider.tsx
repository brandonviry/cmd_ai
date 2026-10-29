'use client';
import { RootProvider } from 'fumadocs-ui/provider/next';
import type { ReactNode } from 'react';
import StaticSearchDialog from '@/components/search';
import { frTranslations } from '@/lib/translations';

export function Provider({ children }: { children: ReactNode }) {
  return (
    <RootProvider
      search={{ SearchDialog: StaticSearchDialog }}
      i18n={{ locale: 'fr', translations: frTranslations }}
    >
      {children}
    </RootProvider>
  );
}
