#!/usr/bin/env python3
"""Pre-request test fixture script: reads the request context JSON from
stdin (ignored here) and hands back a header to add to the outgoing
request, per the pre-request JSON contract described in nova-engine's
`script` module.
"""
import json
import sys


def main():
    # The request context is available on stdin if a real script needed
    # it (e.g. to compute an HMAC signature over the body); this fixture
    # just adds a fixed header to prove the override plumbing works.
    json.load(sys.stdin)
    print(json.dumps({"headers": {"X-Signature": "deadbeef"}}))


if __name__ == "__main__":
    main()
