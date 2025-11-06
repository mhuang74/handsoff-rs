.PHONY: build bundle fix-plist sign dmg pkg clean install test check clippy

# Get version from Cargo.toml
VERSION := $(shell cargo pkgid | cut -d\# -f2 | cut -d: -f2 | cut -d@ -f2)
APP_NAME := HandsOff
# cargo-bundle creates lowercase bundle name when using --bin
BUNDLE_PATH := target/release/bundle/osx/handsoff.app
FINAL_BUNDLE_PATH := target/release/bundle/osx/$(APP_NAME).app
DIST_DIR := dist

# Build the release binary
# Intermediate target - use 'fix-plist' or 'pkg' for complete builds
build:
	cargo build --release

# Create the .app bundle (using tray binary for menu bar icon)
# Intermediate target - use 'fix-plist' or 'pkg' for complete builds
bundle: build
	cargo bundle --release --bin handsoff-tray
	@# Rename bundle to proper case if needed
	@if [ -d "$(BUNDLE_PATH)" ] && [ ! -d "$(FINAL_BUNDLE_PATH)" ]; then \
		mv "$(BUNDLE_PATH)" "$(FINAL_BUNDLE_PATH)"; \
	fi

# Fix Info.plist to add LSUIElement (menu bar only app)
# This is the primary build target - creates a working .app bundle
fix-plist: bundle
	plutil -insert LSUIElement -bool true $(FINAL_BUNDLE_PATH)/Contents/Info.plist
	@echo "Added LSUIElement to Info.plist"
	@plutil -p $(FINAL_BUNDLE_PATH)/Contents/Info.plist | grep -E "(LSUIElement|CFBundleDisplayName)"

# Create PKG installer with Launch Agent setup
# This is the distribution target - creates the installer package
pkg:
	./installer/build-pkg.sh

# Install to /Applications
# Local testing only - installs the .app bundle to /Applications
install: fix-plist
	cp -r $(FINAL_BUNDLE_PATH) /Applications/
	@echo "Installed to /Applications/$(APP_NAME).app"

# Developer tools - run these before committing
test:
	cargo test

check:
	cargo check

clippy:
	cargo clippy

# Clean build artifacts
clean:
	cargo clean
	rm -rf target/release/bundle
	rm -rf $(DIST_DIR)
	rm -rf installer/pkg-root
	rm -f installer/*.pkg
	rm -f installer/distribution.xml
	rm -f installer/*.html
	rm -f installer/LICENSE

# Build everything (bundle with fixes)
all: fix-plist

# Help target
help:
	@echo "Available targets:"
	@echo ""
	@echo "Primary targets:"
	@echo "  fix-plist  - Create .app bundle with LSUIElement fix (menu bar only)"
	@echo "  pkg        - Create .pkg installer (recommended for distribution)"
	@echo "  all        - Same as fix-plist (default)"
	@echo ""
	@echo "Developer tools:"
	@echo "  test       - Run cargo tests"
	@echo "  check      - Run cargo check"
	@echo "  clippy     - Run cargo clippy"
	@echo "  clean      - Remove build artifacts"
	@echo ""
	@echo "Intermediate targets:"
	@echo "  build      - Build release binary only"
	@echo "  bundle     - Create .app bundle (without LSUIElement fix)"
	@echo ""
	@echo "Other:"
	@echo "  install    - Install to /Applications (for local testing)"
	@echo "  help       - Show this help message"
