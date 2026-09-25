import { Marked } from 'marked';

const ESCAPES: Record<string, string> = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
  "'": '&#39;',
};

function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (c) => ESCAPES[c] ?? c);
}

const SAFE_URL = /^(https?:|mailto:)/i;

/**
 * Markdown for model output (GFM: tables, lists, code, links).
 *
 * Model output is untrusted: raw HTML is shown as text, only http(s) and
 * mailto links are kept, and remote images are shown as links instead of
 * being loaded.
 */
const markdown = new Marked({
  gfm: true,
  breaks: false,
  renderer: {
    html({ text }) {
      return escapeHtml(text);
    },
    link({ href, title, tokens }) {
      const label = this.parser.parseInline(tokens);
      if (!SAFE_URL.test(href)) return label;
      const titleAttr = title ? ` title="${escapeHtml(title)}"` : '';
      return `<a href="${escapeHtml(href)}"${titleAttr}>${label}</a>`;
    },
    image({ href, text }) {
      const label = escapeHtml(text || href);
      return SAFE_URL.test(href) ? `<a href="${escapeHtml(href)}">${label}</a>` : label;
    },
  },
});

export function renderMarkdown(source: string): string {
  return markdown.parse(source, { async: false });
}
