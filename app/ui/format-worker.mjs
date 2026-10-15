// Bundled parsers only; worker prevents a long format operation freezing the menus.
import { format } from './vendor/prettier/standalone.mjs';
const parsers = {
  babel: ['babel', 'estree'], typescript: ['typescript', 'estree'],
  html: ['html'], markdown: ['markdown'], css: ['postcss'], yaml: ['yaml'],
};
self.onmessage = async ({ data: { text, parser } }) => {
  try {
    if (!parsers[parser]) throw Error('不支持该格式化语言');
    const plugins = await Promise.all(parsers[parser].map(name => import(`./vendor/prettier/${name}.mjs`)));
    self.postMessage({ text: await format(text, { parser, plugins, tabWidth: 2, endOfLine: 'lf' }) });
  } catch (error) { self.postMessage({ error: error.message }); }
};
