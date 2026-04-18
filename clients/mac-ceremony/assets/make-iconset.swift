// Render the Onym Ceremony Tool app icon to a macOS .iconset directory.
//
//   swift make-iconset.swift <output-iconset-dir>
//
// Produces icon_{16,32,128,256,512}x{…}.png and @2x variants, ready for
//   iconutil -c icns <output-iconset-dir> -o AppIcon.icns
//
// The renderer draws the icon directly in CoreGraphics so the build has
// no external dependencies beyond the Swift toolchain (same Xcode CLT
// we already require).

import Foundation
import CoreGraphics
import AppKit
import ImageIO
import UniformTypeIdentifiers

// -----------------------------------------------------------------------------
// Palette (matches deploy/ceremony/assets/ceremony.css).
// -----------------------------------------------------------------------------
struct Palette {
    static let bodyTop    = CGColor(srgbRed: 0x58/255.0, green: 0x47/255.0, blue: 0xd4/255.0, alpha: 1.0)
    static let bodyMid    = CGColor(srgbRed: 0x3b/255.0, green: 0x2d/255.0, blue: 0x9c/255.0, alpha: 1.0)
    static let bodyBot    = CGColor(srgbRed: 0x24/255.0, green: 0x1a/255.0, blue: 0x6e/255.0, alpha: 1.0)
    static let accent     = CGColor(srgbRed: 0x6c/255.0, green: 0x5c/255.0, blue: 0xe7/255.0, alpha: 1.0)
    static let accentGlow = CGColor(srgbRed: 0xa2/255.0, green: 0x9b/255.0, blue: 0xfe/255.0, alpha: 1.0)
    static let white      = CGColor(srgbRed: 1, green: 1, blue: 1, alpha: 1)
}

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

// -----------------------------------------------------------------------------
// Icon renderer. Everything is drawn in the canonical 1024x1024 space; the
// caller scales the context to the target size so strokes remain crisp.
// -----------------------------------------------------------------------------
func drawIcon(into ctx: CGContext, size: CGFloat) {
    let side: CGFloat = 1024
    ctx.saveGState()
    ctx.scaleBy(x: size / side, y: size / side)

    // Clip to the squircle so nothing bleeds past the body.
    let bodyR: CGFloat = (side - 2 * 83.2) / 2  // 428.8
    let body = squirclePath(cx: 512, cy: 512, r: bodyR, n: 5)
    ctx.saveGState()
    ctx.addPath(body)
    ctx.clip()

    // --- Body: top-to-bottom purple gradient ---
    let space = CGColorSpaceCreateDeviceRGB()
    let bodyGrad = CGGradient(
        colorsSpace: space,
        colors: [Palette.bodyTop, Palette.bodyMid, Palette.bodyBot] as CFArray,
        locations: [0.0, 0.55, 1.0]
    )!
    ctx.drawLinearGradient(
        bodyGrad,
        start: CGPoint(x: 512, y: 1024),
        end: CGPoint(x: 512, y: 0),
        options: []
    )

    // --- Specular highlight from the top-left ---
    let shineColors = [
        Palette.accentGlow.copy(alpha: 0.55)!,
        Palette.accentGlow.copy(alpha: 0.08)!,
        Palette.accentGlow.copy(alpha: 0.0)!,
    ]
    let shineGrad = CGGradient(
        colorsSpace: space,
        colors: shineColors as CFArray,
        locations: [0.0, 0.55, 1.0]
    )!
    ctx.drawRadialGradient(
        shineGrad,
        startCenter: CGPoint(x: 1024 * 0.28, y: 1024 * (1 - 0.24)),
        startRadius: 0,
        endCenter:   CGPoint(x: 1024 * 0.28, y: 1024 * (1 - 0.24)),
        endRadius:   1024 * 0.55,
        options: []
    )

    // --- Central glow behind the link cluster ---
    let glowColors = [
        Palette.accentGlow.copy(alpha: 0.55)!,
        Palette.accent.copy(alpha: 0.18)!,
        Palette.accent.copy(alpha: 0.0)!,
    ]
    let glow = CGGradient(
        colorsSpace: space,
        colors: glowColors as CFArray,
        locations: [0.0, 0.55, 1.0]
    )!
    ctx.drawRadialGradient(
        glow,
        startCenter: CGPoint(x: 512, y: 1024 - 520),
        startRadius: 0,
        endCenter:   CGPoint(x: 512, y: 1024 - 520),
        endRadius:   340,
        options: []
    )

    ctx.restoreGState()

    // --- Rim for small-size definition ---
    ctx.addPath(body)
    ctx.setStrokeColor(Palette.accentGlow.copy(alpha: 0.22)!)
    ctx.setLineWidth(4)
    ctx.strokePath()

    // --- Chain of three interlocked rings ---
    // Note: CG y-axis is bottom-up, so we flip the SVG y-values (540 → 484, 470 → 554).
    let ringR: CGFloat = 140
    let lw: CGFloat = 60
    ctx.setLineWidth(lw)
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)

    // We approximate the SVG's linkStroke gradient by drawing each ring with
    // a vertical linear gradient stroke: a clipped mask of the ring's stroke
    // region, then fill with a linear gradient.
    func drawRingStroked(cx: CGFloat, cy: CGFloat) {
        ctx.saveGState()
        let ring = CGMutablePath()
        ring.addArc(center: CGPoint(x: cx, y: cy), radius: ringR,
                    startAngle: 0, endAngle: .pi * 2, clockwise: false)
        // Use the stroked path as the clip, then paint a gradient.
        let stroked = ring.copy(strokingWithWidth: lw, lineCap: .round,
                                lineJoin: .round, miterLimit: 10)
        ctx.addPath(stroked)
        ctx.clip()

        let strokeColors = [
            Palette.white,
            CGColor(srgbRed: 0xd7/255, green: 0xd3/255, blue: 0xff/255, alpha: 1),
            Palette.accentGlow,
        ]
        let g = CGGradient(
            colorsSpace: space,
            colors: strokeColors as CFArray,
            locations: [0.0, 0.6, 1.0]
        )!
        ctx.drawLinearGradient(
            g,
            start: CGPoint(x: cx, y: cy + ringR + lw / 2),
            end:   CGPoint(x: cx, y: cy - ringR - lw / 2),
            options: []
        )
        ctx.restoreGState()
    }

    let yLR: CGFloat = 1024 - 540   // 484 — left and right centers
    let yM: CGFloat  = 1024 - 470   // 554 — middle (sits higher in SVG, so lower-y in SVG → higher-y in CG)

    // Left (back)
    drawRingStroked(cx: 330, cy: yLR)
    // Right (back)
    drawRingStroked(cx: 694, cy: yLR)
    // Middle (front) — painted last so it appears above the others
    drawRingStroked(cx: 512, cy: yM)

    // Reinforce the front arc of the middle ring to hide the intersection
    // with its neighbours (the interlock illusion). This keeps the chain
    // reading as three linked rings rather than three overlapping circles.
    ctx.saveGState()
    let front = CGMutablePath()
    front.addArc(center: CGPoint(x: 512, y: yM), radius: ringR,
                 startAngle: CGFloat.pi * 1.25,
                 endAngle:   CGFloat.pi * 1.75,
                 clockwise: false)
    let frontStroked = front.copy(strokingWithWidth: lw, lineCap: .round,
                                  lineJoin: .round, miterLimit: 10)
    ctx.addPath(frontStroked)
    ctx.clip()
    let strokeColors2 = [
        Palette.white,
        CGColor(srgbRed: 0xd7/255, green: 0xd3/255, blue: 0xff/255, alpha: 1),
        Palette.accentGlow,
    ]
    let g2 = CGGradient(
        colorsSpace: space,
        colors: strokeColors2 as CFArray,
        locations: [0.0, 0.6, 1.0]
    )!
    ctx.drawLinearGradient(
        g2,
        start: CGPoint(x: 512, y: yM + ringR + lw / 2),
        end:   CGPoint(x: 512, y: yM - ringR - lw / 2),
        options: []
    )
    ctx.restoreGState()

    // --- Sparkle dots — hint at randomness / toxic waste scattered in the void ---
    ctx.setFillColor(Palette.white.copy(alpha: 0.85)!)
    let sparkles: [(CGFloat, CGFloat, CGFloat)] = [
        (250, 1024 - 300, 6),
        (780, 1024 - 330, 5),
        (820, 1024 - 720, 6),
        (220, 1024 - 740, 5),
        (512, 1024 - 260, 4),
    ]
    for (x, y, r) in sparkles {
        ctx.fillEllipse(in: CGRect(x: x - r, y: y - r, width: 2 * r, height: 2 * r))
    }

    ctx.restoreGState()
}

// -----------------------------------------------------------------------------
// Write a single PNG at the requested pixel size.
// -----------------------------------------------------------------------------
func writePNG(size: Int, to url: URL) throws {
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
    drawIcon(into: ctx, size: s)

    guard let img = ctx.makeImage() else { fatalError("makeImage failed") }
    guard let dest = CGImageDestinationCreateWithURL(
        url as CFURL, UTType.png.identifier as CFString, 1, nil
    ) else { fatalError("CGImageDestination init failed for \(url.path)") }
    CGImageDestinationAddImage(dest, img, nil)
    guard CGImageDestinationFinalize(dest) else { fatalError("finalize failed") }
}

// -----------------------------------------------------------------------------
// Entry point.
// -----------------------------------------------------------------------------
let args = CommandLine.arguments
guard args.count == 2 else {
    FileHandle.standardError.write(Data("usage: \(args[0]) <output-iconset-dir>\n".utf8))
    exit(2)
}
let outDir = URL(fileURLWithPath: args[1])
try? FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

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
    try writePNG(size: px, to: u)
    FileHandle.standardError.write(Data("wrote \(u.path) (\(px)x\(px))\n".utf8))
}
