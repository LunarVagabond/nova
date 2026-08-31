#!/usr/bin/env python3
"""Collection-scoped post-response test fixture script (inner scope, the
`users/` folder) — see `outer-post.py`."""
import json
import sys


def main():
    json.load(sys.stdin)
    print(json.dumps({"post_order": "inner"}))


if __name__ == "__main__":
    main()
