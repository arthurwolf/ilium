# catalogs

## catalogs

A catalogue is a named list of options that a `choice` row or widget can read instead of carrying the options itself. Ilium owns seven catalogues of its own data: palettes, dithers, colour filters, style presets, icons, sounds and models. A plugin may read them, and may declare catalogues of its own that it serves with the `catalog-provider` export. The `catalogs` import is the way to query both. The host does the filtering and the ranking, so every catalogue filters the same way.

### You will need

| Need | Value |
|---|---|
| Side | Server, client and scene. The import is in all three base worlds. |
| Grant | None for `ilium.*` catalogues, except `ilium.models`, which needs `inference`. Reading another plugin's catalogue needs `services.use:catalog/<plugin-id>`. |
| Manifest (own catalogue) | `contributes.catalogs`: `{ id, title, item_kind }`, plus `"exports": ["catalog-provider"]`. At most 8 catalogues per plugin. |
| Rust | Crate `ilium-plugin-sdk`, module `ilium_plugin_sdk::host::catalogs`; trait `ilium_plugin_sdk::exports::CatalogProvider`. |
| TypeScript | Package `@ilium/plugin-sdk`, module `@ilium/plugin-sdk/host/catalogs`; member `queryCatalog` of `definePlugin`. |

### Interface

```wit
record catalog-item {
  id: string, title: string,
  description: option<string>,          // at most 120 characters
  preview: option<preview>,             // glyph, swatch or pattern
  group: option<string>,                // heading in the choice dialog, at most 32 characters
  keywords: list<string>,               // at most 8, matched by the filter
  disabled-reason: option<string>,      // at most 40 characters
}
record catalog-page { items: list<catalog-item>, next: option<string>, total: option<u32> }
record catalog-query { catalog: string, filter: string, cursor: option<string>, limit: u16 }   // limit 1..256

query: func(q: catalog-query) -> result<catalog-page, plugin-error>;       // synchronous for host catalogues
query-async: func(q: catalog-query) -> result<request-id, plugin-error>;   // plugin catalogues
take-page: func(request: request-id) -> option<catalog-page>;
lookup: func(catalog: string, id: string) -> option<catalog-item>;

// export (with-catalog)
query-catalog: func(q: catalog-query) -> result<catalog-page, plugin-error>;
```

`preview` is one of `glyph(string)` (one grapheme), `swatch(list<u32>)` (1 to 16 RGB colours drawn as a strip of half blocks) or `pattern(list<string>)` (1 to 4 lines of at most 8 cells, drawn in the muted colour).

### Functions

| WIT function | Rust | TypeScript | Result | What it does |
|---|---|---|---|---|
| `query` | `catalogs::query(&CatalogQuery) -> Result<CatalogPage, PluginError>` | `query(q: CatalogQuery): CatalogPage` | page | Returns a page of a host catalogue now. |
| `query-async` | `catalogs::query_async(&CatalogQuery) -> Result<RequestId, PluginError>` | `queryAsync(q): bigint` | `request-id` | Starts a query to a plugin catalogue. The page arrives as a request completion; read it with `take-page`. |
| `take-page` | `catalogs::take_page(RequestId) -> Option<CatalogPage>` | `takePage(request): CatalogPage \| undefined` | page | Takes the finished page once. |
| `lookup` | `catalogs::lookup(&str, &str) -> Option<CatalogItem>` | `lookup(catalog, id): CatalogItem \| undefined` | item | Returns one item by id, for example to show the title of a stored choice. |
| `query-catalog` (export) | `CatalogProvider::query_catalog(&mut self, q: CatalogQuery) -> Result<CatalogPage, PluginError>` | `queryCatalog(q): CatalogPage` | page | Serves a catalogue the plugin declared. |

### The host catalogues

| Id | Content | Preview | Notes |
|---|---|---|---|
| `ilium.palettes` | The 38 and more palette presets of the shared look | `swatch` | Read-only kernel data. |
| `ilium.dithers` | Every dither mode | `pattern` | |
| `ilium.filters` | The 46 colour filters | `swatch` | |
| `ilium.style-presets` | The 50 and more style presets | none | |
| `ilium.icons` | The icon catalogue, grouped by set | `glyph` | The glyphs are not copied into the manual; the catalogue is the source. |
| `ilium.sounds` | Bundled and contributed sounds | none | |
| `ilium.models` | Configured inference models | none | Needs the `inference` grant. |

Plugin catalogues have the global id `<plugin-id>/<id>`. An unknown catalogue returns `not-found`.

### How queries behave

- **Filter.** Case-insensitive substring over title, id and keywords. Entries whose title starts with the filter rank first. A plugin catalogue does not implement filtering: the host applies it to the items the provider returns, except that a provider may narrow its work using `filter`.
- **Paging.** `limit` is 1 to 256. `next` holds the cursor for the following page, `total` the full count when known.
- **Deadline.** The export has 20 ms. A slow source returns the page it has cached and calls `ui.invalidate` (or `settings.set-choice-items`) when fresh data arrives.
- **Use in rows and widgets.** A `choice` row names a catalogue with `"source": { "catalog": "ilium.palettes" }`, and the widget uses `source: "ilium.palettes"`. A source that names neither a host catalogue nor one of the plugin's own fails the install with `manifest.catalog-unknown`.
- **Activation.** The trigger `catalog:<id>` starts a stopped instance when a row or widget opens one of its catalogues.

<figure>
<svg viewBox="0 0 420 210" role="img" aria-label="Query of a plugin catalogue from a choice row">
<defs>
<marker id="cat-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
<path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
</marker>
</defs>
<title>Catalogue query sequence</title>
<desc>The choice dialog asks the host for a page. The host asks the provider, filters and ranks the items, and draws them.</desc>
<text x="50" y="16" font-size="12" text-anchor="middle" fill="currentColor">Choice dialog</text>
<text x="210" y="16" font-size="12" text-anchor="middle" fill="currentColor">Host</text>
<text x="370" y="16" font-size="12" text-anchor="middle" fill="currentColor">Provider</text>
<line x1="50" y1="24" x2="50" y2="200" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="210" y1="24" x2="210" y2="200" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="370" y1="24" x2="370" y2="200" stroke="currentColor" stroke-dasharray="3 3"/>
<line x1="50" y1="48" x2="208" y2="48" stroke="currentColor" marker-end="url(#cat-arrow)"/>
<text x="60" y="42" font-size="10" fill="currentColor">typed filter "su"</text>
<line x1="210" y1="84" x2="368" y2="84" stroke="currentColor" marker-end="url(#cat-arrow)"/>
<text x="222" y="78" font-size="10" fill="currentColor">query-catalog(limit 256)</text>
<line x1="368" y1="116" x2="212" y2="116" stroke="currentColor" marker-end="url(#cat-arrow)"/>
<text x="232" y="110" font-size="10" fill="currentColor">catalog-page, within 20 ms</text>
<text x="214" y="140" font-size="10" fill="currentColor">host filters and ranks</text>
<line x1="208" y1="164" x2="52" y2="164" stroke="currentColor" marker-end="url(#cat-arrow)"/>
<text x="60" y="158" font-size="10" fill="currentColor">items with previews</text>
</svg>
<figcaption>The host sits between the dialog and the provider and applies the same filter to every catalogue.</figcaption>
</figure>

### Example 1: read a host catalogue

List the first 20 icons that match a word.

**Rust**

```rust
use ilium_plugin_sdk::host::catalogs::{self, CatalogQuery};
use ilium_plugin_sdk::host::log::{log, Level};

fn find_icons(word: &str) -> Result<(), PluginError> {
    let page = catalogs::query(&CatalogQuery {
        catalog: "ilium.icons".into(),
        filter: word.into(),
        cursor: None,
        limit: 20,
    })?;
    for item in page.items {
        log(Level::Debug, &format!("{} {}", item.id, item.title));
    }
    Ok(())
}
```

**TypeScript**

```ts
import { query } from '@ilium/plugin-sdk/host/catalogs';
import { log } from '@ilium/plugin-sdk/host/log';

export function findIcons(word: string): void {
  const page = query({ catalog: 'ilium.icons', filter: word, cursor: undefined, limit: 20 });
  for (const item of page.items) log('debug', `${item.id} ${item.title}`);
}
```

### Example 2: serve a catalogue of themes

The manifest declares `{ "id": "themes", "title": "Themes", "item_kind": "swatch" }`. A choice row uses `{ "source": { "catalog": "mythemes/themes" } }`.

**Rust**

```rust
use ilium_plugin_sdk::exports::CatalogProvider;
use ilium_plugin_sdk::types::ui::CatalogItem;
use ilium_plugin_sdk::host::catalogs::{CatalogPage, CatalogQuery};
use ilium_plugin_sdk::types::text::Preview;

impl CatalogProvider for Plugin {
    fn query_catalog(&mut self, _q: CatalogQuery) -> Result<CatalogPage, PluginError> {
        let items = self.themes.iter().map(|theme| {
            CatalogItem::new(&theme.id, &theme.name)
                .preview(Preview::Swatch(theme.colors.clone()))
                .group("Dark")
                .keywords(["night"])
        }).collect();
        Ok(CatalogPage { items, next: None, total: Some(self.themes.len() as u32) })
    }
}
```

**TypeScript**

```ts
import { definePlugin } from '@ilium/plugin-sdk';
import type { CatalogPage } from '@ilium/plugin-sdk/types';

export const { catalogProvider } = definePlugin({
  side: 'client',
  queryCatalog(_q): CatalogPage {
    return {
      items: themes.map((theme) => ({
        id: theme.id, title: theme.name, description: undefined,
        preview: { tag: 'swatch', val: theme.colors },
        group: 'Dark', keywords: ['night'], disabledReason: undefined,
      })),
      next: undefined,
      total: themes.length,
    };
  },
});
```

### What the user sees

A `choice` with `style: "dialog"` opens the filterable choice dialog of the client: a rounded block with the options, `●` before the current option, a status line with the count and the current value, and the thumb `┃` on the scrollbar track when the list is longer than the dialog. A disabled option shows its reason in brackets.

<pre class="term">
╭  ≡ ● · Palette─────────────────────────╮
│ su                                     │
│   Sunset                              ││
│ ● Sunrise                             ┃│
│   Subtle grey [unavailable]           ││
│ 3 of 38 options · Current: Sunrise     │
│ Enter applies · Esc cancels            │
╰────────────────────────────────────────╯
</pre>

The text lines are those of the client's choice dialog (`value_dialog.rs`). Swatch previews are drawn as a strip of half blocks next to the title and are not reproducible in plain text.

### Errors and limits

| Function | Code | When |
|---|---|---|
| `query`, `query-async` | `not-found` | The catalogue does not exist. |
| `query`, `query-async` | `invalid-argument` | `limit` is 0 or above 256. |
| `query-async`, export | `timed-out` | The provider missed its 20 ms deadline. |
| `query`, `query-async` | `not-permitted` | `ilium.models` without `inference`, or another plugin's catalogue without `services.use`. |
| any | `internal` | A host bug. |

Caps: 8 catalogues per plugin, 256 items per page, `description` 120 characters, `group` 32, `disabled-reason` 40, 8 keywords per item. Install errors: `manifest.catalog-unknown`, `manifest.contribution-limit`.

### See also

- [Plugin exports](/pages/api-exports/) for `catalog-provider` and its deadline.
- [Settings](/pages/api-settings/) for `choice` rows and `set-choice-items`.
- [Widgets: input](/pages/widgets-input/) for the `choice` widget.
- [Inference](/pages/api-inference/) for the model catalogue.
- [Services](/pages/api-services/) for `services.use`.

Contract: `ilium:plugin@0.1.0`, import `catalogs`, export fragment `with-catalog`.
