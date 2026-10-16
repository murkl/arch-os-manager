# Recipes are bash and stop at the first failing command, in pipes too.
SHELL       := bash
.SHELLFLAGS := -eu -o pipefail -c

BIN := target/release/system-manager
# The release a run builds; without it, build.rs names a pre-release of the
# next patch.
VERSION ?=
export SM_VERSION := $(VERSION)

.PHONY: all build check fmt fmt-check lint test aur-check run install clean

all: build

# What ships, read back against the version it was built for.
build:
	cargo build --release --locked
	@said="$$($(BIN) --version)"; [ -z "$(VERSION)" ] || [ "$$said" = "system-manager $(VERSION)" ] \
		|| { echo "$(BIN) answers '$$said', not $(VERSION)" >&2; exit 1; }

# The one command that has to pass before a commit.
check: fmt-check lint test aur-check build

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

lint:
	cargo clippy --all-targets --locked -- -D warnings
	shellcheck -x scripts/lib.sh scripts/*/script.sh tools/*.sh docs/*.sh
	yamllint -s .
	actionlint
	zizmor --offline --quiet .github/workflows
	msgfmt --check-format -o /dev/null locales/de.po

test:
	cargo test --locked

# The PKGBUILD as makepkg reads it, stamped with stand-ins.
aur-check:
	tools/aur.sh 0.0.0 LICENSE target/aur-check

# Against a throwaway configuration, the menu entry and the timer included; what
# it runs still runs on this machine.
run:
	SM_HOME="$(CURDIR)/target/home" XDG_DATA_HOME="$(CURDIR)/target/home/data" \
		XDG_CONFIG_HOME="$(CURDIR)/target/home/config" cargo run -- $(ARGS)

install: build
	install -Dm755 $(BIN) "$(HOME)/.local/bin/system-manager"

clean:
	cargo clean
