### Added
- Client plugins can read and write the clipboard (`clipboard.read`, `clipboard.write`). The text goes through the same clipboard helper as terminal copy and paste, and each read shows "clipboard read by <plugin>" in the status line.
- Client plugins can ask the user for a file or a folder (`fs.pick`). The file picker opens over the current screen, names the plugin that asked, and accepts only the file types the plugin asked for.
- Client plugins can play video (`media.video`). Playback uses `ffmpeg` and `ffprobe` from `PATH`, each video is decoded in its own limited child process, and a plugin reads only files it may already read.
