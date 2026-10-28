# Recipes are bash and stop at the first failing command, in pipes too.
SHELL       := bash
.SHELLFLAGS := -eu -o pipefail -c

BIN := target/release/system-manager

# Bash: the scripts the runtime runs, the files they put in place, and the
# tools.
BASH_SCRIPTS  := $(wildcard scripts/*/*.sh scripts/*/*/*.sh scripts/*/*/*/*.sh tools/*.sh)

.PHONY: all build check secrets-check fmt fmt-check lint test aur-check run install screenshots banner github clean

all: build

build:
	cargo build --release --locked

# The one command that has to pass before a commit, cheapest and loudest first.
check: secrets-check fmt-check lint test aur-check build

secrets-check:
	gitleaks dir . --redact --no-banner

fmt:
	cargo fmt
	shfmt -w -i 4 $(BASH_SCRIPTS)

fmt-check:
	cargo fmt --check
	shfmt -d -i 4 $(BASH_SCRIPTS)

lint:
	cargo clippy --all-targets --locked -- -D warnings
	shellcheck -x -S style $(BASH_SCRIPTS)
	yamllint -s .
	actionlint
	zizmor --offline --persona auditor .github
	msgfmt --check-format -o /dev/null locales/de.po

test:
	cargo test --locked

# The PKGBUILD as makepkg reads it, stamped with stand-ins.
aur-check:
	tools/aur.sh stamp 0.0.0 LICENSE target/aur-check

# Against a throwaway configuration, the menu entry and the timer included, and
# so the data and settings of this user that what it runs reads, such as
# Flatpak's installation of this user and paru.conf. The home folder is a
# throwaway too, as ~/.dotfiles and their links lie there, so cargo builds with
# the real one. What it runs still runs on this machine. GSettings goes to a
# file there: on the session's dconf it would write the real database and read
# the throwaway one. dconf itself, which the GNOME step of the dotfiles loads,
# stays the session's. Flatpak's exports join the search path as systemd's
# generator puts them there for a session, or Flatpak warns at every change.
run:
	cargo build
	export SM_HOME="$(CURDIR)/target/home" HOME="$(CURDIR)/target/home/home" \
		XDG_DATA_HOME="$(CURDIR)/target/home/data" XDG_CONFIG_HOME="$(CURDIR)/target/home/config" \
		GSETTINGS_BACKEND=keyfile; \
	mkdir -p "$$HOME"; \
	if command -v flatpak >/dev/null; then \
		while IFS= read -r line; do export "$$line"; done < <(GIO_USE_VFS=local flatpak --print-updated-env); \
	fi; \
	target/debug/system-manager $(ARGS)

install: build
	install -Dm755 $(BIN) "$(HOME)/.local/bin/system-manager"

# The README's pictures, by hand after a visible change: the pages out of
# example data through the window's painter, as the ignored test `screenshots`
# in src/ui/app.rs takes them. VERSION is what the pages show, so the next
# release can be pictured before it exists.
screenshots:
	rm -f docs/screenshots/*.png
	SM_VERSION="$(VERSION)" cargo test --locked --lib ui::app::tests::screenshots -- --ignored --exact
	ls docs/screenshots

# docs/banner.png out of two screenshots, so after `make screenshots`, by
# banner.py of murkl/workflows checked out next to this project. Of the product,
# it reads the built-in theme's accent and the wordmark.
# Needs chromium, imagemagick and FiraCode Nerd Font.
banner:
	mkdir -p target
	accent="$$(sed -n 's/^ *accent: *"\(#[0-9A-Fa-f]*\)".*/\1/p' themes/nord.yaml)"; \
	{ printf 'accent: "%s"\nlogo: |\n' "$$accent"; sed 's/^/  /' docs/wordmark.txt; } >target/product.yaml
	python3 ../workflows/banner/banner.py --product target/product.yaml --logo scripts/desktop/logo.svg \
		--card docs/screenshots/home.png --card docs/screenshots/update.png \
		--tagline "Update, maintain and repair<br>Arch Linux from one place." --cell 7

# The repository's settings on GitHub, out of .github/settings/, by murkl/workflows
# checked out next to this project. Run by hand as an admin: a workflow may not
# change the rules it is held to.
github:
	../workflows/settings/settings.sh

clean:
	cargo clean
