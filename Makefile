VERSION ?= $(shell sed -n 's/^version = "\([^"]*\)"$$/\1/p' Cargo.toml | head -n 1)
APPS ?= /Applications

# Build Quill.app and put it in $(APPS), replacing a running copy. Without
# QUILL_SIGN_IDENTITY it's signed ad hoc, and macOS asks for Accessibility again.
install:
	QUILL_VERSION=$(VERSION) cargo build --release --locked
	scripts/bundle.sh target/release/quill $(VERSION) target
	-pkill -x quill
	rm -rf $(APPS)/Quill.app
	cp -R target/Quill.app $(APPS)/Quill.app
	open $(APPS)/Quill.app

test:
	cargo test --locked

# Regenerate THIRD_PARTY_LICENSES.md after changing dependencies; CI checks it's current.
licenses:
	{ cargo about generate --fail about.hbs; cat assets/licenses/fonts.md; } > THIRD_PARTY_LICENSES.md
