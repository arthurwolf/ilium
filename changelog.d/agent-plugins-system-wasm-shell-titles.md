### Changed
- A command typed at a plain shell prompt now names its pane through the Titles plugin: the server reports each exactly rebuilt shell line as `user-line-submitted`, and the plugin turns it into an inferred title that never replaces a name you typed. With the Titles plugin off, shell panes keep their default title.
- A plugin rename with source `inferred` on a terminal pane now goes through the automatic title path (a confirmed plain shell with an automatic title only) instead of renaming the pane by hand.
