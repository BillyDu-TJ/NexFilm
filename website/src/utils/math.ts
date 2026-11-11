import katex from 'katex';

export interface MathRenderOptions {
  displayMode?: boolean;
  throwOnError?: boolean;
}

/**
 * Render a TeX string to KaTeX static HTML string at build time.
 * Defaults to throwOnError: true to fail fast on typos during SSG.
 */
export function renderMath(tex: string, options: MathRenderOptions = {}): string {
  const { displayMode = false, throwOnError = true } = options;
  return katex.renderToString(tex, {
    displayMode,
    throwOnError,
    output: 'htmlAndMathml',
  });
}

/**
 * Render inline TeX formula.
 */
export function renderInlineMath(tex: string): string {
  return renderMath(tex, { displayMode: false, throwOnError: true });
}

/**
 * Render block / display TeX formula.
 */
export function renderBlockMath(tex: string): string {
  return renderMath(tex, { displayMode: true, throwOnError: true });
}

/**
 * Scan an HTML / markdown text string, find any $$...$$ (block) and $...$ (inline)
 * TeX expressions, and replace them with static KaTeX HTML.
 */
export function renderMathInHtml(content: string): string {
  if (!content || !content.includes('$')) {
    return content;
  }

  // First replace block math $$...$$
  let result = content.replace(/\$\$([\s\S]+?)\$\$/g, (_, tex) => {
    return renderBlockMath(tex.trim());
  });

  // Then replace inline math $...$
  result = result.replace(/\$([^\$\n]+?)\$/g, (_, tex) => {
    return renderInlineMath(tex.trim());
  });

  return result;
}
