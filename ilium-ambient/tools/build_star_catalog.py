#!/usr/bin/env python3
"""Regenerate ilium-ambient/assets/bright_stars.bin and constellation_lines.bin.

Source: Yale Bright Star Catalogue, 5th revised edition (Hoffleit and Warren 1991),
distributed by CDS as VizieR V/50 (https://cdsarc.cds.unistra.fr/ftp/V/50/).
The BSC is a public astronomical data catalogue distributed by the NASA/CDS
data centres for unrestricted scientific use.

Usage:
  build_star_catalog.py --catalog PATH_TO_catalog[.gz] --output-dir DIR
  build_star_catalog.py --download --cache-dir DIR --output-dir DIR   (one polite GET)

Output: JSONL on stdout, one object per line with a "type" field
(progress, warning, artifact, summary, error).

bright_stars.bin layout (little endian):
  magic  b"BSC1"        4 bytes
  count  u32
  count records of 6 bytes, ordered brightest first:
    ra   u16  right ascension J2000, ra_deg / 360 * 65536
    dec  u16  declination J2000, (dec_deg + 90) / 180 * 65535
    mag  u8   (V + 2.0) * 20      (0.05 mag steps, -2.0 .. 10.75)
    bv   u8   (B-V + 0.6) * 64    (colour index; 255 = unknown, treated as 0.6)
constellation_lines.bin layout:
  magic b"CLN1", count u16, then count pairs of u16 star indices into the
  bright_stars.bin order.

Constellation stick figures are the traditional patterns, authored for this
project by hand as chains of Bayer/Flamsteed designations (facts about which
bright stars are joined, no third-party line dataset is copied).
"""
import argparse, gzip, json, math, os, struct, sys, urllib.request

CATALOG_URL = "https://cdsarc.cds.unistra.fr/ftp/V/50/catalog.gz"
USER_AGENT = "ilium-ambient-tools/0.1 (+https://github.com/arthurwolf/ilium)"
MAG_LIMIT = 6.5

# Each chain joins consecutive designations. "Del Lyr" style; a bare Bayer name
# matches the brightest component when the catalogue numbers them (Alp1/Alp2).
CHAINS = """
Eta UMa,Zet UMa,Eps UMa,Del UMa,Gam UMa,Bet UMa,Alp UMa,Del UMa
Alp UMi,Del UMi,Eps UMi,Zet UMi,Bet UMi,Gam UMi,Eta UMi,Zet UMi
Eps Cas,Del Cas,Gam Cas,Alp Cas,Bet Cas
Bet Cep,Alp Cep,Zet Cep,Iot Cep,Gam Cep,Bet Cep
Alp Cyg,Gam Cyg,Bet Cyg
Del Cyg,Gam Cyg,Eps Cyg
Bet Lyr,Gam Lyr,Del2 Lyr,Zet1 Lyr,Bet Lyr
Alp Lyr,Zet1 Lyr
Gam Aql,Alp Aql,Bet Aql
Alp Aql,Del Aql,Zet Aql
Alp Ori,Zet Ori,Eps Ori,Del Ori,Gam Ori,Lam Ori,Alp Ori
Zet Ori,Kap Ori
Del Ori,Bet Ori
Bet Tau,Gam Tau,Alp Tau,Zet Tau
Gam Tau,Del1 Tau,Eps Tau
Alp Gem,Tau Gem,Eps Gem,Mu Gem,Eta Gem
Bet Gem,Ups Gem,Del Gem,Zet Gem,Gam Gem
Bet Cnc,Del Cnc,Gam Cnc
Del Cnc,Alp Cnc
Eps Leo,Mu Leo,Zet Leo,Gam1 Leo,Eta Leo,Alp Leo
Gam1 Leo,Del Leo,Bet Leo,The Leo,Del Leo
Bet Vir,Eta Vir,Gam Vir,Del Vir,Eps Vir
Gam Vir,Zet Vir,Alp Vir
Alp Boo,Eps Boo,Del Boo,Bet Boo,Gam Boo,Rho Boo,Alp Boo
The CrB,Bet CrB,Alp CrB,Gam CrB,Del CrB,Eps CrB,Iot CrB
Eta Her,Zet Her,Eps Her,Pi Her,Eta Her
Zet Her,Bet Her
Eps Her,Del Her,Alp Her
Alp And,Bet And,Gam1 And
Alp Peg,Bet Peg,Alp And,Gam Peg,Alp Peg
Gam Per,Alp Per,Del Per,Eps Per,Zet Per
Alp Aur,Bet Aur,The Aur,Bet Tau,Iot Aur,Alp Aur
Bet CMa,Alp CMa,Del CMa,Eps CMa
Del CMa,Eta CMa
Alp CMi,Bet CMi
Alp2 Lib,Bet Lib,Gam Lib,Sig Lib,Alp2 Lib
Bet1 Sco,Del Sco,Sig Sco,Alp Sco,Tau Sco,Eps Sco,Mu1 Sco,Zet2 Sco,Eta Sco,The Sco,Iot1 Sco,Kap Sco,Lam Sco,Ups Sco
Lam Sgr,Del Sgr,Eps Sgr,Zet Sgr,Tau Sgr,Sig Sgr,Phi Sgr,Lam Sgr
Del Sgr,Gam2 Sgr,Eps Sgr
Alp2 Cap,Bet Cap,Psi Cap,Ome Cap,Zet Cap,Eps Cap,Gam Cap,Del Cap,The Cap,Alp2 Cap
Eps Aqr,Bet Aqr,Alp Aqr,Gam Aqr,Zet2 Aqr,Eta Aqr
Bet Ari,Alp Ari,41 Ari
Alp Cru,Gam Cru
Bet Cru,Del Cru
Alp1 Cen,Bet Cen
Alp2 CVn,Bet CVn
Alp Crv,Eps Crv,Bet Crv,Del Crv,Gam Crv,Eps Crv
Alp Del,Bet Del,Del Del,Gam2 Del,Alp Del
Gam Sge,Del Sge,Alp Sge
Del Sge,Bet Sge
Gam Dra,Bet Dra,Nu2 Dra,Xi Dra,Gam Dra
Gam Dra,Eta Dra,Zet Dra,Iot Dra,Alp Dra
Alp Oph,Bet Oph,Gam Oph,Nu Oph
Kap Oph,Alp Oph,Del Oph,Eps Oph
Alp Lep,Bet Lep
Alp Cet,Gam Cet,Del Cet,Zet Cet
Alp Phe,Bet Phe
Alp Gru,Bet Gru
Alp Tri,Bet Tri
Alp Lup,Bet Lup
Alp Tuc,Gam Tuc
Alp Pav,Bet Pav
Alp PsA,Bet PsA
Alp Hya,Iot Hya
"""


def emit(record):
    sys.stdout.write(json.dumps(record) + "\n")
    sys.stdout.flush()


def load_catalog(args):
    path = args.catalog
    if args.download:
        os.makedirs(args.cache_dir, exist_ok=True)
        path = os.path.join(args.cache_dir, "bsc5_catalog.gz")
        if not os.path.exists(path):
            emit({"type": "progress", "message": "downloading " + CATALOG_URL})
            request = urllib.request.Request(CATALOG_URL, headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(request, timeout=60) as response, open(path, "wb") as out:
                out.write(response.read())
    if not path:
        emit({"type": "error", "message": "give --catalog or --download"})
        sys.exit(2)
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt", encoding="latin-1") as handle:
        return handle.read().splitlines()


def parse_float(text):
    text = text.strip()
    return float(text) if text else None


def parse_stars(lines):
    stars = []
    for line in lines:
        if len(line) < 148 or not line[75:77].strip():
            continue  # the 14 novae/extragalactic entries carry no position
        mag = parse_float(line[102:107])
        if mag is None or mag > MAG_LIMIT:
            continue
        ra = int(line[75:77]) * 15.0 + int(line[77:79]) * 0.25 + float(line[79:83]) * 15.0 / 3600.0
        dec = int(line[83 + 1:86]) + int(line[86:88]) / 60.0 + int(line[88:90]) / 3600.0
        if line[83] == "-":
            dec = -dec
        bv = parse_float(line[109:114])
        name = line[4:14]
        flamsteed = name[0:3].strip()
        bayer = name[3:6].strip()
        sup = name[6:7].strip()
        constellation = name[7:14].strip()
        keys = []
        if bayer:
            keys.append(f"{bayer}{sup} {constellation}")
            keys.append(f"{bayer} {constellation}")
        if flamsteed:
            keys.append(f"{flamsteed} {constellation}")
        stars.append({"hr": int(line[0:4]), "ra": ra, "dec": dec, "mag": mag, "bv": bv, "keys": keys})
    stars.sort(key=lambda star: (star["mag"], star["hr"]))
    return stars


def encode_stars(stars):
    out = bytearray(b"BSC1" + struct.pack("<I", len(stars)))
    for star in stars:
        ra = round(star["ra"] / 360.0 * 65536) % 65536
        dec = round((star["dec"] + 90.0) / 180.0 * 65535)
        mag = max(0, min(255, round((star["mag"] + 2.0) * 20)))
        bv = 255 if star["bv"] is None else max(0, min(254, round((star["bv"] + 0.6) * 64)))
        out += struct.pack("<HHBB", ra, dec, mag, bv)
    return bytes(out)


def angular_separation(a, b):
    ra1, de1, ra2, de2 = (math.radians(v) for v in (a["ra"], a["dec"], b["ra"], b["dec"]))
    cosine = math.sin(de1) * math.sin(de2) + math.cos(de1) * math.cos(de2) * math.cos(ra1 - ra2)
    return math.degrees(math.acos(max(-1.0, min(1.0, cosine))))


def build_lines(stars):
    exact = {}
    prefix = {}
    for index, star in enumerate(stars):
        for key in star["keys"]:
            exact.setdefault(key, index)
    def resolve(name):
        if name in exact:
            return exact[name]
        bayer, constellation = name.rsplit(" ", 1)
        candidates = [i for s_i, s in enumerate(stars) for i in [s_i]
                      if any(k.startswith(bayer) and k.endswith(" " + constellation)
                             and len(k.split(" ")[0]) <= len(bayer) + 1 for k in s["keys"])]
        return min(candidates) if candidates else None  # stars sorted brightest first
    pairs = set()
    for chain in CHAINS.strip().splitlines():
        names = [n.strip() for n in chain.split(",")]
        indices = []
        for name in names:
            index = resolve(name)
            if index is None:
                emit({"type": "warning", "message": "unresolved designation", "name": name})
            indices.append(index)
        for a, b in zip(indices, indices[1:]):
            if a is not None and b is not None and a != b:
                pairs.add((min(a, b), max(a, b)))
    pairs = sorted(pairs)
    for a, b in pairs:
        separation = angular_separation(stars[a], stars[b])
        if separation > 30.0:
            emit({"type": "warning", "message": "long constellation segment (check designations)",
                  "hr_a": stars[a]["hr"], "hr_b": stars[b]["hr"], "degrees": round(separation, 1)})
    out = bytearray(b"CLN1" + struct.pack("<H", len(pairs)))
    for a, b in pairs:
        out += struct.pack("<HH", a, b)
    return bytes(out), len(pairs)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--catalog", help="local BSC5 'catalog' file (plain or .gz)")
    parser.add_argument("--download", action="store_true", help="fetch the catalogue once into --cache-dir")
    parser.add_argument("--cache-dir", default=".cache")
    parser.add_argument("--output-dir", required=True)
    args = parser.parse_args()
    lines = load_catalog(args)
    emit({"type": "progress", "message": "parsing", "records": len(lines)})
    stars = parse_stars(lines)
    os.makedirs(args.output_dir, exist_ok=True)
    star_bytes = encode_stars(stars)
    line_bytes, pair_count = build_lines(stars)
    for name, payload in (("bright_stars.bin", star_bytes), ("constellation_lines.bin", line_bytes)):
        path = os.path.join(args.output_dir, name)
        with open(path, "wb") as handle:
            handle.write(payload)
        emit({"type": "artifact", "path": os.path.abspath(path), "bytes": len(payload)})
    emit({"type": "summary", "stars": len(stars), "line_segments": pair_count, "mag_limit": MAG_LIMIT})


if __name__ == "__main__":
    main()
