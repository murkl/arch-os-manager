# Recipes are bash and stop at the first failing command, in pipes too.
SHELL       := bash
.SHELLFLAGS := -eu -o pipefail -c

BIN := target/release/system-manager

.PHONY: all build check fmt fmt-check lint test aur-check run install github clean

all: build

build:
	cargo build --release --locked

# The one command that has to pass before a commit.
check: fmt-check lint test aur-check build

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

lint:
	cargo clippy --all-targets --locked -- -D warnings
	shellcheck -x scripts/lib.sh scripts/*/script.sh tools/*.sh docs/*.sh .github/settings.sh .github/release-token.sh
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
# it runs still runs on this machine. GSettings goes to a file there: on the
# session's dconf it would write the real database and read the throwaway one.
run:
	SM_HOME="$(CURDIR)/target/home" XDG_DATA_HOME="$(CURDIR)/target/home/data" \
		XDG_CONFIG_HOME="$(CURDIR)/target/home/config" GSETTINGS_BACKEND=keyfile \
		cargo run -- $(ARGS)

install: build
	install -Dm755 $(BIN) "$(HOME)/.local/bin/system-manager"

# The repository's settings on GitHub, out of .github/settings/. Run by hand as
# an admin: a workflow may not change the rules it is held to.
github:
	.github/settings.sh

clean:
	cargo clean
