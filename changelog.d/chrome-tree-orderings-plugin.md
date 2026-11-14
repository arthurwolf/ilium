### Changed
- The tree's item-type, newest-first, oldest-first and name (A-Z, Z-A) orders now come from the Tree core plugin (`ilium.core.tree`) as plugin tree orderings; the Order By menu and the Appearance "Tree order" row list Manual order plus every plugin ordering. A legacy `[ui] tree_order` value in `config.toml` is migrated to the matching plugin ordering.
- The tree no longer closes a group that its last item leaves empty by itself; with "Remove empty groups" on, the Tree core plugin does it.
