# Recipes are bash and stop at the first failing command, in pipes too.
SHELL       := bash
.SHELLFLAGS := -eu -o pipefail -c

BIN := target/release/system-manager
# The version in Cargo.toml: the next release, with -dev until a tag stamps it.
CARGO_VERSION := $(shell sed -n '0,/^version = /s/^version = "\(.*\)"/\1/p' Cargo.toml)
# The release a tag names, without its v; set by the release run.
VERSION ?=

.PHONY: all build check fmt fmt-check lint test aur-check screenshots run install release clean

all: build

# What ships, read back: a renamed slot would leave the version unstamped.
build:
	cargo build --release --locked
	@said="$$($(BIN) --version)"; [ "$$said" = "system-manager $(CARGO_VERSION)" ] \
		|| { echo "$(BIN) answers '$$said', not $(CARGO_VERSION)" >&2; exit 1; }

# The one command that has to pass before a commit.
check: fmt-check lint test aur-check build

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

lint:
	cargo clippy --all-targets --locked -- -D warnings
	shellcheck -x scripts/lib.sh scripts/*/script.sh tools/*.sh
	yamllint -s .
	msgfmt --check-format -o /dev/null locales/de.po

test:
	cargo test --locked

# The PKGBUILD as makepkg reads it, stamped with stand-ins.
aur-check:
	tools/aur.sh 0.0.0 LICENSE target/aur-check

# The pictures in docs/, from example data through the window's painter.
screenshots:
	cargo test --locked --lib screenshots -- --ignored

# Against a throwaway configuration, the menu entry and the timer included; what
# it runs still runs on this machine.
run:
	SM_HOME="$(CURDIR)/target/home" XDG_DATA_HOME="$(CURDIR)/target/home/data" \
		XDG_CONFIG_HOME="$(CURDIR)/target/home/config" cargo run -- $(ARGS)

install: build
	install -Dm755 $(BIN) "$(HOME)/.local/bin/system-manager"

# The binary the checks ran on, stamped with the tag's version, its checksum and its notes.
release:
	[ "$(VERSION)-dev" = "$(CARGO_VERSION)" ] || { echo "tag $(VERSION) does not match Cargo.toml $(CARGO_VERSION)" >&2; exit 1; }
	mkdir -p dist
	cp $(BIN) dist/system-manager
	tools/stamp.sh dist/system-manager "$(VERSION)"
	cd dist && sha256sum system-manager >system-manager.sha256
	tools/notes.sh "$(VERSION)" >dist/notes.md
	tools/aur.sh "$(VERSION)" dist/source.tar.gz dist/aur

clean:
	cargo clean
	rm -rf dist
