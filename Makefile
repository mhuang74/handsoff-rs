.PHONY: build bundle fix-plist sign dmg clean install test check clippy

# Get version from Cargo.toml
VERSION := $(shell cargo pkgid | cut -d\# -f2 | cut -d: -f2 | cut -d@ -f2)
APP_NAME := HandsOff
BUNDLE_PATH := target/release/bundle/osx/$(APP_NAME).app
DIST_DIR := dist

# Build the release binary
build:
	cargo build --release

# Create the .app bundle
bundle: build
	cargo bundle --release

# Fix Info.plist to add LSUIElement (menu bar only app)
fix-plist: bundle
	plutil -insert LSUIElement -bool true $(BUNDLE_PATH)/Contents/Info.plist
	@echo "Added LSUIElement to Info.plist"
	@plutil -p $(BUNDLE_PATH)/Contents/Info.plist | grep -E "(LSUIElement|CFBundleDisplayName)"

# Sign the app (development signing)
sign: fix-plist
	codesign --force --deep --sign - $(BUNDLE_PATH)
	@echo "App signed successfully"

# Create DMG installer
dmg: sign
	@mkdir -p $(DIST_DIR)
	@mkdir -p dmg-contents
	cp -r $(BUNDLE_PATH) dmg-contents/
	ln -sf /Applications dmg-contents/Applications
	hdiutil create -volname "$(APP_NAME)" \
		-srcfolder dmg-contents \
		-ov -format UDZO \
		$(DIST_DIR)/$(APP_NAME)-v$(VERSION).dmg
	@rm -rf dmg-contents
	@echo "DMG created at $(DIST_DIR)/$(APP_NAME)-v$(VERSION).dmg"

# Install to /Applications
install: fix-plist
	cp -r $(BUNDLE_PATH) /Applications/
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
	@echo "  install    - Build, bundle, fix, and install to /Applications"
	@echo "  test       - Run cargo tests"
	@echo "  check      - Run cargo check"
	@echo "  clippy     - Run cargo clippy"
	@echo "  clean      - Remove build artifacts"
	@echo "  all        - Build and create fixed bundle (default)"
	@echo "  help       - Show this help message"
