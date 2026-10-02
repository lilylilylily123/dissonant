//! Scale, diatonic-chord and naming helpers. v1 covers major/natural minor with hand-rolled
//! interval math. Richer chord types and exotic scales layer on later.

use super::normalize;
use crate::model::{ChordEvent, ScaleType};
use serde::{Deserialize, Serialize};

pub const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const MAJOR_STEPS: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
const MINOR_STEPS: [i32; 7] = [0, 2, 3, 5, 7, 8, 10];

/// A suggestable chord, expressed as absolute pitch classes plus display labels.
/// Names are available but never required to use the chord (R9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordSuggestion {
    pub pitch_classes: Vec<i32>,
    pub name: String,
    pub roman_numeral: String,
}

/// A known-good progression starter, as scale degrees (0-based).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressionStarter {
    pub name: &'static str,
    pub degrees: Vec<i32>,
}

pub fn starters() -> Vec<ProgressionStarter> {
    vec![
        ProgressionStarter { name: "I–IV–V–vi", degrees: vec![0, 3, 4, 5] },
        ProgressionStarter { name: "I–V–vi–IV", degrees: vec![0, 4, 5, 3] },
        ProgressionStarter { name: "vi–IV–I–V", degrees: vec![5, 3, 0, 4] },
        ProgressionStarter { name: "ii–V–I", degrees: vec![1, 4, 0, 0] },
        ProgressionStarter { name: "I–vi–IV–V", degrees: vec![0, 5, 3, 4] },
        ProgressionStarter { name: "i–VI–III–VII", degrees: vec![0, 5, 2, 6] },
    ]
}

pub fn scale_steps(scale: ScaleType) -> &'static [i32; 7] {
    match scale {
        ScaleType::Major => &MAJOR_STEPS,
        ScaleType::Minor => &MINOR_STEPS,
    }
}

/// Display name of a pitch class (sharps).
pub fn note_name(pitch_class: i32) -> &'static str {
    NOTE_NAMES[normalize(pitch_class) as usize]
}

/// "C4"-style name for a MIDI note number (C4 = 60).
pub fn midi_name(pitch: i32) -> String {
    format!("{}{}", note_name(pitch), pitch.div_euclid(12) - 1)
}

pub fn key_name(root: i32, scale: ScaleType) -> String {
    let suffix = match scale {
        ScaleType::Major => "maj",
        ScaleType::Minor => "min",
    };
    format!("{} {}", note_name(root), suffix)
}

/// The seven pitch classes of a key's scale, in scale order starting from the root.
pub fn scale_pitch_classes(root: i32, scale: ScaleType) -> Vec<i32> {
    scale_steps(scale).iter().map(|s| normalize(root + s)).collect()
}

/// The seven diatonic triads of a key, built by stacking scale thirds.
pub fn diatonic_chords(root: i32, scale: ScaleType) -> Vec<ChordSuggestion> {
    let pcs = scale_pitch_classes(root, scale);
    (0..7)
        .map(|degree| {
            let chord: Vec<i32> = [degree, degree + 2, degree + 4].iter().map(|d| pcs[d % 7]).collect();
            let quality = triad_quality(&chord);
            ChordSuggestion {
                name: format!("{}{}", note_name(chord[0]), quality.suffix()),
                roman_numeral: quality.roman(degree),
                pitch_classes: chord,
            }
        })
        .collect()
}

/// Build a progression from scale degrees, filling `total_beats` by cycling the degrees so
/// long patterns get chords across their whole length.
pub fn progression(degrees: &[i32], root: i32, scale: ScaleType, total_beats: f64, chord_beats: f64) -> Vec<ChordEvent> {
    if degrees.is_empty() || chord_beats <= 0.0 {
        return vec![];
    }
    let diatonic = diatonic_chords(root, scale);
    let slots = ((total_beats / chord_beats).floor() as usize).max(degrees.len());
    (0..slots)
        .map(|i| {
            let degree = degrees[i % degrees.len()].rem_euclid(7) as usize;
            let c = &diatonic[degree];
            ChordEvent::new(i as f64 * chord_beats, chord_beats, c.pitch_classes.clone(), Some(c.name.clone()))
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriadQuality {
    Major,
    Minor,
    Diminished,
    Augmented,
    Other,
}

impl TriadQuality {
    pub fn suffix(self) -> &'static str {
        match self {
            TriadQuality::Major => "",
            TriadQuality::Minor => "m",
            TriadQuality::Diminished => "dim",
            TriadQuality::Augmented => "aug",
            TriadQuality::Other => "?",
        }
    }

    pub fn roman(self, degree: usize) -> String {
        const NUMERALS: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];
        let base = NUMERALS[degree % 7];
        match self {
            TriadQuality::Major => base.to_string(),
            TriadQuality::Augmented => format!("{base}+"),
            TriadQuality::Minor => base.to_lowercase(),
            TriadQuality::Diminished => format!("{}°", base.to_lowercase()),
            TriadQuality::Other => format!("{base}?"),
        }
    }
}

/// Quality of a root-position triad given as `[root, third, fifth]` pitch classes.
pub fn triad_quality(pcs: &[i32]) -> TriadQuality {
    if pcs.len() != 3 {
        return TriadQuality::Other;
    }
    quality_from_intervals(normalize(pcs[1] - pcs[0]), normalize(pcs[2] - pcs[0]))
}

fn quality_from_intervals(third: i32, fifth: i32) -> TriadQuality {
    match (third, fifth) {
        (4, 7) => TriadQuality::Major,
        (3, 7) => TriadQuality::Minor,
        (3, 6) => TriadQuality::Diminished,
        (4, 8) => TriadQuality::Augmented,
        _ => TriadQuality::Other,
    }
}

/// Best-effort name for an arbitrary set of pitch classes (the free-build editor).
/// Recognizes triads in any inversion by trying each note as the root; otherwise lists notes.
pub fn chord_name(pitch_classes: &[i32]) -> String {
    let mut pcs: Vec<i32> = pitch_classes.iter().map(|&p| normalize(p)).collect();
    pcs.sort_unstable();
    pcs.dedup();
    if pcs.is_empty() {
        return "—".to_string();
    }
    if pcs.len() == 3 {
        for &root in &pcs {
            let mut intervals: Vec<i32> = pcs.iter().map(|&p| normalize(p - root)).collect();
            intervals.sort_unstable();
            let q = quality_from_intervals(intervals[1], intervals[2]);
            if q != TriadQuality::Other {
                return format!("{}{}", note_name(root), q.suffix());
            }
        }
    }
    pcs.iter().map(|&p| note_name(p)).collect::<Vec<_>>().join("·")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_major_scale() {
        assert_eq!(scale_pitch_classes(0, ScaleType::Major), vec![0, 2, 4, 5, 7, 9, 11]);
        assert_eq!(scale_pitch_classes(9, ScaleType::Minor), vec![9, 11, 0, 2, 4, 5, 7]);
    }

    #[test]
    fn diatonic_chords_of_c_major() {
        let chords = diatonic_chords(0, ScaleType::Major);
        let names: Vec<&str> = chords.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["C", "Dm", "Em", "F", "G", "Am", "Bdim"]);
        let romans: Vec<&str> = chords.iter().map(|c| c.roman_numeral.as_str()).collect();
        assert_eq!(romans, vec!["I", "ii", "iii", "IV", "V", "vi", "vii°"]);
    }

    #[test]
    fn diatonic_chords_of_a_minor() {
        let chords = diatonic_chords(9, ScaleType::Minor);
        let names: Vec<&str> = chords.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Am", "Bdim", "C", "Dm", "Em", "F", "G"]);
    }

    #[test]
    fn names_and_inversions() {
        assert_eq!(chord_name(&[0, 4, 7]), "C");
        assert_eq!(chord_name(&[4, 7, 0]), "C");
        assert_eq!(chord_name(&[9, 0, 4]), "Am");
        assert_eq!(chord_name(&[0, 1, 2]), "C·C#·D");
        assert_eq!(chord_name(&[]), "—");
        assert_eq!(midi_name(60), "C4");
        assert_eq!(midi_name(24), "C1");
        assert_eq!(key_name(0, ScaleType::Major), "C maj");
    }

    #[test]
    fn progression_fills_pattern_and_follows_key() {
        let chords = progression(&[0, 3, 4, 5], 0, ScaleType::Major, 32.0, 4.0);
        assert_eq!(chords.len(), 8);
        assert_eq!(chords[0].name.as_deref(), Some("C"));
        assert_eq!(chords[5].name.as_deref(), Some("F"));
        assert_eq!(chords[7].start_beat, 28.0);

        let g = progression(&[0, 3, 4, 5], 7, ScaleType::Major, 16.0, 4.0);
        let names: Vec<&str> = g.iter().map(|c| c.name.as_deref().unwrap()).collect();
        assert_eq!(names, vec!["G", "C", "D", "Em"]);
    }

    #[test]
    fn progression_never_shorter_than_degrees() {
        let chords = progression(&[0, 3, 4, 5], 0, ScaleType::Major, 4.0, 4.0);
        assert_eq!(chords.len(), 4);
        assert!(progression(&[], 0, ScaleType::Major, 16.0, 4.0).is_empty());
    }
}
