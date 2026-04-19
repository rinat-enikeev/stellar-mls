import SwiftUI

struct SettingsIconBox: View {
    let systemImage: String
    let background: Color
    var size: CGFloat = 30
    var imageWeight: Font.Weight = .semibold
    var imageScale: Image.Scale = .small

    var body: some View {
        RoundedRectangle(cornerRadius: size >= 30 ? 7 : 6, style: .continuous)
            .fill(background)
            .frame(width: size, height: size)
            .overlay(
                Image(systemName: systemImage)
                    .font(.system(size: size * 0.52, weight: imageWeight))
                    .imageScale(imageScale)
                    .foregroundStyle(.white)
            )
    }
}

#Preview {
    VStack(spacing: 12) {
        SettingsIconBox(systemImage: "antenna.radiowaves.left.and.right", background: .orange)
        SettingsIconBox(systemImage: "photo.on.rectangle.angled", background: .purple)
        SettingsIconBox(systemImage: "network", background: .cyan)
        SettingsIconBox(systemImage: "star.fill", background: .yellow)
        SettingsIconBox(systemImage: "person.3.fill", background: .green)
        SettingsIconBox(systemImage: "gearshape.fill", background: .gray)
        SettingsIconBox(systemImage: "info.circle.fill", background: .gray)
        SettingsIconBox(systemImage: "lock.shield.fill", background: .blue)
    }
    .padding()
}
