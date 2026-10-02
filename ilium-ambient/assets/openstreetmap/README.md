# OpenStreetMap world catalogue

Ten finite city-centre extracts are embedded for offline maps. Each gzip-compressed `.compact.json.gz` contains whole OpenStreetMap elements from its recorded query; JSON whitespace was removed and the byte stream was compressed losslessly. Decoding is bounded to 16 MiB per place on the source worker. `catalogue.json` records geographic bounds, OSM data timestamps, exact queries/source URLs, raw and compiled asset hashes, element counts and licensing.

Map data © OpenStreetMap contributors, available under the Open Database License (ODbL) 1.0:

https://www.openstreetmap.org/copyright
https://opendatacommons.org/licenses/odbl/1-0/

These are retained snapshots, not current traffic, live imagery or a worldwide download. Camera motion reuses geometry. Custom locations use a local Overpass `out geom` JSON extract or an explicitly configured HTTPS source. Large elements crossing an output bounding box may contain missing coordinates; these gaps must remain discontinuities, never invented connecting lines or filled polygons.
