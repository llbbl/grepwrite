#!/usr/bin/env bash
# Pre-canned smoke demo. Runs inside the grepwrite-smoke container.
set -euo pipefail

git init -q
git config user.email gw@example.com
git config user.name gw
echo 'TODO: write the docs' > a.md
echo 'TODO: tests' > b.md
git add -A && git commit -q -m initial

echo "=== find ==="
gw find TODO -o caveman

echo "=== rewrite dry-run ==="
gw rewrite TODO DONE -o diff

echo "=== rewrite --apply ==="
gw rewrite TODO DONE --apply -o caveman

echo "=== snapshots ==="
gw snapshots

echo "=== undo ==="
gw undo

echo "=== verify restored ==="
cat a.md b.md
