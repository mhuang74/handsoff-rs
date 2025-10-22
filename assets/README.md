# Assets Directory

This directory contains resources for the macOS application bundle.

## Application Icon

To add an application icon, create `AppIcon.icns` in this directory and update `Cargo.toml`:

```toml
[package.metadata.bundle]
# ... other settings ...
icon = ["assets/AppIcon.icns"]
```

### Creating an .icns file

1. Start with a 1024x1024 PNG file named `AppIcon.png`
2. Create an iconset:

```bash
mkdir AppIcon.iconset
sips -z 16 16     AppIcon.png --out AppIcon.iconset/icon_16x16.png
sips -z 32 32     AppIcon.png --out AppIcon.iconset/icon_16x16@2x.png
sips -z 32 32     AppIcon.png --out AppIcon.iconset/icon_32x32.png
sips -z 64 64     AppIcon.png --out AppIcon.iconset/icon_32x32@2x.png
sips -z 128 128   AppIcon.png --out AppIcon.iconset/icon_128x128.png
sips -z 256 256   AppIcon.png --out AppIcon.iconset/icon_128x128@2x.png
sips -z 256 256   AppIcon.png --out AppIcon.iconset/icon_256x256.png
sips -z 512 512   AppIcon.png --out AppIcon.iconset/icon_256x256@2x.png
sips -z 512 512   AppIcon.png --out AppIcon.iconset/icon_512x512.png
sips -z 1024 1024 AppIcon.png --out AppIcon.iconset/icon_512x512@2x.png
```

3. Convert to .icns:

```bash
iconutil -c icns AppIcon.iconset -o assets/AppIcon.icns
```
