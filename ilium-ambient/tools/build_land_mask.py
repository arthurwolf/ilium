#!/usr/bin/env python3
"""Regenerate ilium-ambient/assets/land_mask.bin from Natural Earth 110m land.

Source: Natural Earth "ne_110m_land" (public domain,
https://www.naturalearthdata.com/about/terms-of-use/), here read as GeoJSON from
https://github.com/nvkelso/natural-earth-vector (geojson/ne_110m_land.geojson).

Usage:
  build_land_mask.py --geojson ne_110m_land.geojson --output-dir DIR
  build_land_mask.py --download --cache-dir DIR --output-dir DIR   (one GET)

Output: JSONL on stdout (progress, artifact, summary, error).

land_mask.bin: 360 columns x 180 rows, 1 bit per sample, row-major, row 0 is
north (latitude 90..89), column 0 is longitude -180..-179; sample centres are
tested. Bit order: least significant bit first within each byte. 8100 bytes.
"""
import argparse, json, os, sys, urllib.request

URL = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_110m_land.geojson"
USER_AGENT = "ilium-ambient-tools/0.1 (+https://github.com/arthurwolf/ilium)"
WIDTH, HEIGHT = 360, 180


def emit(record):
    sys.stdout.write(json.dumps(record) + "\n")
    sys.stdout.flush()


def rings_of(geometry):
    if geometry["type"] == "Polygon":
        polygons = [geometry["coordinates"]]
    else:
        polygons = geometry["coordinates"]
    for polygon in polygons:
        for ring in polygon:
            yield ring


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--geojson")
    parser.add_argument("--download", action="store_true")
    parser.add_argument("--cache-dir", default=".cache")
    parser.add_argument("--output-dir", required=True)
    args = parser.parse_args()
    path = args.geojson
    if args.download:
        os.makedirs(args.cache_dir, exist_ok=True)
        path = os.path.join(args.cache_dir, "ne_110m_land.geojson")
        if not os.path.exists(path):
            emit({"type": "progress", "message": "downloading " + URL})
            request = urllib.request.Request(URL, headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(request, timeout=60) as response, open(path, "wb") as out:
                out.write(response.read())
    if not path:
        emit({"type": "error", "message": "give --geojson or --download"})
        sys.exit(2)
    with open(path, encoding="utf-8") as handle:
        data = json.load(handle)
    edges = []  # (lat_a, lon_a, lat_b, lon_b) with lat_a < lat_b
    for feature in data["features"]:
        for ring in rings_of(feature["geometry"]):
            for (lon_a, lat_a), (lon_b, lat_b) in zip(ring, ring[1:]):
                if lat_a == lat_b:
                    continue
                if lat_a > lat_b:
                    lon_a, lat_a, lon_b, lat_b = lon_b, lat_b, lon_a, lat_a
                edges.append((lat_a, lon_a, lat_b, lon_b))
    emit({"type": "progress", "message": "edges", "count": len(edges)})
    bits = bytearray((WIDTH * HEIGHT + 7) // 8)
    land = 0
    for row in range(HEIGHT):
        latitude = 90.0 - (row + 0.5)
        crossings = []
        for lat_a, lon_a, lat_b, lon_b in edges:
            if lat_a <= latitude < lat_b:
                fraction = (latitude - lat_a) / (lat_b - lat_a)
                crossings.append(lon_a + fraction * (lon_b - lon_a))
        crossings.sort()
        for start, end in zip(crossings[0::2], crossings[1::2]):
            for column in range(WIDTH):
                longitude = -180.0 + column + 0.5
                if start <= longitude < end:
                    index = row * WIDTH + column
                    bits[index // 8] |= 1 << (index % 8)
                    land += 1
    os.makedirs(args.output_dir, exist_ok=True)
    out_path = os.path.join(args.output_dir, "land_mask.bin")
    with open(out_path, "wb") as handle:
        handle.write(bytes(bits))
    emit({"type": "artifact", "path": os.path.abspath(out_path), "bytes": len(bits)})
    emit({"type": "summary", "land_samples": land, "land_fraction": round(land / (WIDTH * HEIGHT), 4)})


if __name__ == "__main__":
    main()
