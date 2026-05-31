#!/usr/bin/env bash
# scripts/llms.sh — Generate llms.txt and llms-full.txt from the English docs.
#
# Both files are derived entirely from docs/en:
#   - llms.txt        : llmstxt.org index — H1, project pitch, one line per page.
#   - llms-full.txt   : the same header plus the full text of every page.
#
# Page order and membership come from the "## Contents" list in docs/en/index.md.
# Each page's title + one-line summary come from its YAML frontmatter
# (`title:` / `description:`). The project pitch is index.md's `description:`.
#
# Usage:
#   ./scripts/llms.sh            # regenerate both files
#   ./scripts/llms.sh --check    # fail (non-zero) if either file is out of date
#
# Requires: bash, awk, sed. No build step, no network.

set -euo pipefail

# ── config ────────────────────────────────────────────────────────────
REPO="petstack/roxy"
RAW_BASE="https://github.com/${REPO}/blob/main"
TITLE="roxy"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DOCS_DIR="$ROOT_DIR/docs/en"
INDEX="$DOCS_DIR/index.md"

CHECK=0
[[ "${1:-}" == "--check" ]] && CHECK=1

# ── helpers ───────────────────────────────────────────────────────────

# Read a single frontmatter field (between the first two `---` fences).
# Usage: frontmatter_field <file> <field>
frontmatter_field() {
  awk -v field="$2" '
    NR == 1 && $0 == "---" { infm = 1; next }
    infm && $0 == "---"    { exit }
    infm {
      # match "field: value" with optional surrounding spaces
      if ($0 ~ "^[[:space:]]*" field ":[[:space:]]*") {
        sub("^[[:space:]]*" field ":[[:space:]]*", "")
        print
        exit
      }
    }
  ' "$1"
}

# Strip the YAML frontmatter block, print the rest of the file verbatim.
strip_frontmatter() {
  awk '
    NR == 1 && $0 == "---" { infm = 1; next }
    infm && $0 == "---"    { infm = 0; next }
    !infm { print }
  ' "$1"
}

# Page filenames in order, parsed from the numbered "[text](file.md)" links
# in index.md. Only local .md links (no scheme, no anchor) are taken, and
# index.md itself is skipped.
ordered_pages() {
  grep -oE '\]\([a-z0-9-]+\.md\)' "$INDEX" \
    | sed -E 's/^\]\(//; s/\)$//' \
    | awk '!seen[$0]++ && $0 != "index.md"'
}

# ── gather data ───────────────────────────────────────────────────────
PITCH="$(frontmatter_field "$INDEX" description)"
if [[ -z "$PITCH" ]]; then
  echo "error: docs/en/index.md has no frontmatter 'description:' field" >&2
  exit 1
fi

# `mapfile`/`readarray` is bash 4+; macOS ships bash 3.2, so read line by line.
PAGES=()
while IFS= read -r _page; do
  [[ -n "$_page" ]] && PAGES+=("$_page")
done < <(ordered_pages)
if [[ ${#PAGES[@]} -eq 0 ]]; then
  echo "error: no page links found in docs/en/index.md '## Contents'" >&2
  exit 1
fi

# ── build llms.txt ────────────────────────────────────────────────────
build_index() {
  printf '# %s\n\n' "$TITLE"
  printf '> %s\n\n' "$PITCH"
  printf '## Documentation\n\n'
  local page title desc
  for page in "${PAGES[@]}"; do
    local path="$DOCS_DIR/$page"
    if [[ ! -f "$path" ]]; then
      echo "error: docs/en/index.md links to missing page '$page'" >&2
      exit 1
    fi
    title="$(frontmatter_field "$path" title)"
    desc="$(frontmatter_field "$path" description)"
    [[ -n "$title" ]] || { echo "error: $page has no frontmatter 'title:'" >&2; exit 1; }
    [[ -n "$desc"  ]] || { echo "error: $page has no frontmatter 'description:'" >&2; exit 1; }
    printf -- '- [%s](%s/docs/en/%s): %s\n' "$title" "$RAW_BASE" "$page" "$desc"
  done
}

# ── build llms-full.txt ───────────────────────────────────────────────
build_full() {
  printf '# %s\n\n' "$TITLE"
  printf '> %s\n\n' "$PITCH"
  printf 'This file concatenates the full English documentation for %s.\n' "$TITLE"
  local page
  for page in "${PAGES[@]}"; do
    printf '\n---\n\n'
    strip_frontmatter "$DOCS_DIR/$page" | sed -e 's/[[:space:]]*$//' | cat -s
  done
}

# ── write or check ────────────────────────────────────────────────────
write_or_check() {
  local name="$1" content="$2" dest="$ROOT_DIR/$1"
  if [[ "$CHECK" -eq 1 ]]; then
    if ! diff -q <(printf '%s' "$content") "$dest" >/dev/null 2>&1; then
      echo "out of date: $name (run ./scripts/llms.sh)" >&2
      return 1
    fi
    echo "up to date: $name"
  else
    printf '%s' "$content" > "$dest"
    echo "wrote: $name"
  fi
}

rc=0
write_or_check "llms.txt"      "$(build_index)"$'\n' || rc=1
write_or_check "llms-full.txt" "$(build_full)"$'\n'  || rc=1
exit "$rc"
