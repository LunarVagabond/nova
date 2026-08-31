#!/usr/bin/env python3
"""Collection-scoped post-response test fixture script (outermost scope,
should run last so its value is the one that survives) — extracts a
fixed `post_order` value naming which scope ran."""
import json
import sys


def main():
    json.load(sys.stdin)
    print(json.dumps({"post_order": "outer"}))


if __name__ == "__main__":
    main()
