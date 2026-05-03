import SwiftUI

// MARK: - Design tokens

enum OnymTokens {
    static let bg       = Color(red: 0.949, green: 0.949, blue: 0.957)   // #F2F2F4
    static let card     = Color.white
    static let card2    = Color(red: 0.969, green: 0.969, blue: 0.976)   // #F7F7F9
    static let hairline = Color(red: 60/255, green: 60/255, blue: 67/255).opacity(0.12)
    static let text     = Color(red: 10/255, green: 10/255, blue: 12/255)
    static let text2    = Color(red: 60/255, green: 60/255, blue: 67/255).opacity(0.62)
    static let text3    = Color(red: 60/255, green: 60/255, blue: 67/255).opacity(0.42)
    static let text4    = Color(red: 60/255, green: 60/255, blue: 67/255).opacity(0.28)
    static let blue     = Color(red: 10/255, green: 132/255, blue: 255/255) // #0A84FF
    static let green    = Color(red: 48/255, green: 180/255, blue: 90/255)  // #30B45A
    static let amber    = Color(red: 255/255, green: 149/255, blue: 0/255)  // #FF9500
    static let red     = Color(red: 229/255, green: 57/255, blue: 46/255)   // #E5392E
    static let purple   = Color(red: 160/255, green: 76/255, blue: 224/255) // #A04CE0

    enum Tile {
        static let purple  = Color(red: 160/255, green: 76/255, blue: 224/255)
        static let blue    = Color(red: 10/255,  green: 132/255, blue: 255/255)
        static let indigo  = Color(red: 91/255,  green: 91/255,  blue: 226/255)
        static let orange  = Color(red: 255/255, green: 122/255, blue: 45/255)
        static let green   = Color(red: 48/255,  green: 180/255, blue: 90/255)
        static let gray    = Color(red: 142/255, green: 142/255, blue: 147/255)
        static let red     = Color(red: 229/255, green: 57/255,  blue: 46/255)
        static let teal    = Color(red: 43/255,  green: 179/255, blue: 207/255)
    }
}

// MARK: - Onym mark (broken-ring brand glyph)

struct OnymMark: View {
    var size: CGFloat = 28
    var color: Color = OnymTokens.text
    var strokeRatio: CGFloat = 0.18
    var spin: Bool = false

    @State private var rotation: Double = 0

    var body: some View {
        let sw = 100 * strokeRatio
        Canvas { ctx, _ in
            let rect = CGRect(x: sw / 2, y: sw / 2, width: 100 - sw, height: 100 - sw)
            var path = Path(ellipseIn: rect)
            // Approximate dash pattern from the design (4 long arcs separated by 4 small gaps)
            let circumference = .pi * (100 - sw)
            let lung = circumference * 0.46
            let kurz = circumference * 0.04
            path = path.strokedPath(StrokeStyle(
                lineWidth: sw,
                lineCap: .butt,
                dash: [lung, kurz, lung, kurz],
                dashPhase: -circumference * 0.8333
            ))
            ctx.fill(path, with: .color(color))
        }
        .frame(width: size, height: size)
        .rotationEffect(.degrees(rotation))
        .onAppear {
            guard spin else { return }
            withAnimation(.linear(duration: 4.2).repeatForever(autoreverses: false)) {
                rotation = 360
            }
        }
    }
}

// MARK: - Apple-Settings-style square icon tile

struct OnymTile<Content: View>: View {
    let bg: Color
    var size: CGFloat = 30
    var radius: CGFloat = 8
    @ViewBuilder var content: () -> Content

    var body: some View {
        RoundedRectangle(cornerRadius: radius, style: .continuous)
            .fill(bg)
            .frame(width: size, height: size)
            .overlay(content().foregroundStyle(.white))
            .overlay(
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .stroke(Color.white.opacity(0.18), lineWidth: 0.5)
            )
    }
}

// MARK: - SF-Symbol shortcut filled into a tile

struct OnymSymbolTile: View {
    let symbol: String
    let bg: Color
    var size: CGFloat = 30
    var weight: Font.Weight = .semibold

    var body: some View {
        OnymTile(bg: bg, size: size) {
            Image(systemName: symbol)
                .font(.system(size: size * 0.5, weight: weight))
                .foregroundStyle(.white)
        }
    }
}

// MARK: - Card surface

struct OnymCard<Content: View>: View {
    @ViewBuilder var content: () -> Content
    var body: some View {
        VStack(spacing: 0) { content() }
            .background(OnymTokens.card, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .padding(.horizontal, 16)
    }
}

// MARK: - Section label & footnote

struct OnymSectionLabel: View {
    let text: String
    var trailing: AnyView? = nil
    var body: some View {
        HStack(alignment: .bottom) {
            Text(text)
                .font(.system(size: 12.5, weight: .medium))
                .foregroundStyle(OnymTokens.text2)
                .tracking(-0.07)
            Spacer()
            if let trailing { trailing }
        }
        .padding(.horizontal, 20)
        .padding(.top, 20)
        .padding(.bottom, 8)
    }
}

struct OnymFootnote: View {
    let text: String
    var body: some View {
        Text(text)
            .font(.system(size: 12.5))
            .foregroundStyle(OnymTokens.text2)
            .lineSpacing(2)
            .padding(.horizontal, 20)
            .padding(.top, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct OnymLargeTitle: View {
    let text: String
    var body: some View {
        Text(text)
            .font(.system(size: 34, weight: .bold))
            .foregroundStyle(OnymTokens.text)
            .tracking(-0.75)
            .padding(.horizontal, 20)
            .padding(.top, 4)
            .padding(.bottom, 12)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

// MARK: - Row with optional left tile, title, subtitle, accessory and chevron

struct OnymRow<Tile: View, Right: View, Accessory: View>: View {
    @ViewBuilder var tile: () -> Tile
    let title: String
    var titleMono: Bool = false
    var subtitle: String? = nil
    var subtitleMono: Bool = false
    var danger: Bool = false
    var hasChevron: Bool = true
    var inset: CGFloat = 60
    var last: Bool = false
    var onTap: (() -> Void)? = nil
    @ViewBuilder var right: () -> Right
    @ViewBuilder var accessory: () -> Accessory

    var body: some View {
        Button(action: { onTap?() }) {
            VStack(spacing: 0) {
                HStack(spacing: 12) {
                    tile()
                    VStack(alignment: .leading, spacing: 1) {
                        Text(title)
                            .font(.system(size: 16.5, design: titleMono ? .monospaced : .default))
                            .foregroundStyle(danger ? OnymTokens.red : OnymTokens.text)
                            .tracking(-0.16)
                            .lineLimit(1)
                        if let subtitle {
                            Text(subtitle)
                                .font(.system(size: 12.5, design: subtitleMono ? .monospaced : .default))
                                .foregroundStyle(OnymTokens.text2)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }
                    }
                    Spacer(minLength: 8)
                    right()
                    accessory()
                    if hasChevron, onTap != nil {
                        Image(systemName: "chevron.right")
                            .font(.system(size: 12, weight: .semibold))
                            .foregroundStyle(OnymTokens.text3)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 11)

                if !last {
                    Rectangle()
                        .fill(OnymTokens.hairline)
                        .frame(height: 0.5)
                        .padding(.leading, inset)
                }
            }
        }
        .buttonStyle(.plain)
        .disabled(onTap == nil)
    }
}

extension OnymRow where Right == EmptyView, Accessory == EmptyView {
    init(
        title: String,
        titleMono: Bool = false,
        subtitle: String? = nil,
        subtitleMono: Bool = false,
        danger: Bool = false,
        hasChevron: Bool = true,
        inset: CGFloat = 60,
        last: Bool = false,
        onTap: (() -> Void)? = nil,
        @ViewBuilder tile: @escaping () -> Tile
    ) {
        self.tile = tile
        self.title = title
        self.titleMono = titleMono
        self.subtitle = subtitle
        self.subtitleMono = subtitleMono
        self.danger = danger
        self.hasChevron = hasChevron
        self.inset = inset
        self.last = last
        self.onTap = onTap
        self.right = { EmptyView() }
        self.accessory = { EmptyView() }
    }
}

extension OnymRow where Accessory == EmptyView {
    init(
        title: String,
        titleMono: Bool = false,
        subtitle: String? = nil,
        subtitleMono: Bool = false,
        danger: Bool = false,
        hasChevron: Bool = true,
        inset: CGFloat = 60,
        last: Bool = false,
        onTap: (() -> Void)? = nil,
        @ViewBuilder tile: @escaping () -> Tile,
        @ViewBuilder right: @escaping () -> Right
    ) {
        self.tile = tile
        self.title = title
        self.titleMono = titleMono
        self.subtitle = subtitle
        self.subtitleMono = subtitleMono
        self.danger = danger
        self.hasChevron = hasChevron
        self.inset = inset
        self.last = last
        self.onTap = onTap
        self.right = right
        self.accessory = { EmptyView() }
    }
}

extension OnymRow where Right == EmptyView {
    init(
        title: String,
        titleMono: Bool = false,
        subtitle: String? = nil,
        subtitleMono: Bool = false,
        danger: Bool = false,
        hasChevron: Bool = true,
        inset: CGFloat = 60,
        last: Bool = false,
        onTap: (() -> Void)? = nil,
        @ViewBuilder tile: @escaping () -> Tile,
        @ViewBuilder accessory: @escaping () -> Accessory
    ) {
        self.tile = tile
        self.title = title
        self.titleMono = titleMono
        self.subtitle = subtitle
        self.subtitleMono = subtitleMono
        self.danger = danger
        self.hasChevron = hasChevron
        self.inset = inset
        self.last = last
        self.onTap = onTap
        self.right = { EmptyView() }
        self.accessory = accessory
    }
}

// MARK: - Chip badge

struct OnymChip: View {
    let text: String
    let fg: Color
    let bg: Color
    var body: some View {
        Text(text)
            .font(.system(size: 9.5, weight: .bold))
            .tracking(0.5)
            .padding(.horizontal, 6)
            .padding(.vertical, 3)
            .background(bg, in: RoundedRectangle(cornerRadius: 4))
            .foregroundStyle(fg)
    }
}

// MARK: - Switch matching iOS Settings

struct OnymSwitch: View {
    @Binding var on: Bool
    var body: some View {
        Toggle("", isOn: $on)
            .labelsHidden()
            .tint(OnymTokens.green)
    }
}

// MARK: - Primary CTA

struct OnymPrimaryButton<Label: View>: View {
    var disabled: Bool = false
    let action: () -> Void
    @ViewBuilder var label: () -> Label

    var body: some View {
        Button(action: action) {
            label()
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(.white)
                .frame(maxWidth: .infinity, minHeight: 50)
                .background(disabled
                    ? OnymTokens.blue.opacity(0.45)
                    : OnymTokens.blue,
                    in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        }
        .disabled(disabled)
    }
}

// MARK: - Custom NavBar (sticky pill back-button + centered title)

struct OnymNavBar: View {
    let title: String
    var subtitle: String? = nil
    var onBack: (() -> Void)? = nil
    var trailing: AnyView? = nil

    var body: some View {
        HStack(spacing: 0) {
            if let onBack {
                Button(action: onBack) {
                    Image(systemName: "chevron.backward")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(OnymTokens.blue)
                        .frame(width: 36, height: 36)
                        .background(Color.white, in: Circle())
                        .shadow(color: .black.opacity(0.05), radius: 1, y: 1)
                }
            } else {
                Color.clear.frame(width: 36, height: 36)
            }
            Spacer()
            VStack(spacing: 1) {
                Text(title)
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(OnymTokens.text)
                    .tracking(-0.16)
                if let subtitle {
                    Text(subtitle)
                        .font(.system(size: 11.5))
                        .foregroundStyle(OnymTokens.text2)
                }
            }
            Spacer()
            if let trailing {
                trailing.frame(width: 36, height: 36)
            } else {
                Color.clear.frame(width: 36, height: 36)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .frame(minHeight: 44)
    }
}

// MARK: - Page wrapper (sets the bg + hides system nav bar)

struct OnymPage<Content: View>: View {
    @ViewBuilder var content: () -> Content
    var body: some View {
        ZStack(alignment: .top) {
            OnymTokens.bg.ignoresSafeArea()
            ScrollView { content().padding(.bottom, 40) }
        }
        .navigationBarBackButtonHidden(true)
        #if os(iOS)
        .toolbar(.hidden, for: .navigationBar)
        #endif
    }
}

// MARK: - Identity tile (round, with broken-ring mark)

struct OnymIdentityTile: View {
    var active: Bool = false
    var size: CGFloat = 36

    var body: some View {
        Circle()
            .fill(active ? Color(red: 0.878, green: 0.933, blue: 0.996) : Color(red: 0.918, green: 0.918, blue: 0.933))
            .frame(width: size, height: size)
            .overlay(OnymMark(size: size * 0.55, color: active ? OnymTokens.blue : Color(red: 142/255, green: 142/255, blue: 147/255)))
            .overlay(Circle().stroke(active ? OnymTokens.blue : .clear, lineWidth: 1.5))
    }
}

// MARK: - Step indicator

struct OnymStepIndicator: View {
    let step: Int
    var count: Int = 3
    var body: some View {
        HStack(spacing: 6) {
            ForEach(0..<count, id: \.self) { i in
                Capsule()
                    .fill(i <= step ? OnymTokens.blue : OnymTokens.text4)
                    .frame(width: i == step ? 22 : 6, height: 6)
            }
        }
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity)
    }
}

// MARK: - QR code wrapper that styles to match the design (rounded, with a centered Onym badge)

struct OnymQRCode: View {
    let value: String
    var size: CGFloat = 220

    var body: some View {
        ZStack {
            QRCodeView(value, size: size)
            Color.white
                .frame(width: size * 0.22, height: size * 0.22)
                .overlay(OnymMark(size: size * 0.18, color: OnymTokens.text))
                .clipShape(RoundedRectangle(cornerRadius: size * 0.05, style: .continuous))
        }
        .background(.white)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
    }
}
