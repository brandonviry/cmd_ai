import { language, stemmer } from '@zbsearch/stemmers/french';
import type { DefaultTokenizerConfig } from 'zbsearch';

/**
 * Tokenizer shared by the index (built at build time) and the browser
 * (queries): French word splitting and stemming, so that "installer" finds
 * "installation". Both sides MUST use the same configuration.
 */
export const searchTokenizer: DefaultTokenizerConfig = {
  language: language as DefaultTokenizerConfig['language'],
  stemming: true,
  stemmer,
};
