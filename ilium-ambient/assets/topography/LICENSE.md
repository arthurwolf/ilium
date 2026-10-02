# Topography data

The PNG grids are derived, downsampled and quantised (12-bit) from public-domain
US government elevation datasets; sources and credits per body are in
`manifest.json`, and `build_assets.py` rebuilds them:

- Earth: NOAA NCEI, ETOPO 2022 Global Relief Model, doi:10.25921/fd45-gt74
- Moon: NASA Lunar Reconnaissance Orbiter LOLA, LDEM_4 (PDS Geosciences Node)
- Mars: NASA Mars Global Surveyor MOLA, MEGDR 16 pixels/degree (PDS Geosciences Node)
- Venus: NASA Magellan global topography (USGS Astrogeology Science Center)
- Mercury: NASA MESSENGER MDIS global DEM (USGS Astrogeology Science Center)
- Ceres: NASA/JPL Dawn framing camera HAMO DTM, produced by DLR (USGS Astrogeology Science Center)

Elevations are relative to each body's reference surface; non-Earth bodies are
re-zeroed at their median surface (`zero_m`).
