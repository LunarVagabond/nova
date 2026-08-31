#!/usr/bin/env python3
"""The request's own pre-request script (innermost, should run last) —
see `outer-pre.py` for the shared X-Order trick."""
import json
import sys


def main():
    context = json.load(sys.stdin)
    existing = next(
        (h["value"] for h in context.get("headers", []) if h["name"] == "X-Order"),
        "",
    )
    print(json.dumps({"headers": {"X-Order": existing + "R"}}))


if __name__ == "__main__":
    main()
