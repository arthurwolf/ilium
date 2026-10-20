# llms.txt and Markdown pages

## llms.txt and Markdown pages

The llms.txt proposal (llmstxt.org) gives a site a Markdown file at a known address that an agent reads to find the pages worth fetching. This site follows it. The build writes `/llms.txt`, `/llms-full.txt`, a `.md` copy of every page, and the folder `/agent-kit/`, all from the page sources. Nobody edits those outputs by hand.

## You will need

- The built site, or the running copy. The files are static and need no server feature.
- For adding a page: the page conventions in the manual folder (`CONVENTIONS.md`) and an entry in `_data/nav.yml`.

## The standard

The llmstxt.org format is a Markdown file in this order. The H1 with the site name is the only required part.

| Part | Rule |
|---|---|
| H1 | The project or site name. Required. |
| Blockquote | A short summary with the context needed to read the rest. Optional. |
| Free text | Paragraphs or lists without headings. Optional. |
| H2 sections | Each holds a list of `[name](url)` links, each optionally followed by `: notes`. Optional. |
| H2 named `Optional` | Secondary links an agent can skip when it needs a shorter context. |

The file lives at the site root, or at a subpath that it covers; the most specific file wins. A page can also be served as Markdown at the same address with `.md` appended or substituted. `llms-full.txt` is a common extension, not part of the proposal: the whole documentation in one file.

## What this site publishes

| Address | Content | Made by |
|---|---|---|
| `/llms.txt` | The index: H1, summary, one H2 per navigation group, one link per page with a description | `write_llms_files` in the build |
| `/llms-full.txt` | Every page body in navigation order, each after a `<!-- source: URL -->` line | the same function |
| `/pages/<slug>.md` | The page body as Markdown under an H1 title, for every page | the build, one per page |
| `/agent-kit/` | The files in `_agent-kit/`, copied unchanged | the build |

This is the start of the real index, as the build produced it:

<pre class="term"># Ilium Plugin Developer Manual

&gt; Developer manual for Ilium plugins: WebAssembly components (wasm32-wasip2)
&gt; written in Rust or TypeScript that add tree decorations, commands, ...

Every page below is also served as plain Markdown at the same address with
`.md` instead of the trailing slash. `llms-full.txt` holds the whole manual
in one file. Ready-made agent instruction files for plugin projects are
under `agent-kit/`.

## Start here

- [Plugin manual](http://127.0.0.1:4327/pages/landing.md): A plugin adds
  features to Ilium. It can decorate tree rows with badges, add commands ...
- [What a plugin is](http://127.0.0.1:4327/pages/getting-started-overview.md):
  Ilium splits its work into two parts. The kernel ...
</pre>

The excerpt is wrapped and shortened for this page; in the file each link entry is one line. The link host comes from `url` in `_config.yml`. The index follows the standard: H1, blockquote, a free-text paragraph, then H2 sections of links. The H2 names are the navigation groups, so the order of the index is the order of the navigation.

## Add a page so it appears

1. Create `pages/<slug>.md` with `layout`, `title` and `permalink` front matter.
2. Add an entry with the page title and permalink to `_data/nav.yml`. A page that is not in the navigation is not in the index.
3. Choose the slug prefix of its group (`api-`, `capability-`, `widgets-`, `agent-docs` and so on). The build groups pages by prefix.
4. Make the first body paragraph a plain sentence. The index description is the first prose paragraph, shortened to 220 characters; it skips headings, tables, lists, quotes and HTML.
5. Run `bash build.sh` and check the `result` line. Then open `/llms.txt` and find the page.

## Point an agent at the manual

Put the index address in the instruction file of the plugin project; the kit templates already do. For a one-off session, give the agent the address in the prompt.

<pre class="term"><span class="t-prompt">$</span> curl -s http://127.0.0.1:4327/llms.txt | head -3
# Ilium Plugin Developer Manual

&gt; Developer manual for Ilium plugins: WebAssembly components (wasm32-wasip2)
<span class="t-prompt">$</span> curl -s http://127.0.0.1:4327/pages/events-and-intents.md | head -3
# Events and intents

## Events and intents
</pre>

<div class="note">
<p>The twin is the page body without front matter, with the page title added as an H1. Raw HTML blocks such as diagrams and terminal captures stay in the text.</p>
</div>

## See also

- [Providing the docs to agents](/pages/agent-docs/)
- [Keeping the kit current](/pages/agent-docs-maintenance/)
