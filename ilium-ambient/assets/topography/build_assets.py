#!/usr/bin/env python3
"""Rebuild the embedded topography grids from the public source datasets.

Usage: build_assets.py --source-dir DIR --out-dir DIR
Needs numpy, scipy, tifffile, imagecodecs, pillow. Prints one JSONL object per
body (type=body) and a final type=summary line. The source files are listed in
manifest.json ("source_url"); download them into --source-dir first.
Each body becomes a 16-bit grayscale equirectangular PNG (row 0 = north pole,
column 0 = longitude -180 degrees) whose samples (level << 4 | 8, level 0..4095) map linearly onto
[min_m, max_m] recorded in manifest.json.
"""
import argparse, json, os, sys
import numpy as np
from PIL import Image
from scipy import ndimage
import tifffile

TARGET_WIDTH = 2048
QUANT_LEVELS = 4095  # 12-bit steps stored in the high bits of 16-bit PNG samples: compresses far better


def read_raw(path, dtype, shape):
    return np.fromfile(path, dtype=dtype).reshape(shape).astype(np.float32)


def shift_to_minus_180(grid, zero_lon_first):
    """Datasets start at 0E (PDS) or -180 (GeoTIFF); the output starts at -180."""
    return np.roll(grid, grid.shape[1] // 2, axis=1) if zero_lon_first else grid


def fill_nodata(grid, nodata):
    mask = ~np.isfinite(grid) if nodata is None else (grid == nodata) | ~np.isfinite(grid)
    if not mask.any():
        return grid
    grid = grid.copy()
    if mask.all():
        raise SystemExit("all samples are no-data")
    idx = ndimage.distance_transform_edt(mask, return_distances=False, return_indices=True)
    return grid[tuple(idx)]


def reduce_width(grid, width):
    if grid.shape[1] <= width:
        return grid
    height = width // 2
    image = Image.fromarray(grid, mode="F").resize((width, height), Image.BOX)
    return np.asarray(image, dtype=np.float32)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-dir", required=True)
    parser.add_argument("--out-dir", required=True)
    args = parser.parse_args()
    manifest = json.load(open(os.path.join(args.out_dir, "manifest.json")))
    for body in manifest["bodies"]:
        src = os.path.join(args.source_dir, body["source_file"])
        kind = body["source_format"]
        if kind == "pds_lsb_i2_scaled":
            grid = read_raw(src, "<i2", (720, 1440)) * body["scale"]
            zero_first = True
            nodata = None
        elif kind == "pds_msb_i2":
            grid = read_raw(src, ">i2", (2880, 5760))
            zero_first = True
            nodata = None
        else:
            grid = tifffile.imread(src).astype(np.float32)
            zero_first = False
            nodata = body.get("nodata")
        grid = fill_nodata(grid, nodata)
        grid = shift_to_minus_180(grid, zero_first)
        grid = reduce_width(grid, TARGET_WIDTH)
        if body.get("detrend") == "zonal":
            # Ceres is an oblate spheroid but the DTM is referenced to a sphere:
            # without this the equatorial bulge swamps the real relief.
            grid = grid - np.median(grid, axis=1, keepdims=True)
        # Clip speckle at the extremes so the 12-bit range is spent on real relief.
        low, high = (float(v) for v in np.percentile(grid, [0.001, 99.999]))
        grid = np.clip(grid, low, high)
        # Earth is measured against sea level; other bodies against their median
        # surface, because their datums are reference spheres or ellipsoids.
        body["zero_m"] = 0.0 if body["id"] == "earth" else round(float(np.median(grid)), 0)
        unit = np.clip((grid - low) / (high - low), 0.0, 1.0)
        out = os.path.join(args.out_dir, body["id"] + ".png")
        Image.fromarray((np.round(unit * QUANT_LEVELS).astype(np.uint16) << 4) | 8).save(out, optimize=True)
        body["min_m"] = round(low, 1)
        body["max_m"] = round(high, 1)
        body["width"] = int(grid.shape[1])
        body["height"] = int(grid.shape[0])
        print(json.dumps({"type": "body", "id": body["id"], "path": out, "min_m": low,
                          "max_m": high, "bytes": os.path.getsize(out)}), flush=True)
    json.dump(manifest, open(os.path.join(args.out_dir, "manifest.json"), "w"), indent=2)
    print(json.dumps({"type": "summary", "bodies": len(manifest["bodies"])}))


if __name__ == "__main__":
    sys.exit(main())
