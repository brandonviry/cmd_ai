import type { Metadata, Viewport } from 'next';
import type { ReactNode } from 'react';
import { Provider } from '@/components/provider';
import { googleSiteVerification, siteDescription, siteName, siteUrl } from '@/lib/site';
import './global.css';

/**
 * Site-wide SEO. `metadataBase` carries the public URL, base path included
 * (e.g. https://brandonviry.github.io/cmd_ai/), so the relative canonical and
 * Open Graph URLs set by each page resolve under it. No canonical here: each
 * page declares its own (lib/metadata.ts).
 */
export const metadata: Metadata = {
  metadataBase: new URL(`${siteUrl}/`),
  title: {
    default: siteName,
    template: `%s | ${siteName}`,
  },
  description: siteDescription,
  applicationName: siteName,
  generator: 'Fumadocs',
  keywords: ['cmd_ai', 'agent', 'terminal', 'PowerShell', 'Bash', 'CMD', 'Ollama', 'clé USB', 'portable'],
  openGraph: {
    type: 'website',
    locale: 'fr_FR',
    siteName,
    title: siteName,
    description: siteDescription,
    url: '/',
  },
  twitter: {
    card: 'summary',
    title: siteName,
    description: siteDescription,
  },
  robots: {
    index: true,
    follow: true,
  },
  // <meta name="google-site-verification"> (Search Console, meta tag method:
  // DNS verification is impossible on github.io).
  ...(googleSiteVerification ? { verification: { google: googleSiteVerification } } : {}),
};

export const viewport: Viewport = {
  themeColor: [
    { media: '(prefers-color-scheme: light)', color: '#ffffff' },
    { media: '(prefers-color-scheme: dark)', color: '#0a0a0a' },
  ],
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="fr" suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        <Provider>{children}</Provider>
      </body>
    </html>
  );
}
