#!/usr/bin/env python3
"""Write the bounded synthetic Markdown corpus used by the renderer probe."""
import argparse
from pathlib import Path


def markdown():
    return "".join(f'### Section {i}\n\nParagraph **synthetic** and `code`.\n\n'
                   f'```rust\nlet value_{i} = "' + ("data " * 32) + '";\n```\n\n'
                   for i in range(800))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with args.output.open("x", encoding="utf-8") as stream:
        stream.write(markdown())
