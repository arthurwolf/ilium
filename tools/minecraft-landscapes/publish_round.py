#!/usr/bin/env python3
"""Publish completed saved/generated captures into the offline Minecraft demo."""

from __future__ import annotations

import argparse
import hashlib
import html
import json
import shutil
from pathlib import Path


PAGE = Path(__file__).resolve().parents[2] / "docs/minecraft-landscapes/demo.html"
CHAPTER_ID = "round-2"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def esc(value: object) -> str:
    return html.escape(str(value), quote=True)


def capture_images(record: dict, capture_root: Path) -> list[Path]:
    result = record.get("result")
    frames = result.get("frames") if isinstance(result, dict) else None
    if not isinstance(frames, list) or not frames:
        raise ValueError(f"{record.get('case_id')}: no captured frames in case result")
    images: list[Path] = []
    for frame in frames:
        raw_path = frame.get("path") if isinstance(frame, dict) else None
        if not isinstance(raw_path, str) or not raw_path:
            raise ValueError(f"{record.get('case_id')}: frame has no image path")
        image = Path(raw_path).resolve(strict=True)
        if not image.is_relative_to(capture_root):
            raise ValueError(f"capture image escaped its evidence root: {image}")
        if image.suffix.lower() != ".png" or image.stat().st_size == 0:
            raise ValueError(f"capture image is not a non-empty PNG: {image}")
        images.append(image)
    return images


def card(case: dict, source_images: list[Path], capture_root: Path,
         asset_root: Path) -> str:
    case_id = case["case_id"]
    destination_dir = asset_root / "round-2"
    destination_dir.mkdir(parents=True, exist_ok=True)
    image_cards = []
    for index, source in enumerate(source_images, start=1):
        name = f"{case_id}-frame-{index:02d}.png"
        destination = destination_dir / name
        if source.resolve() != destination.resolve():
            shutil.copy2(source, destination)
        relative = f"demo/round-2/{name}"
        mode = case["texture_mode"]
        world_or_biome = case.get("world") or case.get("biome") or case.get("biome_id")
        label = f"{world_or_biome} · {mode} · frame {index}"
        image_cards.append(
            f'<figure class="card"><button class="image-open" type="button" '
            f'data-full="{esc(relative)}" aria-label="Open {esc(label)} full screen">'
            f'<img src="{esc(relative)}" alt="Isometric Minecraft {esc(world_or_biome)} '
            f'rendered with {esc(mode)}, frame {index}" loading="lazy"></button>'
            f'<figcaption><div class="caption-top"><strong>{esc(world_or_biome)}</strong>'
            f'<span>{esc(mode)}</span></div><p>Frame {index} of {len(source_images)}; '
            f'capture case <code>{esc(case_id)}</code>.</p>'
            f'<details><summary>Capture facts</summary><dl>'
            f'<dt>Case</dt><dd><code>{esc(case_id)}</code></dd>'
            f'<dt>Kind</dt><dd>{esc(case.get("kind"))}</dd>'
            f'<dt>Zoom</dt><dd>{esc(case.get("capture", {}).get("zoom", "not recorded"))}%</dd>'
            f'<dt>Copied from</dt><dd><code>{esc(capture_root)}</code></dd>'
            f'<dt>Image SHA-256</dt><dd><code>{sha256(destination)}</code></dd>'
            f'<dt>Evidence source</dt><dd>{esc(source.relative_to(capture_root))}</dd>'
            f'</dl></details></figcaption></figure>'
        )
    return "\n".join(image_cards)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capture-root", type=Path, required=True)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--triggering-prompt-file", type=Path, required=True)
    parser.add_argument("--transcript-path", required=True)
    parser.add_argument("--page", type=Path, default=PAGE)
    args = parser.parse_args()
    for path in (args.capture_root, args.plan, args.triggering_prompt_file):
        if not path.is_absolute():
            raise SystemExit(f"path must be absolute: {path}")
    capture_root = args.capture_root.resolve(strict=True)
    plan_path = args.plan.resolve(strict=True)
    page = args.page.resolve(strict=True)
    asset_root = page.with_suffix("")
    plan = json.loads(plan_path.read_text())
    progress_path = capture_root / "capture-progress.json"
    summary_path = capture_root / "capture-summary.json"
    evidence_path = summary_path if summary_path.is_file() else progress_path
    evidence = json.loads(evidence_path.read_text())
    cases = {case["case_id"]: case for case in plan.get("cases", [])}
    records = evidence.get("cases", [])
    completed = [record for record in records
                 if record.get("status") == "captured_pending_pixel_review"
                 and record.get("case_id") in cases]
    if not completed:
        raise SystemExit("no accepted capture records are available to publish")

    prompt = args.triggering_prompt_file.read_text().strip()
    if not prompt:
        raise SystemExit("triggering prompt is empty")
    sections = {"saved-world": [], "generated-biome": []}
    for record in completed:
        case = cases[record["case_id"]]
        section = sections.get(case.get("kind"))
        if section is None:
            continue
        sources = capture_images(record, capture_root)
        section.append(card(case, sources, capture_root, asset_root))

    saved_count = sum(case.get("kind") == "saved-world" for case in
                      (cases[r["case_id"]] for r in completed))
    generated_count = sum(case.get("kind") == "generated-biome" for case in
                          (cases[r["case_id"]] for r in completed))
    prompt_html = esc(prompt).replace("\n", "<br>")
    chapter = (
        f'<!-- {CHAPTER_ID}:start -->'
        f'<details class="chapter" id="{CHAPTER_ID}" open><summary>Round 2 · '
        f'current-source saved and generated captures ({len(completed)} cases)</summary>'
        '<div class="chapter-body">'
        f'<blockquote><p>{prompt_html}</p></blockquote>'
        f'<p class="transcript">Triggering session transcript: '
        f'<code>{esc(args.transcript_path)}</code>.</p>'
        '<p class="lede">Current capture evidence is accumulating below. Each image is a '
        'native renderer output copied into this page’s own asset folder; open an image '
        'for the uncropped view. Cases not yet present in the retained capture summary '
        'remain outstanding.</p>'
        f'<div class="findings"><div class="finding"><strong>{saved_count} saved-world cases</strong>'
        '<p>Images from on-disk saves.</p></div>'
        f'<div class="finding"><strong>{generated_count} generated cases</strong>'
        '<p>Images from procedural biome scenes.</p></div>'
        f'<div class="finding"><strong>{len(completed)} of {len(cases)} cases published</strong>'
        '<p>Only successful, validated capture records are included.</p></div></div>'
        '<h2>Saved worlds</h2><div class="gallery">'
        + "\n".join(sections["saved-world"])
        + '</div><h2>Generated landscapes</h2><div class="gallery">'
        + "\n".join(sections["generated-biome"])
        + '</div><p class="section-note">Capture receipts, pixels and pack provenance must '
        'still pass the separate visual and native-emission checks before this round can '
        'be described as accepted.</p></div></details>'
        f'<!-- {CHAPTER_ID}:end -->'
    )

    text = page.read_text()
    start_marker = f'<!-- {CHAPTER_ID}:start -->'
    end_marker = f'<!-- {CHAPTER_ID}:end -->'
    start = text.find(start_marker)
    if start >= 0:
        end = text.find(end_marker, start)
        if end < 0:
            raise SystemExit("existing Round 2 chapter is malformed")
        end += len(end_marker)
        text = text[:start] + chapter + text[end:]
    else:
        marker = "<footer>"
        position = text.find(marker)
        if position < 0:
            raise SystemExit("demo page footer marker not found")
        text = text[:position] + chapter + "\n" + text[position:]
    temporary = page.with_suffix(".html.tmp")
    temporary.write_text(text)
    temporary.replace(page)
    print(json.dumps({"type": "result", "status": "published",
                      "page": str(page), "chapter": CHAPTER_ID,
                      "cases": len(completed), "saved": saved_count,
                      "generated": generated_count}, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
