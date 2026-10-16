#!/usr/bin/env bash
# Renders docs/screenshots/ out of example data through the window's painter, by
# hand after a visible change; which pages, and the way to each, is the ignored
# test `screenshots` in src/ui/app.rs. VERSION is what the pages show, so the
# next release can be pictured before it exists. docs/banner.sh comes after.
#
#   docs/screenshots.sh [VERSION]
set -euo pipefail
cd "$(dirname "$0")/.."

rm -f docs/screenshots/*.png
SM_VERSION="${1:-}" cargo test --locked --lib ui::app::tests::screenshots -- --ignored --exact
ls docs/screenshots
