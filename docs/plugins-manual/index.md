---
layout: default
title: Ilium Plugin Developer Manual
permalink: /
---
<article class="page">
<p class="crumbs"><b>Ilium plugins</b></p>
<div class="hero">
<h1>Build on Ilium without touching Ilium</h1>
<p class="lead">A plugin is a sandboxed WebAssembly component. It reads events from the session tree, answers with intents, and draws with the same widgets the Ilium interface uses. Write it in Rust or TypeScript, sign it, ship one <code>.iliumpkg</code> file.</p>
<pre class="term"><span class="t-prompt">$</span> ilium plugin new agent-badge --language rust
<span class="t-ok">ok</span>   created agent-badge/  (manifest.json, src/lib.rs, wit/)
<span class="t-prompt">$</span> ilium plugin pack ./agent-badge
<span class="t-ok">ok</span>   agent-badge.iliumpkg  (unsigned, 118 KiB)
<span class="t-prompt">$</span> ilium plugin install agent-badge.iliumpkg
<span class="t-hi">grant?</span> agent.read, tree.decorate  <span class="t-dim">[y/N]</span> y
<span class="t-ok">ok</span>   agent-badge 0.1.0 running  <span class="t-dim">(server side, sandboxed)</span>
</pre>
</div>

<h2>Start with a tutorial</h2>
<div class="cards">
<a class="card" href="/pages/getting-started-first-plugin/"><b>Your first plugin in five minutes</b><span>Template, build, install, see the result.</span><em>Start here</em></a>
<a class="card" href="/pages/tutorial-01-agent-badge/"><b>1. Agent badge</b><span>Events in, intents out: badge every agent pane.</span><em>Tutorial</em></a>
<a class="card" href="/pages/tutorial-03-panel/"><b>3. A panel with a table</b><span>Draw with the retained-mode widget tree.</span><em>Tutorial</em></a>
<a class="card" href="/pages/tutorial-05-todo-scanner/"><b>5. TODO scanner</b><span>Scoped file access and background jobs.</span><em>Tutorial</em></a>
<a class="card" href="/pages/tutorial-07-ambient-scene/"><b>7. Ambient scene</b><span>A background animation that follows the shared look.</span><em>Tutorial</em></a>
<a class="card" href="/pages/tutorial-08-typescript/"><b>8. The same plugin in TypeScript</b><span>Same contract, different language.</span><em>Tutorial</em></a>
</div>

<h2>Understand the model</h2>
<div class="cards">
<a class="card" href="/pages/architecture/"><b>Architecture</b><span>What stays native, what becomes a plugin.</span><em>Concept</em></a>
<a class="card" href="/pages/capabilities/"><b>Capabilities and grants</b><span>What a plugin may do, asked per capability.</span><em>Concept</em></a>
<a class="card" href="/pages/requests-and-outcomes/"><b>Requests and outcomes</b><span>How long operations return later.</span><em>Concept</em></a>
<a class="card" href="/pages/primitives/"><b>Pane primitives</b><span>Terminal, agent, editor and basic panes.</span><em>Concept</em></a>
</div>

<h2>Look things up</h2>
<div class="cards">
<a class="card" href="/pages/reference/"><b>Reference</b><span>Manifest keys, events, intents, errors, limits.</span><em>Reference</em></a>
<a class="card" href="/pages/api-fs/"><b>Host API</b><span>Every import a plugin can call, with examples.</span><em>57 pages</em></a>
<a class="card" href="/pages/widgets-text/"><b>Widget catalogue</b><span>The ratatui-shaped UI vocabulary.</span><em>9 pages</em></a>
<a class="card" href="/pages/reference-capabilities/"><b>All 68 capabilities</b><span>Grant syntax, risk and what each unlocks.</span><em>69 pages</em></a>
</div>
<div class="note">Status: the plugin system is designed, not built. Every render on these pages is an illustration. API names follow the contract <code>ilium:plugin@0.1.0</code> and can still change.</div>
</article>
