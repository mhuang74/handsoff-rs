.PHONY: build bundle fix-plist sign dmg pkg clean install test check clippy

# Get version from Cargo.toml
VERSION := $(shell cargo pkgid | cut -d\# -f2 | cut -d: -f2 | cut -d@ -f2)
APP_NAME := HandsOff
# cargo-bundle creates lowercase bundle name when using --bin
BUNDLE_PATH := target/release/bundle/osx/handsoff.app
FINAL_BUNDLE_PATH := target/release/bundle/osx/$(APP_NAME).app
DIST_DIR := dist

# Build the release binary
build:
	cargo build --release

# Create the .app bundle (using tray binary for menu bar icon)
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

# Sign the app with certificate
sign: fix-plist
	codesign --force --deep --sign "Installer Signing Self-Signed" $(FINAL_BUNDLE_PATH)
	@echo "App signed successfully with certificate: Installer Signing Self-Signed"

# Create DMG installer
dmg: sign
	@mkdir -p $(DIST_DIR)
	@mkdir -p dmg-contents
	cp -r $(FINAL_BUNDLE_PATH) dmg-contents/
	ln -sf /Applications dmg-contents/Applications
	hdiutil create -volname "$(APP_NAME)" \
		-srcfolder dmg-contents \
		-ov -format UDZO \
		$(DIST_DIR)/$(APP_NAME)-v$(VERSION).dmg
	@rm -rf dmg-contents
	@echo "DMG created at $(DIST_DIR)/$(APP_NAME)-v$(VERSION).dmg"

# Create PKG installer with Launch Agent setup
pkg:
	./installer/build-pkg.sh

# Install to /Applications
install: fix-plist
	cp -r $(FINAL_BUNDLE_PATH) /Applications/
	@echo "Installed to /Applications/$(APP_NAME).app"

# Run tests
test:
	cargo test

# Run cargo check
check:
	cargo check

# Run clippy
clippy:
	cargo clippy

# Clean build artifacts
clean:
	cargo clean
	rm -rf target/release/bundle
	rm -rf $(DIST_DIR)
	rm -rf dmg-contents
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
	@echo "  build      - Build the release binary"
	@echo "  bundle     - Create the .app bundle"
	@echo "  fix-plist  - Create bundle and fix Info.plist (add LSUIElement)"
	@echo "  sign       - Build, bundle, fix, and sign the app"
	@echo "  dmg        - Build, bundle, fix, sign, and create DMG installer"
	@echo "  pkg        - Build and create .pkg installer with Launch Agent setup"
	@echo "  install    - Build, bundle, fix, and install to /Applications"
	@echo "  test       - Run cargo tests"
	@echo "  check      - Run cargo check"
	@echo "  clippy     - Run cargo clippy"
	@echo "  clean      - Remove build artifacts"
	@echo "  all        - Build and create fixed bundle (default)"
	@echo "  help       - Show this help message"
