import SwiftUI
import UIKit

@main
struct PTPProbeApp: App {
    @StateObject private var probe = Probe()

    var body: some Scene {
        WindowGroup {
            ContentView(probe: probe)
        }
    }
}

struct ContentView: View {
    @ObservedObject var probe: Probe

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(probe.cameraName ?? "No camera").font(.headline)
                Text(probe.status).font(.subheadline)
                Text("USB mode: \(probe.mode)").font(.subheadline).foregroundStyle(.secondary)
            }

            HStack {
                Button("Connect") { probe.connect() }
                    .disabled(probe.cameraName == nil || probe.sessionOpen)
                Button("Disconnect") { probe.disconnect() }
                    .disabled(!probe.sessionOpen)
            }
            .buttonStyle(.bordered)

            LazyVGrid(columns: [GridItem(.adaptive(minimum: 150))], alignment: .leading) {
                step("Device info") { await probe.deviceInfo() }
                step("Read state") { await probe.readState() }
                step("Describe D381") { await probe.describeD381() }
                step("Write test") { await probe.writeTest() }
                step("Recipe read") { await probe.recipeRead() }
                step("Run all") { await probe.runAll() }
            }

            HStack {
                Button("Copy log") { UIPasteboard.general.string = probe.log }
                ShareLink("Share log", item: probe.log)
                Spacer()
                Button("Clear", role: .destructive) { probe.clear() }
            }
            .buttonStyle(.bordered)

            ScrollViewReader { proxy in
                ScrollView {
                    Text(probe.log)
                        .font(.system(.caption, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Color.clear.frame(height: 1).id("end")
                }
                .onChange(of: probe.log) { proxy.scrollTo("end", anchor: .bottom) }
            }
            .background(Color(.secondarySystemBackground))
        }
        .padding()
    }

    private func step(_ title: String, _ action: @escaping () async -> Void) -> some View {
        Button(title) { probe.run(action) }
            .buttonStyle(.borderedProminent)
            .disabled(!probe.sessionOpen || probe.busy)
    }
}
