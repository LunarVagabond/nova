#!/usr/bin/env python3
"""The request's own post-response script (should run first, so its
value gets overwritten by every enclosing scope) — see `outer-post.py`."""
import json
import sys


def main():
    json.load(sys.stdin)
    print(json.dumps({"post_order": "own"}))


if __name__ == "__main__":
    main()
