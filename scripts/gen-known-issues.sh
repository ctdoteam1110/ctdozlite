#!/usr/bin/env bash
# Sinh docs/DA_BIET.md từ các issue đang mở trên GitHub.
#
# Vì sao: người dùng mở issue trùng vì họ không biết lỗi đó đã được ghi nhận.
# Trang "Đã biết" lấy dữ liệu trực tiếp từ issue tracker nên không bao giờ lệch.
#
# Dùng:
#   scripts/gen-known-issues.sh              # ghi vào docs/DA_BIET.md
#   GITHUB_TOKEN=... scripts/gen-known-issues.sh
#   REPO=user/fork scripts/gen-known-issues.sh /tmp/out.md
#
# Nếu không có mạng: giữ nguyên file cũ và thoát 0, để `mdbook build` vẫn chạy được.

set -euo pipefail

REPO="${REPO:-ctdoteam1110/ctdozlite}"
OUT="${1:-docs/DA_BIET.md}"
API="https://api.github.com/repos/${REPO}/issues?state=open&per_page=100&sort=created&direction=desc"

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

auth=()
if [ -n "${GITHUB_TOKEN:-}" ]; then
  auth=(-H "Authorization: Bearer ${GITHUB_TOKEN}")
fi

if ! curl -fsSL -H "Accept: application/vnd.github+json" "${auth[@]}" "$API" -o "$tmp"; then
  echo "! Không tải được issue từ GitHub — giữ nguyên ${OUT}" >&2
  exit 0
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "! Thiếu jq — giữ nguyên ${OUT}" >&2
  exit 0
fi

# Phân loại: ưu tiên label, nếu chưa có label thì đoán theo tiền tố tiêu đề.
# (Repo hiện chưa gắn label cho issue mới, nên nhánh đoán là nhánh chính.)
JQ_CLASSIFY='
  def cat:
    (.labels | map(.name)) as $l
    | if ($l | index("bug")) then "bug"
      elif ($l | index("enhancement")) then "feat"
      elif (.title | test("^(fix|bug)[(:]"; "i")) then "bug"
      elif (.title | test("^(feat|feature)[(:]"; "i")) then "feat"
      else "other" end;

  def rows($c):
    .[] | select(.pull_request == null) | select(cat == $c)
    | "| [#\(.number)](\(.html_url)) | \(.title | gsub("\\|"; "\\|")) |";
'

issues() {
  jq -r "[.[] | select(.pull_request == null)] | .[] | $1" "$tmp"
}

total="$(jq '[.[] | select(.pull_request == null)] | length' "$tmp")"
bugs="$(jq '[.[] | select(.pull_request == null)] | map(select(.title | test("^(fix|bug)[(:]"; "i"))) | length' "$tmp")"
features="$(jq '[.[] | select(.pull_request == null)] | map(select(.title | test("^(feat|feature)[(:]"; "i"))) | length' "$tmp")"

table_for() {
  local cat="$1"
  jq -r "${JQ_CLASSIFY} [.[] | select(.pull_request == null)] | rows(\"${cat}\")" "$tmp"
}

{
  echo "# Đã biết"
  echo
  echo "Trang này được **sinh tự động** từ GitHub Issues — đừng sửa tay."
  echo "Chạy \`scripts/gen-known-issues.sh\` để cập nhật."
  echo
  echo "> Nguồn: [${REPO}/issues](https://github.com/${REPO}/issues) · cập nhật $(date -u '+%Y-%m-%d %H:%M UTC')"
  echo
  echo "Đang mở: **${total}** mục, trong đó **${bugs}** là lỗi và **${features}** là đề xuất."
  echo

  if [ "$bugs" -gt 0 ]; then
    echo "## Lỗi đã biết"
    echo
    echo "| Issue | Mô tả |"
    echo "|---|---|"
    table_for bug
    echo
  fi

  if [ "$features" -gt 0 ]; then
    echo "## Đề xuất đang xem xét"
    echo
    echo "| Issue | Mô tả |"
    echo "|---|---|"
    table_for feat
    echo
  fi

  echo "## Tất cả mục đang mở"
  echo
  echo "| Issue | Mô tả |"
  echo "|---|---|"
  issues '"| [#\(.number)](\(.html_url)) | \(.title | gsub("\\|"; "\\|")) |"'
  echo
  echo "---"
  echo
  echo "Không thấy lỗi của bạn? [Mở issue mới](https://github.com/${REPO}/issues/new) và ghi rõ hệ điều hành, phiên bản app, chế độ GUI/TUI và các bước tái hiện."
} > "$OUT"

echo "✓ Đã ghi ${OUT} (${total} mục: ${bugs} lỗi, ${features} đề xuất)"
