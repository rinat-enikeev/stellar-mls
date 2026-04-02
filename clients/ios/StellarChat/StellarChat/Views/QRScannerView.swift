import AVFoundation
import SwiftUI

/// Camera-based QR code scanner.
struct QRScannerView: View {
    let onCodeScanned: (String) -> Void

    @State private var cameraAuthStatus: AVAuthorizationStatus = AVCaptureDevice.authorizationStatus(for: .video)

    var body: some View {
        Group {
            switch cameraAuthStatus {
            case .denied, .restricted:
                cameraAccessDeniedView
            default:
                ZStack {
                    QRScannerRepresentable(onCodeScanned: onCodeScanned)
                        .ignoresSafeArea()
                    reticleOverlay
                }
                .onAppear {
                    if cameraAuthStatus == .notDetermined {
                        AVCaptureDevice.requestAccess(for: .video) { granted in
                            DispatchQueue.main.async {
                                cameraAuthStatus = AVCaptureDevice.authorizationStatus(for: .video)
                            }
                        }
                    }
                }
            }
        }
    }

    // MARK: - Camera Access Denied

    private var cameraAccessDeniedView: some View {
        VStack(spacing: 20) {
            Image(systemName: "camera.fill")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("Camera Access Required")
                .font(.title3.bold())
            Text("StellarChat needs camera access to scan QR codes. Please enable it in Settings.")
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
            Button("Open Settings") {
                if let url = URL(string: UIApplication.openSettingsURLString) {
                    UIApplication.shared.open(url)
                }
            }
            .buttonStyle(.borderedProminent)
        }
    }

    // MARK: - Reticle Overlay

    private var reticleOverlay: some View {
        GeometryReader { geometry in
            let reticleSize: CGFloat = 250
            let center = CGPoint(x: geometry.size.width / 2, y: geometry.size.height / 2)
            let reticleRect = CGRect(
                x: center.x - reticleSize / 2,
                y: center.y - reticleSize / 2,
                width: reticleSize,
                height: reticleSize
            )

            ZStack {
                // Semi-transparent dark background with clear cutout
                ReticleCutoutShape(reticleRect: reticleRect, cornerRadius: 16)
                    .fill(Color.black.opacity(0.55))
                    .ignoresSafeArea()

                // White border on the reticle
                RoundedRectangle(cornerRadius: 16)
                    .stroke(Color.white, lineWidth: 3)
                    .frame(width: reticleSize, height: reticleSize)
                    .position(center)

                // Instruction text below the reticle
                Text("Point camera at QR code")
                    .font(.callout.weight(.medium))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                    .background(.ultraThinMaterial, in: Capsule())
                    .position(x: center.x, y: reticleRect.maxY + 40)
            }
        }
        .allowsHitTesting(false)
    }
}

/// Shape that fills the entire frame except for a rounded-rect cutout.
private struct ReticleCutoutShape: Shape {
    let reticleRect: CGRect
    let cornerRadius: CGFloat

    func path(in rect: CGRect) -> Path {
        var path = Path()
        path.addRect(rect)
        path.addRoundedRect(in: reticleRect, cornerSize: CGSize(width: cornerRadius, height: cornerRadius))
        return path
    }
}

// MARK: - UIViewControllerRepresentable wrapper

private struct QRScannerRepresentable: UIViewControllerRepresentable {
    let onCodeScanned: (String) -> Void

    func makeUIViewController(context: Context) -> ScannerViewController {
        let vc = ScannerViewController()
        vc.onCodeScanned = onCodeScanned
        return vc
    }

    func updateUIViewController(_ uiViewController: ScannerViewController, context: Context) {}
}

final class ScannerViewController: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
    var onCodeScanned: ((String) -> Void)?
    private let captureSession = AVCaptureSession()
    private var hasScanned = false

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black

        guard let device = AVCaptureDevice.default(for: .video),
              let input = try? AVCaptureDeviceInput(device: device),
              captureSession.canAddInput(input)
        else { return }

        captureSession.addInput(input)

        let output = AVCaptureMetadataOutput()
        if captureSession.canAddOutput(output) {
            captureSession.addOutput(output)
            output.setMetadataObjectsDelegate(self, queue: .main)
            output.metadataObjectTypes = [.qr]
        }

        let previewLayer = AVCaptureVideoPreviewLayer(session: captureSession)
        previewLayer.frame = view.layer.bounds
        previewLayer.videoGravity = .resizeAspectFill
        view.layer.addSublayer(previewLayer)

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            self?.captureSession.startRunning()
        }
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        captureSession.stopRunning()
    }

    func metadataOutput(
        _ output: AVCaptureMetadataOutput,
        didOutput metadataObjects: [AVMetadataObject],
        from connection: AVCaptureConnection
    ) {
        guard !hasScanned,
              let object = metadataObjects.first as? AVMetadataMachineReadableCodeObject,
              let value = object.stringValue
        else { return }

        hasScanned = true
        captureSession.stopRunning()
        UIImpactFeedbackGenerator(style: .medium).impactOccurred()
        onCodeScanned?(value)
    }
}
