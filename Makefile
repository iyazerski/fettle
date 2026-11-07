VERSION ?= $(shell sed -n 's/^version = "\([^"]*\)"$$/\1/p' Cargo.toml | head -n 1)
APPS ?= /Applications

# Build Fettle.app and put it in $(APPS), replacing a running copy. Without
# FETTLE_SIGN_IDENTITY it's signed ad hoc, and macOS asks for Accessibility again.
install:
	FETTLE_VERSION=$(VERSION) cargo build --release --locked
	scripts/bundle.sh target/release/fettle $(VERSION) target
	-pkill -x fettle
	rm -rf $(APPS)/Fettle.app
	cp -R target/Fettle.app $(APPS)/Fettle.app
	open $(APPS)/Fettle.app

test:
	cargo test --locked

# Regenerate THIRD_PARTY_LICENSES.md after changing dependencies; CI checks it's current.
licenses:
	{ cargo about generate --fail about.hbs; cat assets/licenses/fonts.md; } > THIRD_PARTY_LICENSES.md
