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

    func noteOn(_ pitch: UInt8, velocity: UInt8) {
        MusicDeviceMIDIEvent(avAudioUnit.audioUnit, 0x90, UInt32(pitch), UInt32(velocity), 0)
    }

    func noteOff(_ pitch: UInt8) {
        MusicDeviceMIDIEvent(avAudioUnit.audioUnit, 0x80, UInt32(pitch), 0, 0)
    }
}
