#!/usr/bin/env python3
"""Describe only the release files actually built, with streaming SHA-256 digests."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys


def generate(directory):
    repository = os.environ.get("RELEASE_REPOSITORY", "lebad280324/ctdozlite")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("Invalid release repository")
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], text=True))
    version = next(p["version"] for p in metadata["packages"] if p["name"] == "ctdozlite")
    tag = f"v{version}"
    expected = f"ctdozlite-{version}-macos-arm64.dmg"
    files = sorted(directory.glob("*.dmg"))
    if [p.name for p in files] != [expected]:
        raise ValueError("Expected exactly the versioned Apple Silicon DMG")
    base = f"https://github.com/{repository}"
    assets = []
    for path in files:
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        assets.append({"name": path.name, "sha256": digest, "size": path.stat().st_size,
                       "browser_download_url": f"{base}/releases/download/{tag}/{path.name}"})
    (directory / "SHA256SUMS.txt").write_text("".join(f'{a["sha256"]}  {a["name"]}\n' for a in assets))
    notes = Path(f"docs/releases/{tag}.md")
    manifest = {"schema_version": 1, "version": version, "tag_name": tag,
                "published_at": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "repository": base, "release_url": f"{base}/releases/tag/{tag}",
                "name": f"ctdozlite {tag} — macOS Apple Silicon · Liquid Glass",
                "notes": notes.read_text() if notes.exists() else "", "assets": assets}
    (directory / "latest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    print(f"Prepared {tag}: {expected} and SHA-256 metadata for {repository}")


if __name__ == "__main__":
    generate(Path(sys.argv[1] if len(sys.argv) > 1 else "dist"))
