# Keeping the kit current

## Keeping the kit current

An agent file that states an old fact does more harm than none. This page separates what the build generates from what people maintain, and gives the checklist to run for every release of the manual or the plugin contract.

## You will need

- The manual sources and the command `bash build.sh` in the manual folder.
- The version of the plugin contract being released.

## Generated and maintained

| Item | Origin | Action on release |
|---|---|---|
| `/llms.txt` | generated from page front matter, navigation and first paragraphs | rebuild |
| `/llms-full.txt` | generated from page bodies | rebuild |
| `/pages/<slug>.md` | generated, one per page | rebuild |
| `/agent-kit/` | copied unchanged from `_agent-kit/` | edit the sources, rebuild |
| Files in `_agent-kit/` | written by hand, stamped `generated-with: manual` | review against the contract |
| Page text and navigation | written by hand | update with the change |

<figure>
<svg viewBox="0 0 520 120" role="img" aria-label="Pages and agent kit sources go through build.sh into llms files and the agent-kit folder">
  <rect x="8" y="20" width="120" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="68" y="43" text-anchor="middle" fill="currentColor" font-size="11">pages/*.md, nav</text>
  <rect x="8" y="68" width="120" height="36" rx="6" fill="none" stroke="currentColor"/>
  <text x="68" y="91" text-anchor="middle" fill="currentColor" font-size="11">_agent-kit/</text>
  <rect x="200" y="40" width="110" height="40" rx="6" fill="none" stroke="currentColor"/>
  <text x="255" y="64" text-anchor="middle" fill="currentColor" font-size="12">bash build.sh</text>
  <rect x="380" y="10" width="132" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="446" y="30" text-anchor="middle" fill="currentColor" font-size="11">llms.txt, llms-full.txt</text>
  <rect x="380" y="45" width="132" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="446" y="65" text-anchor="middle" fill="currentColor" font-size="11">pages/*.md twins</text>
  <rect x="380" y="80" width="132" height="30" rx="6" fill="none" stroke="currentColor"/>
  <text x="446" y="100" text-anchor="middle" fill="currentColor" font-size="11">agent-kit/</text>
  <line x1="128" y1="38" x2="200" y2="55" stroke="currentColor"/>
  <line x1="128" y1="86" x2="200" y2="68" stroke="currentColor"/>
  <line x1="310" y1="60" x2="380" y2="25" stroke="currentColor"/>
  <line x1="310" y1="60" x2="380" y2="60" stroke="currentColor"/>
  <line x1="310" y1="60" x2="380" y2="95" stroke="currentColor"/>
</svg>
<figcaption>Sources on the left are edited. Everything on the right is output and is never edited.</figcaption>
</figure>

## Release checklist

1. Update the pages that the change touches, and add new pages to `_data/nav.yml`.
2. Update the kit files under `_agent-kit/`. Check each fact against [the manual pages](/pages/landing/): command names and flags ([Set up a toolchain](/pages/getting-started-toolchain/)), deadlines and limits ([Errors and limits](/pages/errors-and-limits/)), SDK names ([Rust SDK](/pages/rust-sdk/), [TypeScript SDK](/pages/typescript-sdk/)), manifest keys ([Manifest keys](/pages/reference-manifest/)).
3. Set the `contract:` stamp in every kit file to the released contract version. Search for stale stamps:

<pre class="term"><span class="t-prompt">$</span> rg -L "contract: 0.1" _agent-kit
<span class="t-prompt">$</span> rg -n "contract:" _agent-kit
</pre>

   The first command lists files that lack the current stamp. Every file must carry one.
4. Run the build and read the `result` line. The command exits with a non-zero status on any error.

<pre class="term"><span class="t-prompt">$</span> bash build.sh
</pre>

5. Check the outputs: `llms.txt` lists every new page, `llms-full.txt` ends with the last page, `agent-kit/` holds all files.
6. Check links. Every `/pages/<slug>/` target in the new pages must have a source file `pages/<slug>.md`, and every `.md` address in `llms.txt` must exist under `_site/pages/`.
7. Check the host in the kit files. The files carry the address `http://127.0.0.1:4327`; for a published manual replace it with the public address.
8. Install the kit in a scratch plugin project and ask an agent to add a command. Confirm that it fetches the index and uses the right commands.

## Version stamps

The stamp `contract: 0.1` says the facts were checked against contract 0.1. The manifest `api` key of a package names the exact WIT package version, see [Versioning](/pages/versioning/). Keep both in step: when the manifest version a template generates changes, update the stamps in the same commit.

## See also

- [llms.txt and Markdown pages](/pages/agent-docs-llms-txt/)
- [Project instruction files](/pages/agent-docs-project-files/)
