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

# Copy the setup script into the app bundle's MacOS directory
mkdir -p "${PKG_ROOT}/Applications/${APP_NAME}.app/Contents/MacOS"
cp "${SCRIPTS_DIR}/setup-launch-agent.sh" \
   "${PKG_ROOT}/Applications/${APP_NAME}.app/Contents/MacOS/"
chmod +x "${PKG_ROOT}/Applications/${APP_NAME}.app/Contents/MacOS/setup-launch-agent.sh"

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
    <domains enable_localSystem="true"/>
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
        .warning { background: #fff3cd; border-left: 4px solid #ffc107; padding: 10px; margin: 15px 0; }
    </style>
</head>
<body>
    <h1>Welcome to HandsOff</h1>
    <p>This installer will install HandsOff, a macOS utility to block unsolicited input during video calls, presentations, or when leaving your laptop unattended.</p>

    <h2>What This Installer Does</h2>
    <ul>
        <li>Installs HandsOff.app to /Applications</li>
        <li>Includes setup script for Launch Agent configuration</li>
    </ul>

    <h2>After Installation</h2>
    <p>You will need to complete setup by running:</p>
    <p><code>/Applications/HandsOff.app/Contents/MacOS/setup-launch-agent.sh</code></p>
    <p>This setup script will:</p>
    <ul>
        <li>Prompt you for your secret passphrase</li>
        <li>Configure HandsOff to start automatically at login</li>
        <li>Launch the application</li>
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
        code { background: #f5f5f5; padding: 2px 6px; border-radius: 3px; font-family: monospace; font-size: 12px; }
        .success { background: #d4edda; border-left: 4px solid #28a745; padding: 10px; margin: 15px 0; }
        .next-steps { background: #e7f3ff; border-left: 4px solid #007bff; padding: 10px; margin: 15px 0; }
    </style>
</head>
<body>
    <h1>Installation Complete!</h1>

    <div class="success">
        HandsOff has been installed to /Applications/HandsOff.app
    </div>

    <div class="next-steps">
        <h2>Next Steps: Complete Setup</h2>
        <p>Open Terminal and run:</p>
        <p><code>/Applications/HandsOff.app/Contents/MacOS/setup-launch-agent.sh</code></p>
        <p>This will:</p>
        <ul>
            <li>Prompt you for your secret passphrase</li>
            <li>Configure automatic startup at login</li>
            <li>Launch HandsOff immediately</li>
        </ul>
    </div>

    <h2>Quick Start</h2>
    <ul>
        <li><strong>Lock:</strong> Click menu bar icon → "Lock Input" or press <code>Ctrl+Cmd+Shift+L</code></li>
        <li><strong>Unlock:</strong> Type your passphrase</li>
        <li><strong>Help:</strong> Click menu bar icon → "Help"</li>
    </ul>

    <h2>Support</h2>
    <p>Documentation: <code>/Applications/HandsOff.app/Contents/Resources/docs/</code></p>
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

productbuild \
    --distribution "${DISTRIBUTION_XML}" \
    --package-path "${INSTALLER_DIR}" \
    --resources "${INSTALLER_DIR}" \
    "${PKG_FINAL}"

echo "✓ Final package created: ${PKG_FINAL}"
echo ""

# Step 7: Verify the package
echo "Step 7: Verifying package..."
pkgutil --check-signature "${PKG_FINAL}" || echo "  (unsigned - this is expected for development)"
echo ""

# Clean up intermediate files
echo "Cleaning up intermediate files..."
rm -rf "${PKG_ROOT}"
rm -f "${PKG_COMPONENT}"
rm -f "${DISTRIBUTION_XML}"
rm -f "${INSTALLER_DIR}/welcome.html"
rm -f "${INSTALLER_DIR}/conclusion.html"
rm -f "${INSTALLER_DIR}/LICENSE"

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
echo "Or install from command line:"
echo "  sudo installer -pkg ${PKG_FINAL} -target /"
echo ""

exit 0