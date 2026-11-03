#!/usr/bin/env python3
"""Caption the specific Resource Queue recording; run on ni-vm with Pillow.

The input must be the unedited recording identified below. The sequence is an
isolated staged demonstration with local agent fixtures, not provider calls.
This editor does not connect to Ilium or mutate resources.
"""
import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

SOURCE_SHA256 = "d4c980bf21650ae432a299a47b19052c4fc84207301901d3a100591cc1b5c052"
CAPTIONS = (
    (8, "One shared demo resource is free", "Staged session: local agent fixtures; no model or provider calls"),
    (20, "Agent A (pa) reserves the resource", "The fixture command 'res' requests the shared demo slot"),
    (35, "Agent B (pb) joins the queue", "The resource is taken, so B waits behind A"),
    (45, "Agent A (pa) releases the resource", "The fixture command 'rel' releases A's reservation"),
    (56, "Agent B (pb) takes the resource", "The waiting reservation is promoted after A releases"),
    (65, "Agent B (pb) releases the resource", "The shared demo resource is free again"),
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    if hashlib.sha256(args.input.read_bytes()).hexdigest() != SOURCE_SHA256:
        raise SystemExit("Input hash differs from the reviewed source recording")
    source = Image.open(args.input)
    if source.size != (1277, 843) or source.n_frames != 65:
        raise SystemExit("Unexpected source dimensions or frame count")
    title_font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 23)
    detail_font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 17)
    frames = []
    durations = []
    for index in range(source.n_frames):
        source.seek(index)
        image = source.convert("RGB")
        canvas = Image.new("RGB", (1277, 925), "#1d202b")
        canvas.paste(image, (0, 0))
        draw = ImageDraw.Draw(canvas)
        # Cover only the account-specific quota notice, leaving the UI intact.
        draw.rectangle((750, 794, 1090, 813), fill="#a682d6")
        title, detail = next((title, detail) for end, title, detail in CAPTIONS if index < end)
        draw.rectangle((0, 843, 1276, 924), fill="#1d202b")
        draw.rectangle((0, 843, 1276, 845), fill="#a682d6")
        draw.text((22, 855), title, font=title_font, fill="#f6f4fa")
        draw.text((22, 888), detail, font=detail_font, fill="#d0c8e2")
        frames.append(canvas)
        durations.append(source.info.get("duration", 100))
    frames[0].save(args.output, save_all=True, append_images=frames[1:], duration=durations,
                   loop=source.info.get("loop", 0), disposal=2, optimize=False)
    print(json.dumps({"type": "result", "frames": len(frames), "duration_ms": sum(durations),
                      "source_sha256": SOURCE_SHA256,
                      "output_sha256": hashlib.sha256(args.output.read_bytes()).hexdigest()}))


if __name__ == "__main__":
    main()
