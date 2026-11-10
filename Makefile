VERSION ?= $(shell sed -n 's/^version = "\([^"]*\)"$$/\1/p' Cargo.toml | head -n 1)
APPS ?= /Applications
DEV_LOG = $(CURDIR)/target/debug/fettle-dev.log

# Build Fettle.app and put it in $(APPS), replacing a running copy. Without
# FETTLE_SIGN_IDENTITY it's signed ad hoc, and macOS asks for Accessibility again.
install:
	FETTLE_VERSION=$(VERSION) cargo build --release --locked
	scripts/bundle.sh target/release/fettle $(VERSION) target
	-pkill -x fettle
	rm -rf $(APPS)/Fettle.app
	cp -R target/Fettle.app $(APPS)/Fettle.app
	open $(APPS)/Fettle.app

# Build Fettle Dev.app from a debug build and (re)start it beside the installed
# Fettle, with its own settings, sign-in and shortcut (⌃⌥⇧E). It's signed with a
# local self-signed identity, so macOS keeps its Accessibility grant across builds.
dev:
	cargo build --locked
	keychain="$$(scripts/dev-identity.sh)" && \
		FETTLE_DEV=1 FETTLE_SIGN_IDENTITY="Fettle Dev" FETTLE_SIGN_KEYCHAIN="$$keychain" \
		scripts/bundle.sh target/debug/fettle $(VERSION) target/debug
	-pkill -x fettle-dev
	for _ in $$(seq 50); do pgrep -qx fettle-dev || break; sleep 0.1; done
	open --stdout "$(DEV_LOG)" --stderr "$(DEV_LOG)" "target/debug/Fettle Dev.app"

test:
	cargo test --locked

# Regenerate THIRD_PARTY_LICENSES.md after changing dependencies; CI checks it's current.
licenses:
	{ cargo about generate --fail about.hbs; cat assets/licenses/fonts.md; } > THIRD_PARTY_LICENSES.md
