# Writing conventions for the plugin manual

## Adding a page

1. Create `pages/<slug>.md` (lowercase, hyphens; one topic per file).
2. Start it with front matter. All three keys are required:

   ```yaml
   ---
   layout: page
   title: Build and install
   permalink: /pages/build-and-install/
   ---
   ```

3. Write the body in Markdown below the front matter. Start with `##`; the page title comes from `title`.
4. Add an entry to `_data/nav.yml`, in display order:

   ```yaml
   - title: Build and install
     url: /pages/build-and-install/
   ```

5. Link to other pages with their `permalink` (for example `[Build](/pages/build-and-install/)`).
6. Run `./build.sh` and check that it prints a `result` line and exits 0.

## Terminal screenshots

Write a terminal capture as raw HTML. Leave a blank line before and after the block so Markdown does not reformat it.

```html
<pre class="term"><span class="t-prompt">$</span> ilium plugin list
<span class="t-ok">ok</span>   demo-plugin 0.1.0
<span class="t-warn">warn</span> no manifest hooks declared
</pre>
```

Allowed colour classes (spans only, no inline styles): `t-prompt`, `t-ok`, `t-err`, `t-warn`, `t-hi`, `t-dim`. Keep each line at 80 characters or fewer so it fits a 360px phone screen without sideways scrolling inside the block.

## Inline SVG diagrams

Put an SVG directly in the page as raw HTML, inside a `<figure>` with a `<figcaption>`. Use `viewBox` and no fixed `width`, so the diagram scales to the column. Use `currentColor` or the theme classes for strokes and text so the diagram reads in dark and light themes. Give it `role="img"` and an `aria-label`.

```html
<figure>
<svg viewBox="0 0 320 120" role="img" aria-label="Plugin load order">
  <rect x="8" y="30" width="90" height="50" rx="6" fill="none" stroke="currentColor"/>
  <text x="53" y="60" text-anchor="middle" fill="currentColor">Manifest</text>
</svg>
<figcaption>The host reads the manifest before loading the plugin.</figcaption>
</figure>
```

## Callouts

Use `<div class="note">` for extra information and `<div class="warn">` for something that can break a plugin. Put the text in a paragraph inside the div.

## Prose

All text is normal English prose: full sentences, plain words, no emojis, no decorative symbols. Use metric units. Use the same product names as the rest of the manual.

## Raw HTML rule

Markdown passes raw HTML through only when it is a block of its own, separated from the surrounding paragraphs by blank lines. Do not put HTML tags inside a Markdown paragraph.

## One owner page per interface

Every WIT interface has exactly one owner page, and the owner of all type definitions is [reference-wit](/pages/reference-wit/). Other pages hold excerpts only. An excerpt is a `wit` fence whose first line is `// Excerpt of ilium:<pkg>/<iface> in reference-wit`, followed by only the names and cases it needs, in the order of the reference. A page never redefines a type, never changes a case order, and links to the owner page for the full definition. `_tools/check.py` compares every labelled excerpt with the reference and warns about unlabelled `wit` fences and about `host::<module>::<fn>` calls that the reference does not define.

## Requirement rows

A requirement has one owner: the bullet `**[ID]** statement` on its owner page. The row with the same id in `pages/reference-requirements-*.md` repeats that statement word for word, then names the page and how to test it. Change the bullet first, then copy the text into the row. `_tools/check.py` compares the rows of the development tooling table with their bullets and reports every difference. A new requirement takes the next unused id of its prefix: check the highest id in the table before you add one.
