#!/usr/bin/env python3
"""Collection-scoped pre-request test fixture script (inner scope, the
`users/` folder) — see `outer-pre.py` for the shared X-Order trick."""
import json
import sys


def main():
    context = json.load(sys.stdin)
    existing = next(
        (h["value"] for h in context.get("headers", []) if h["name"] == "X-Order"),
        "",
    )
    print(json.dumps({"headers": {"X-Order": existing + "I"}}))


if __name__ == "__main__":
    main()
