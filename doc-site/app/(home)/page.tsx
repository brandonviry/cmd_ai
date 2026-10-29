import type { Metadata } from 'next';
import Link from 'next/link';
import { pageMetadata } from '@/lib/metadata';
import { bugsRoute, docsRoute, githubUrl, siteDescription, siteName } from '@/lib/site';

export const metadata: Metadata = {
  ...pageMetadata({ title: siteName, description: siteDescription, path: '/', type: 'website' }),
  // The home page carries the bare site name, not "cmd_ai | cmd_ai".
  title: { absolute: `${siteName} — documentation` },
};

const highlights = [
  {
    title: 'Zéro commande à taper',
    text: 'Demandez en français ; cmd_ai écrit et exécute la commande PowerShell, Bash ou CMD adaptée à la machine.',
  },
  {
    title: 'Vous gardez la main',
    text: 'Mode plan par défaut : vous validez la suite de commandes avant qu’elle tourne. Le mode bypass est un choix explicite.',
  },
  {
    title: 'Portable',
    text: 'Un dossier sur une clé USB, avec sa configuration, ses logs et sa mémoire : rien n’est écrit sur la machine hôte.',
  },
];

/**
 * Landing page. The static export cannot redirect on the server, so "/"
 * links to the documentation instead.
 */
export default function HomePage() {
  return (
    <main className="flex flex-1 flex-col items-center justify-center gap-10 px-6 py-16 text-center">
      <div className="flex max-w-2xl flex-col items-center gap-4">
        <p className="font-mono text-sm text-fd-muted-foreground" aria-hidden="true">
          &gt;_ {siteName}
        </p>
        <h1 className="text-3xl font-bold tracking-tight sm:text-4xl">Ne tapez plus jamais une commande.</h1>
        <p className="text-fd-muted-foreground sm:text-lg">{siteDescription}</p>
      </div>

      <div className="flex flex-wrap items-center justify-center gap-3">
        <Link
          href={docsRoute}
          className="rounded-lg bg-fd-primary px-5 py-2.5 text-sm font-medium text-fd-primary-foreground transition-colors hover:bg-fd-primary/90"
        >
          Lire la documentation
        </Link>
        <Link
          href={bugsRoute}
          className="rounded-lg border border-fd-border px-5 py-2.5 text-sm font-medium transition-colors hover:bg-fd-accent hover:text-fd-accent-foreground"
        >
          Bugs connus
        </Link>
        <a
          href={githubUrl}
          className="rounded-lg border border-fd-border px-5 py-2.5 text-sm font-medium transition-colors hover:bg-fd-accent hover:text-fd-accent-foreground"
        >
          Code source sur GitHub
        </a>
      </div>

      <ul className="grid w-full max-w-4xl gap-4 text-left sm:grid-cols-3">
        {highlights.map((item) => (
          <li key={item.title} className="rounded-xl border border-fd-border bg-fd-card p-5 text-fd-card-foreground">
            <h2 className="mb-2 font-semibold">{item.title}</h2>
            <p className="text-sm text-fd-muted-foreground">{item.text}</p>
          </li>
        ))}
      </ul>
    </main>
  );
}
