#!/usr/bin/env python3
"""Collection-scoped pre-request test fixture script (outermost scope):
reads the request context JSON from stdin, appends "O" to whatever
X-Order header value is already there (empty if none), and hands it back
as a header override — proving run order by proving what was already
applied before this script ran.
"""
import json
import sys


def main():
    context = json.load(sys.stdin)
    existing = next(
        (h["value"] for h in context.get("headers", []) if h["name"] == "X-Order"),
        "",
    )
    print(json.dumps({"headers": {"X-Order": existing + "O"}}))


if __name__ == "__main__":
    main()
