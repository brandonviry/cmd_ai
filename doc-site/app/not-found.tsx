import { HomeLayout } from 'fumadocs-ui/layouts/home';
import type { Metadata } from 'next';
import Link from 'next/link';
import { baseOptions } from '@/lib/layout.shared';
import { docsRoute } from '@/lib/site';

// Next.js already adds <meta name="robots" content="noindex"> to this page.
export const metadata: Metadata = {
  title: 'Page introuvable',
};

/** Exported as out/404.html, which GitHub Pages serves for unknown paths. */
export default function NotFound() {
  return (
    <HomeLayout {...baseOptions()}>
      <main className="flex flex-1 flex-col items-center justify-center gap-4 px-6 py-16 text-center">
        <p className="font-mono text-sm text-fd-muted-foreground">404</p>
        <h1 className="text-2xl font-bold">Page introuvable</h1>
        <p className="max-w-md text-fd-muted-foreground">
          La page que vous cherchez a peut-être été supprimée, renommée, ou est temporairement indisponible.
        </p>
        <Link
          href={docsRoute}
          className="rounded-lg bg-fd-primary px-5 py-2.5 text-sm font-medium text-fd-primary-foreground transition-colors hover:bg-fd-primary/90"
        >
          Retour à la documentation
        </Link>
      </main>
    </HomeLayout>
  );
}
