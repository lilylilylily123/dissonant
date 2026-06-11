import SwiftUI
import DissonantCore

/// Visual tokens from DESIGN.md: "The 4-Track Terminal" — committed dark, mono-forward,
/// with a single amber brand color and the accessibility-first three-tier palette.
enum Theme {
    // Surfaces
    static let surface = Color(red: 0.07, green: 0.07, blue: 0.08)   // Tape Black
    static let panel   = Color(red: 0.12, green: 0.12, blue: 0.13)   // Deck Panel
    static let gridLine = Color(red: 0.18, green: 0.18, blue: 0.20)

    // Ink
    static let ink   = Color(red: 0.90, green: 0.89, blue: 0.86)     // Print Ink
    static let faded = Color(red: 0.55, green: 0.54, blue: 0.52)     // Faded Label

    // Brand
    static let brand = Color(red: 0.91, green: 0.64, blue: 0.25)     // amber phosphor

    // Note-tier palette (each tier also carries a non-color cue — see TierStyle)
    static let solid      = Color(red: 0.38, green: 0.72, blue: 0.55) // chord tone
    static let tension    = Color(red: 0.91, green: 0.64, blue: 0.25) // tension (amber)
    static let dissonance = Color(red: 0.89, green: 0.30, blue: 0.30) // dissonance (hot)

    static let mono = "Menlo"

    static func color(for tier: Tier?) -> Color {
        switch tier {
        case .chordTone:   return solid
        case .tension:     return tension
        case .dissonance:  return dissonance
        case nil:          return faded
        }
    }
}

/// Non-color cue per tier (the Three-Tier Doctrine: never color alone). Rendered as an
/// overlay on each note so red-green color blindness and small sizes still read.
enum TierCue {
    case none          // chord tone — solid, no mark
    case dot           // tension — a small inset diamond
    case hatch         // dissonance — diagonal stripes + flag

    static func cue(for tier: Tier?) -> TierCue {
        switch tier {
        case .chordTone: return .none
        case .tension:   return .dot
        case .dissonance: return .hatch
        case nil:        return .none
        }
    }
}

/// Diagonal hatch pattern used as the dissonance non-color cue.
struct HatchOverlay: View {
    var color: Color = .black.opacity(0.55)
    var body: some View {
        Canvas { ctx, size in
            let step: CGFloat = 5
            var x: CGFloat = -size.height
            while x < size.width {
                var path = Path()
                path.move(to: CGPoint(x: x, y: 0))
                path.addLine(to: CGPoint(x: x + size.height, y: size.height))
                ctx.stroke(path, with: .color(color), lineWidth: 1.2)
                x += step
            }
        }
        .allowsHitTesting(false)
    }
}
