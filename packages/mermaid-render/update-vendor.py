#!/usr/bin/env python3
"""Повторно получить ровно проверенный Pele 1.12.0 без npm/node_modules."""
import hashlib
import io
from pathlib import Path
import tarfile
import urllib.request

VERSION = "1.12.0"
URL = f"https://registry.npmjs.org/pele/-/pele-{VERSION}.tgz"
SHA256 = "6da9b31273314272dde6b47fe97303612290ce93bdcf12c82b09fabb4129597d"
FILES = {
    "package/dist/pele.js": ("pele.mjs", "111172ef3cfe3ab8fd58cba28f69e2c23f1d9f08c63db306fb9eb654a77de20b"),
    "package/LICENSE": ("LICENSE", "311bccc30de6f7b9cbfb64232bb0c911ccb2191abe316550813ae25b2c7925b9"),
}


def main():
    with urllib.request.urlopen(URL, timeout=30) as response:
        data = response.read(4 * 1024 * 1024 + 1)
    if len(data) > 4 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != SHA256:
        raise SystemExit("Pele tarball: SHA-256 mismatch or oversized archive")
    root = Path(__file__).resolve().parent / "vendor" / "pele"
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        contents = {}
        for member, (filename, digest) in FILES.items():
            payload = archive.extractfile(member).read()
            if hashlib.sha256(payload).hexdigest() != digest:
                raise SystemExit(f"Pele {filename}: SHA-256 mismatch")
            contents[filename] = payload
    root.mkdir(parents=True, exist_ok=True)
    for filename, payload in contents.items():
        (root / filename).write_bytes(payload)


if __name__ == "__main__":
    main()
