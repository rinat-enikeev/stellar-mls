// Render the Onym Ceremony Tool app icon to a macOS .iconset directory.
//
//   swift make-iconset.swift <output-iconset-dir>
//
// Produces icon_{16,32,128,256,512}x{…}.png and @2x variants, ready for
//   iconutil -c icns <output-iconset-dir> -o AppIcon.icns
//
// The source artwork is the shared logo at
// `clients/assets/app-icon/source.png`, resolved relative to this script.
// Each iconset size is rendered as the source centered on a solid-white
// Big Sur+ squircle body — matching the iOS and Android launchers.

import Foundation
import CoreGraphics
import AppKit
import ImageIO
import UniformTypeIdentifiers

// Superellipse ("squircle") path — the rounded-square shape used by macOS
// Big Sur+ icons. Centered on (cx, cy) with half-side `r`. n=5 gives a
// pleasing curve between a circle (n=2) and a rounded rect.
func squirclePath(cx: CGFloat, cy: CGFloat, r: CGFloat, n: CGFloat = 5) -> CGPath {
    let path = CGMutablePath()
    let steps = 720
    for i in 0...steps {
        let t = CGFloat(i) / CGFloat(steps) * .pi * 2
        let c = cos(t), s = sin(t)
        let x = cx + copysign(pow(abs(c), 2/n), c) * r
        let y = cy + copysign(pow(abs(s), 2/n), s) * r
        if i == 0 { path.move(to: CGPoint(x: x, y: y)) }
        else      { path.addLine(to: CGPoint(x: x, y: y)) }
    }
    path.closeSubpath()
    return path
}

func loadSource(at url: URL) -> CGImage {
    guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
          let img = CGImageSourceCreateImageAtIndex(src, 0, nil) else {
        fatalError("failed to load source image at \(url.path)")
    }
    return img
}

func drawIcon(into ctx: CGContext, size: CGFloat, source: CGImage) {
    let side: CGFloat = 1024
    ctx.saveGState()
    ctx.scaleBy(x: size / side, y: size / side)

    // Squircle body with 8% margin on each side (Big Sur+ guideline).
    let bodyR: CGFloat = (side - 2 * 83.2) / 2
    let body = squirclePath(cx: 512, cy: 512, r: bodyR, n: 5)

    ctx.addPath(body)
    ctx.clip()

    // Solid white background matches the Android launcher + website icon.
    ctx.setFillColor(CGColor(srgbRed: 1, green: 1, blue: 1, alpha: 1))
    ctx.fill(CGRect(x: 0, y: 0, width: side, height: side))

    // Draw the source image full-bleed into the squircle.
    ctx.draw(source, in: CGRect(x: 0, y: 0, width: side, height: side))

    ctx.restoreGState()
}

func writePNG(size: Int, source: CGImage, to url: URL) throws {
    let s = CGFloat(size)
    let space = CGColorSpaceCreateDeviceRGB()
    guard let ctx = CGContext(
        data: nil,
        width: size, height: size,
        bitsPerComponent: 8,
        bytesPerRow: 0,
        space: space,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else { fatalError("CGContext init failed at size \(size)") }

    ctx.clear(CGRect(x: 0, y: 0, width: s, height: s))
    drawIcon(into: ctx, size: s, source: source)

    guard let img = ctx.makeImage() else { fatalError("makeImage failed") }
    guard let dest = CGImageDestinationCreateWithURL(
        url as CFURL, UTType.png.identifier as CFString, 1, nil
    ) else { fatalError("CGImageDestination init failed for \(url.path)") }
    CGImageDestinationAddImage(dest, img, nil)
    guard CGImageDestinationFinalize(dest) else { fatalError("finalize failed") }
}

let args = CommandLine.arguments
guard args.count == 2 else {
    FileHandle.standardError.write(Data("usage: \(args[0]) <output-iconset-dir>\n".utf8))
    exit(2)
}
let outDir = URL(fileURLWithPath: args[1])
try? FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

// Resolve clients/assets/app-icon/source.png relative to this script:
//   clients/mac-ceremony/assets/make-iconset.swift
//   → clients/assets/app-icon/source.png
let scriptURL = URL(fileURLWithPath: args[0])
let sourceURL = scriptURL
    .deletingLastPathComponent()   // clients/mac-ceremony/assets
    .deletingLastPathComponent()   // clients/mac-ceremony
    .deletingLastPathComponent()   // clients
    .appendingPathComponent("assets/app-icon/source.png")
let source = loadSource(at: sourceURL)

let sizes: [(String, Int)] = [
    ("icon_16x16.png",       16),
    ("icon_16x16@2x.png",    32),
    ("icon_32x32.png",       32),
    ("icon_32x32@2x.png",    64),
    ("icon_128x128.png",    128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png",    256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png",    512),
    ("icon_512x512@2x.png", 1024),
]

for (name, px) in sizes {
    let u = outDir.appendingPathComponent(name)
    try writePNG(size: px, source: source, to: u)
    FileHandle.standardError.write(Data("wrote \(u.path) (\(px)x\(px))\n".utf8))
}
