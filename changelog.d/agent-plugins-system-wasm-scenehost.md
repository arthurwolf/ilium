### Added
- Plugin scenes can receive events between frames (for example microphone audio for a spectrum) by exporting `handler`; the scene world gains the optional `with-handler` fragment.
- A plugin scene can read the folder or file typed in one of its path settings (`picker` rows) as a read-only `picked/<n>` scope, for image and video scenes.
- The `geocode` import of plugin scenes now answers with real places from the OpenStreetMap address search and the Open-Meteo place search, instead of always answering with no place.
