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
if [ -f "com.handsoff.inputlock.plist.template" ]; then
    cp "com.handsoff.inputlock.plist.template" \
       "${PKG_ROOT}/Applications/${APP_NAME}.app/Contents/Resources/"
    if [ -f "${PKG_ROOT}/Applications/${APP_NAME}.app/Contents/Resources/com.handsoff.inputlock.plist.template" ]; then
        echo "✓ Plist template copied successfully"
    else
        echo "✗ ERROR: Failed to copy plist template"
        exit 1
    fi
else
    echo "✗ ERROR: Plist template not found in project root"
    exit 1
fi

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
    <license file="EULA" />
    <conclusion file="conclusion.html" mime-type="text/html" />

    <!-- Define the component package -->
    <pkg-ref id="${BUNDLE_ID}"/>

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

# Step 5: Verify installer resources
echo "Step 5: Verifying installer resources..."

# Check for welcome.html
if [ ! -f "${INSTALLER_DIR}/welcome.html" ]; then
    echo "✗ ERROR: welcome.html not found in ${INSTALLER_DIR}/"
    exit 1
fi

# Check for conclusion.html
if [ ! -f "${INSTALLER_DIR}/conclusion.html" ]; then
    echo "✗ ERROR: conclusion.html not found in ${INSTALLER_DIR}/"
    exit 1
fi

# Copy EULA file to installer directory
if [ -f "EULA" ]; then
    cp EULA "${INSTALLER_DIR}/"
    echo "✓ EULA included"
else
    echo "✗ ERROR: EULA not found in project root"
    exit 1
fi

echo "✓ Installer resources verified"
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
rm -f "${INSTALLER_DIR}/EULA"

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