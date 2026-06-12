import AVFoundation
import AudioToolbox

/// Metadata for an installed Audio Unit instrument the user can load as a voice.
struct AUInstrumentInfo: Identifiable, Equatable {
    let id = UUID()
    let name: String
    let manufacturer: String
    let componentDescription: AudioComponentDescription

    static func == (lhs: AUInstrumentInfo, rhs: AUInstrumentInfo) -> Bool {
        lhs.name == rhs.name && lhs.manufacturer == rhs.manufacturer
    }
}

/// Discovers Audio Unit *instruments* installed on the system. This is the macOS-native path
/// to third-party sounds — most free "VST" plugins ship an AU component too.
enum AudioUnitBrowser {
    static func installedInstruments() -> [AUInstrumentInfo] {
        var description = AudioComponentDescription()
        description.componentType = kAudioUnitType_MusicDevice  // instruments
        // componentSubType / Manufacturer = 0 → match all

        return AVAudioUnitComponentManager.shared()
            .components(matching: description)
            .map { component in
                AUInstrumentInfo(
                    name: component.name,
                    manufacturer: component.manufacturerName,
                    componentDescription: component.audioComponentDescription
                )
            }
            .sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }
}

/// Wraps a hosted AU instrument and sends it MIDI note events. Lives outside the AudioKit
/// `Node` graph — `AudioEngineController` attaches it to the engine's AVAudioEngine directly.
@MainActor
final class AUHostInstrument: MidiPlayable {
    let avAudioUnit: AVAudioUnit

    init(avAudioUnit: AVAudioUnit) {
        self.avAudioUnit = avAudioUnit
    }

    func noteOn(_ pitch: UInt8, velocity: UInt8) { send([0x90, pitch, velocity]) }
    func noteOff(_ pitch: UInt8) { send([0x80, pitch, 0]) }

    /// Prefer the v3 MIDI-event block (works for out-of-process AUv3); fall back to the
    /// AudioToolbox C API for in-process v2 units that don't expose the block.
    private func send(_ bytes: [UInt8]) {
        if let block = avAudioUnit.auAudioUnit.scheduleMIDIEventBlock {
            block(AUEventSampleTimeImmediate, 0, bytes.count, bytes)
        } else {
            MusicDeviceMIDIEvent(avAudioUnit.audioUnit, UInt32(bytes[0]), UInt32(bytes[1]),
                                 bytes.count > 2 ? UInt32(bytes[2]) : 0, 0)
        }
    }
}
