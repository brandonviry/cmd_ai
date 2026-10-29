// Content collections of the documentation site.
//
// The Markdown lives OUTSIDE this app, at the repository root:
//   - ../doc  : the user documentation (single source of truth, French)
//   - ../bug  : one sheet per bug, plus README.md (index) and _modele.md (template)
// Nothing is copied under doc-site/ (constitution: no .md/.mdx content here).
import { defineConfig, defineDocs } from 'fumadocs-mdx/config';
import { metaSchema, pageSchema } from 'fumadocs-core/source/schema';
import type { Root } from 'mdast';
import { z } from 'zod';

/**
 * `doc/` pages MUST carry a non-empty `title` and `description` in their
 * frontmatter (constitution, "Documentation"): the build fails otherwise.
 */
const docPageSchema = pageSchema.extend({
  title: z.string().trim().min(1, 'frontmatter `title` is required'),
  description: z.string().trim().min(1, 'frontmatter `description` is required'),
});

/** First level-1 ATX heading of a Markdown source, frontmatter excluded. */
function firstHeading(source: string): string | undefined {
  const body = source.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n?/, '');
  const match = /^#[ \t]+(.+?)[ \t#]*$/m.exec(body);
  return match?.[1]?.trim() || undefined;
}

/** File name without extension, used as a last-resort title. */
function fileTitle(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? path;
  return name.replace(/\.mdx?$/i, '');
}

/**
 * Bug sheets are written by hand from `bug/_modele.md`: the title falls back
 * to the first `# heading`, then to the file name, so a sheet without
 * frontmatter still renders instead of breaking the whole site.
 */
const bugPageSchema = ({ path, source }: { path: string; source: string }) =>
  pageSchema
    .extend({
      title: z.string().trim().min(1).optional(),
      description: z.string().trim().optional(),
    })
    .transform((data) => ({
      ...data,
      title: data.title ?? firstHeading(source) ?? fileTitle(path),
    }));

export const docs = defineDocs({
  dir: '../doc',
  docs: {
    schema: docPageSchema,
  },
  meta: {
    schema: metaSchema,
  },
});

export const bugs = defineDocs({
  dir: '../bug',
  docs: {
    // `_modele.md` (and any `_*.md`) is a template, not a bug: excluded.
    files: ['**/*.{md,mdx}', '!**/_*.{md,mdx}'],
    schema: bugPageSchema,
  },
  meta: {
    schema: metaSchema,
  },
});

/**
 * The page title is rendered by the layout from the frontmatter; a leading
 * `# Title` kept in the Markdown (useful when reading the file on GitHub) is
 * dropped so it does not appear twice, nor in the table of contents.
 */
function remarkDropLeadingH1() {
  return (tree: Root) => {
    const index = tree.children.findIndex((node) => node.type !== 'yaml' && node.type !== 'html');
    const first = tree.children[index];
    if (first?.type === 'heading' && first.depth === 1) tree.children.splice(index, 1);
  };
}

export default defineConfig({
  mdxOptions: {
    remarkPlugins: (plugins) => [remarkDropLeadingH1, ...plugins],
    // No network access at build time: remote images keep their own size.
    remarkImageOptions: { external: false, onError: 'ignore' },
  },
});
