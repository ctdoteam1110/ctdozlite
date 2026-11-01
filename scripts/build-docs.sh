#!/usr/bin/env bash
# Build trang tài liệu tĩnh từ docs/*.md.
#
#   scripts/build-docs.sh            # sinh Đã biết + build
#   SKIP_ISSUES=1 scripts/build-docs.sh
#
# Kết quả: book/index.html
#
# Cài mdbook một lần:
#   cargo install mdbook --locked
# hoặc tải binary tại https://github.com/rust-lang/mdBook/releases

set -euo pipefail

cd "$(dirname "$0")/.."

if [ "${SKIP_ISSUES:-0}" != "1" ]; then
  ./scripts/gen-known-issues.sh || echo "! Bỏ qua bước sinh Đã biết" >&2
fi

if ! command -v mdbook >/dev/null 2>&1; then
  echo "! Chưa có mdbook. Cài bằng: cargo install mdbook --locked" >&2
  exit 1
fi

mdbook build
echo "✓ Docs: $(pwd)/book/index.html"
