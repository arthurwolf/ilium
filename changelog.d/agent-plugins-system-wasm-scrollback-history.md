### Added
- Plugins allowed to read terminal history can now read a pane's scrollback (the lines that scrolled off the top of the screen), rebuilt from the pane's retained output; the Search plugin finds matches in it again.
- A plugin can scroll a terminal pane's view back into its scrollback with the `kernel.scroll-terminal` command (`{"node": <id>, "rows_back": <rows>}`; 0 returns to the live tail).
