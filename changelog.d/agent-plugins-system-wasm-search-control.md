### Changed
- The voice agent's search tool now drives the Search plugin's panel: it types a query, selects a result, moves the selection, opens a result and closes the panel (before, only opening the panel worked and the other actions were refused). The plugin takes these as arguments of its `ilium.core.search/open` command.

### Removed
- Ten voice reply templates that no code used any more (search results opened or closed, board creation and update, board column and checkbox errors, editor updated).
