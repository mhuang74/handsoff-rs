.PHONY: build bundle fix-plist dmg install test check clippy clean help

# Get version from Cargo.toml
VERSION := $(shell cargo pkgid | cut -d\# -f2 | cut -d: -f2 | cut -d@ -f2)
APP_NAME := HandsOff
# cargo-bundle creates lowercase bundle name when using --bin
BUNDLE_PATH := target/release/bundle/osx/handsoff.app
FINAL_BUNDLE_PATH := target/release/bundle/osx/$(APP_NAME).app
DIST_DIR := dist
# uname -m on macOS reports arm64 / x86_64
ARCH := $(shell uname -m)
DMG_STAGING := target/release/dmg-staging
DMG_PATH := $(DIST_DIR)/$(APP_NAME)-v$(VERSION)-$(ARCH).dmg

# Build the release binary
# Intermediate target - use 'dmg' for the distributable artifact
build:
	cargo build --release

# Create the .app bundle (using tray binary for menu bar icon)
# Intermediate target - use 'dmg' for the distributable artifact
bundle: build
	cargo bundle --release --bin handsoff-tray
	@# Rename bundle to proper case if needed
	@if [ -d "$(BUNDLE_PATH)" ] && [ ! -d "$(FINAL_BUNDLE_PATH)" ]; then \
		mv "$(BUNDLE_PATH)" "$(FINAL_BUNDLE_PATH)"; \
	fi

# Fix Info.plist to add LSUIElement (menu bar only app)
fix-plist: bundle
	plutil -insert LSUIElement -bool true $(FINAL_BUNDLE_PATH)/Contents/Info.plist
	@echo "Added LSUIElement to Info.plist"
	@plutil -p $(FINAL_BUNDLE_PATH)/Contents/Info.plist | grep -E "(LSUIElement|CFBundleDisplayName)"

# Create the distributable DMG (macOS only; requires hdiutil)
# Ad-hoc signed (unsigned) app per docs/adr/0001-unsigned-notarization-free-distribution.md
dmg: fix-plist
	codesign --deep --force --sign - $(FINAL_BUNDLE_PATH)
	@echo "Ad-hoc signed $(FINAL_BUNDLE_PATH)"
	@rm -rf "$(DMG_STAGING)"
	@mkdir -p "$(DMG_STAGING)"
	cp -R "$(FINAL_BUNDLE_PATH)" "$(DMG_STAGING)/"
	ln -s /Applications "$(DMG_STAGING)/Applications"
	@mkdir -p $(DIST_DIR)
	hdiutil create -volname $(APP_NAME) -srcfolder "$(DMG_STAGING)" -ov -format UDZO "$(DMG_PATH)"
	@rm -rf "$(DMG_STAGING)"
	@echo "DMG created at: $(DMG_PATH)"

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

# Build everything (bundle with fixes)
all: fix-plist

# Help target
help:
	@echo "Available targets:"
	@echo ""
	@echo "Primary targets:"
	@echo "  fix-plist  - Create .app bundle with LSUIElement fix (menu bar only)"
	@echo "  dmg        - Create distributable DMG with ad-hoc signed .app (macOS only)"
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
