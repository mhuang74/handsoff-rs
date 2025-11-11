#!/bin/bash
# Build HandsOff installer package (.pkg)
# This script creates a macOS installer package with Launch Agent setup

set -e

# Configuration
APP_NAME="HandsOff"
BUNDLE_ID="com.handsoff.inputlock"
VERSION=$(cargo pkgid | cut -d# -f2 | cut -d: -f2 | cut -d@ -f2)
BUNDLE_PATH="target/release/bundle/osx/${APP_NAME}.app"
DIST_DIR="dist"
INSTALLER_DIR="installer"
SCRIPTS_DIR="${INSTALLER_DIR}/scripts"
PKG_ROOT="${INSTALLER_DIR}/pkg-root"
PKG_COMPONENT="${INSTALLER_DIR}/${APP_NAME}-component.pkg"
PKG_FINAL="${DIST_DIR}/${APP_NAME}-v${VERSION}.pkg"

echo ""
echo "========================================"
echo "  Building HandsOff Installer Package"
echo "========================================"
echo ""
echo "Version: ${VERSION}"
echo ""

# Step 0: Cleanup from previous runs
echo "Step 0: Cleaning up from previous builds..."

# Clean up root-owned bundle files from previous builds
if [ -d "${BUNDLE_PATH}" ]; then
    # Check if any files are owned by root
    if [ "$(find "${BUNDLE_PATH}" -user root 2>/dev/null | wc -l)" -gt 0 ]; then
        echo "  - Found root-owned files in bundle directory"
        echo "  - Attempting to remove with sudo (you may be prompted for password)..."
        if sudo -n rm -rf "${BUNDLE_PATH}" 2>/dev/null; then
            echo "  ✓ Removed root-owned bundle files"
        else
            echo "  ! Could not auto-remove root-owned files"
            echo "  ! Please run: sudo rm -rf ${BUNDLE_PATH}"
            exit 1
        fi
    else
        # Regular cleanup if no root-owned files
        rm -rf "${BUNDLE_PATH}" 2>/dev/null || true
    fi
fi

echo "✓ Cleanup complete"
echo ""

# Step 1: Build the app bundle
echo "Step 1: Building application bundle..."
make fix-plist
echo "✓ Bundle built at ${BUNDLE_PATH}"
echo ""

# Step 2: Prepare package root
echo "Step 2: Preparing package files..."
rm -rf "${PKG_ROOT}"
mkdir -p "${PKG_ROOT}/Applications"

# Copy the app bundle
cp -R "${BUNDLE_PATH}" "${PKG_ROOT}/Applications/"

# Copy plist template to app bundle's Resources directory
mkdir -p "${PKG_ROOT}/Applications/${APP_NAME}.app/Contents/Resources"
cp "com.handsoff.inputlock.plist.template" \
   "${PKG_ROOT}/Applications/${APP_NAME}.app/Contents/Resources/"

echo "✓ Package root prepared at ${PKG_ROOT}"
echo ""

# Step 3: Create component package
echo "Step 3: Creating component package..."
pkgbuild \
    --root "${PKG_ROOT}" \
    --identifier "${BUNDLE_ID}" \
    --version "${VERSION}" \
    --install-location "/" \
    --scripts "${SCRIPTS_DIR}" \
    "${PKG_COMPONENT}"

echo "✓ Component package created: ${PKG_COMPONENT}"
echo ""

# Step 4: Create distribution XML
echo "Step 4: Creating distribution definition..."
DISTRIBUTION_XML="${INSTALLER_DIR}/distribution.xml"

cat > "${DISTRIBUTION_XML}" << EOF
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="1">
    <title>${APP_NAME}</title>
    <organization>${BUNDLE_ID}</organization>
    <domains enable_currentUserHome="true"/>
    <options customize="never" require-scripts="false" hostArchitectures="arm64,x86_64"/>

    <!-- Define documents displayed at various steps -->
    <welcome file="welcome.html" mime-type="text/html" />
    <license file="LICENSE" />
    <conclusion file="conclusion.html" mime-type="text/html" />

    <!-- Define the component package -->
    <pkg-ref id="${BUNDLE_ID}">
        <bundle-version>
            <bundle id="${BUNDLE_ID}" CFBundleVersion="${VERSION}" path="Applications/${APP_NAME}.app" />
        </bundle-version>
    </pkg-ref>

    <choices-outline>
        <line choice="default">
            <line choice="${BUNDLE_ID}"/>
        </line>
    </choices-outline>

    <choice id="default"/>
    <choice id="${BUNDLE_ID}" visible="false">
        <pkg-ref id="${BUNDLE_ID}"/>
    </choice>

    <pkg-ref id="${BUNDLE_ID}" version="${VERSION}" onConclusion="none">${APP_NAME}-component.pkg</pkg-ref>
</installer-gui-script>
EOF

echo "✓ Distribution definition created"
echo ""

# Step 5: Create welcome and conclusion HTML
echo "Step 5: Creating installer resources..."

cat > "${INSTALLER_DIR}/welcome.html" << 'EOF'
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; font-size: 13px; line-height: 1.6; }
        h1 { font-size: 24px; font-weight: 300; margin-bottom: 10px; }
        h2 { font-size: 16px; font-weight: 500; margin-top: 20px; margin-bottom: 10px; }
        ul { margin: 10px 0; padding-left: 25px; }
        code { background: rgba(0,0,0,0.08); padding: 2px 6px; border-radius: 3px; font-family: monospace; font-size: 12px; }
        .warning {
            background: rgba(255,193,7,0.15);
            border: 1px solid rgba(255,193,7,0.5);
            border-left: 4px solid #ffc107;
            padding: 10px;
            margin: 15px 0;
        }

        @media (prefers-color-scheme: dark) {
            code { background: rgba(255,255,255,0.1); }
            .warning {
                background: rgba(255,193,7,0.15);
                border: 1px solid rgba(255,193,7,0.4);
                border-left: 4px solid #ffc107;
            }
        }
    </style>
</head>
<body>
    <h1>Welcome to HandsOff</h1>
    <p>This installer will install HandsOff, a macOS utility to block unsolicited input during video calls, presentations, or when leaving your laptop unattended.</p>

    <h2>What This Installer Does</h2>
    <ul>
        <li>Installs HandsOff.app to ~/Applications (your user Applications folder)</li>
        <li>Automatically configures Launch Agent for startup at login</li>
        <li>No administrator password required</li>
    </ul>

    <h2>After Installation</h2>
    <p>You will need to complete two steps:</p>
    <ol>
        <li>Grant Accessibility permissions in System Preferences</li>
        <li>Run the setup command to configure your passphrase:
            <br><code>~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup</code>
        </li>
    </ol>
    <p>The setup wizard will prompt you for:</p>
    <ul>
        <li>Secret passphrase (typing hidden for security)</li>
        <li>Auto-lock timeout (default: 30 seconds)</li>
        <li>Auto-unlock timeout (default: 60 seconds)</li>
    </ul>

    <div class="warning">
        <strong>Important:</strong> HandsOff requires Accessibility permissions to function. You will be prompted to grant these permissions on first launch.
    </div>
</body>
</html>
EOF

cat > "${INSTALLER_DIR}/conclusion.html" << 'EOF'
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; font-size: 13px; line-height: 1.6; }
        h1 { font-size: 24px; font-weight: 300; margin-bottom: 10px; }
        h2 { font-size: 16px; font-weight: 500; margin-top: 20px; margin-bottom: 10px; }
        h3 { font-size: 14px; font-weight: 500; margin-top: 15px; margin-bottom: 8px; }
        code { background: rgba(0,0,0,0.08); padding: 2px 6px; border-radius: 3px; font-family: monospace; font-size: 12px; }
        ol, ul { margin: 10px 0; padding-left: 25px; }
        .success {
            background: rgba(40,167,69,0.15);
            border: 1px solid rgba(40,167,69,0.5);
            border-left: 4px solid #28a745;
            padding: 10px;
            margin: 15px 0;
        }
        .next-steps {
            background: rgba(0,123,255,0.15);
            border: 1px solid rgba(0,123,255,0.5);
            border-left: 4px solid #007bff;
            padding: 10px;
            margin: 15px 0;
        }

        @media (prefers-color-scheme: dark) {
            code { background: rgba(255,255,255,0.1); }
            .success {
                background: rgba(40,167,69,0.2);
                border: 1px solid rgba(40,167,69,0.4);
                border-left: 4px solid #4caf50;
            }
            .next-steps {
                background: rgba(33,150,243,0.2);
                border: 1px solid rgba(33,150,243,0.4);
                border-left: 4px solid #2196f3;
            }
        }
    </style>
</head>
<body>
    <h1>Installation Complete!</h1>

    <div class="success">
        HandsOff has been installed to ~/Applications/HandsOff.app
    </div>

    <div class="next-steps">
        <h2>Next Steps: Complete Setup</h2>

        <h3>STEP 1: Grant Accessibility Permissions</h3>
        <p>You must grant Accessibility permissions for HandsOff to function:</p>
        <ol>
            <li>Go to: <strong>System Preferences → Security & Privacy → Privacy → Accessibility</strong></li>
            <li>Click the lock icon to make changes</li>
            <li>Click the <strong>+</strong> button and add <code>~/Applications/HandsOff.app</code></li>
            <li>Ensure the checkbox next to HandsOff is checked</li>
        </ol>

        <h3>STEP 2: Configure Your Passphrase</h3>
        <p>After granting permissions, open Terminal and run:</p>
        <p><code>~/Applications/HandsOff.app/Contents/MacOS/handsoff-tray --setup</code></p>
        <p>The setup wizard will prompt you for:</p>
        <ul>
            <li>Secret passphrase (typing hidden for security)</li>
            <li>Auto-lock timeout (default: 30 seconds)</li>
            <li>Auto-unlock timeout (default: 60 seconds)</li>
        </ul>
        <p>After setup, start the app with: <code>launchctl start com.handsoff.inputlock</code></p>
    </div>

    <h2>Quick Start</h2>
    <ul>
        <li><strong>Lock:</strong> Click menu bar icon → "Lock Input" or press <code>Ctrl+Cmd+Shift+L</code></li>
        <li><strong>Unlock:</strong> Type your passphrase</li>
        <li><strong>Help:</strong> Click menu bar icon → "Help"</li>
    </ul>

    <h2>Support</h2>
    <p>Documentation: <code>~/Applications/HandsOff.app/Contents/Resources/docs/</code></p>
</body>
</html>
EOF

# Copy LICENSE file to installer directory
if [ -f "LICENSE" ]; then
    cp LICENSE "${INSTALLER_DIR}/"
    echo "✓ License included"
fi

echo "✓ Installer resources created"
echo ""

# Step 6: Create final product package
echo "Step 6: Building final installer package..."
mkdir -p "${DIST_DIR}"

# Build unsigned package first
PKG_UNSIGNED="${INSTALLER_DIR}/${APP_NAME}-v${VERSION}-unsigned.pkg"

productbuild \
    --distribution "${DISTRIBUTION_XML}" \
    --package-path "${INSTALLER_DIR}" \
    --resources "${INSTALLER_DIR}" \
    "${PKG_UNSIGNED}"

echo "✓ Unsigned package created"

# Sign the package
echo "Step 6b: Signing installer package..."
productsign \
    --sign "Installer Signing Self-Signed" \
    "${PKG_UNSIGNED}" \
    "${PKG_FINAL}"

# Remove unsigned package
rm -f "${PKG_UNSIGNED}"

echo "✓ Final signed package created: ${PKG_FINAL}"
echo ""

# Step 7: Verify the package signature
echo "Step 7: Verifying package signature..."
pkgutil --check-signature "${PKG_FINAL}"
echo ""

# Clean up intermediate files
echo "Cleaning up intermediate files..."
rm -rf "${PKG_ROOT}"
rm -f "${PKG_COMPONENT}"
rm -f "${DISTRIBUTION_XML}"
rm -f "${INSTALLER_DIR}/welcome.html"
rm -f "${INSTALLER_DIR}/conclusion.html"
rm -f "${INSTALLER_DIR}/LICENSE"

# Clean up build artifacts to prevent bundle relocation during testing
echo ""
echo "Cleaning up build artifacts..."
echo "  (This prevents macOS bundle relocation when testing on build machine)"
if [ -d "${BUNDLE_PATH}" ]; then
    # Check if any files are owned by root and handle appropriately
    if [ "$(find "${BUNDLE_PATH}" -user root 2>/dev/null | wc -l)" -gt 0 ]; then
        echo "  - Found root-owned files, attempting removal with sudo..."
        sudo rm -rf "${BUNDLE_PATH}" 2>/dev/null || {
            echo "  ! Warning: Could not remove root-owned files"
            echo "  ! Bundle relocation may occur during testing"
        }
    else
        rm -rf "${BUNDLE_PATH}"
    fi
    echo "  ✓ Build artifacts removed"
else
    echo "  ✓ No build artifacts to clean"
fi

echo ""
echo "========================================"
echo "  Build Complete!"
echo "========================================"
echo ""
echo "Installer package created at:"
echo "  ${PKG_FINAL}"
echo ""
echo "File size: $(du -h "${PKG_FINAL}" | cut -f1)"
echo ""
echo "To test the installer:"
echo "  open ${PKG_FINAL}"
echo ""
echo "Or install from command line (no sudo required):"
echo "  installer -pkg ${PKG_FINAL} -target CurrentUserHomeDirectory"
echo ""
echo "Note: Build artifacts have been removed to ensure"
echo "      proper installation testing on this machine."
echo ""

exit 0