mod params;

use fundsp::hacker::*;
use nih_plug::prelude::*;
use params::GainParams;
use std::sync::Arc;
use typenum::{UInt, UTerm};
use util::{db_to_gain_fast, gain_to_db_fast};

// type Compressor = Binop<FrameMul<UInt<UTerm, B1>>, Pipe<Monitor, Monitor>, Pipe<Var, Follow<f64>>>;
// graph: An<Stack<Compressor, Compressor>>
struct Gain {
    // TODO:
    // use audionode?
    rms: Shared,
    peak: Shared,
    amplitude: Shared,
    graph: Box<dyn AudioUnit>,
    input_buffer: BufferArray<UInt<UInt<UTerm, typenum::B1>, typenum::B0>>,
    output_buffer: BufferArray<UInt<UInt<UTerm, typenum::B1>, typenum::B0>>,
    params: Arc<GainParams>,
}

#[derive(PartialEq, nih_plug::prelude::Enum)]
pub enum LevelDetection {
    Rms,
    Peak,
}

fn calculate_gain_reduction(gain: f32, threshold: f32, ratio: f32, knee_width: f32) -> f32 {
    // first, we need to convert our gain to decibels.
    let input_db = gain_to_db_fast(gain);

    // GAIN COMPUTER
    let reduced_db = {
        let difference = input_db - threshold;
        if 2.0 * (difference).abs() <= knee_width {
            // if we're within the knee range, use some special calculations!
            let gain_reduction = (difference + (knee_width / 2.0)).powi(2) / (2.0 * knee_width);
            input_db + (1.0 / ratio - 1.0) * gain_reduction
        } else if 2.0 * (difference) > knee_width {
            // above the knee, apply compression
            threshold + (difference / ratio)
        } else {
            // if we're below the knee/threshold
            input_db
        }
    };
    // to be totally honest, i'm not sure why this has to be done.
    let final_db = reduced_db - input_db;
    // convert back to linear space as a factor to multiply the input
    db_to_gain_fast(final_db)
}

impl Default for Gain {
    fn default() -> Self {
        let rms = shared(0.0);
        let peak = shared(0.0);
        let amplitude = shared(1.0);

        #[allow(clippy::precedence)]
        let compressor = (monitor(&peak, Meter::Peak(0.1)) >> monitor(&rms, Meter::Rms(0.1)))
            * (var(&amplitude) >> follow(0.01));

        let start_note = 0; // MIDI note number for Middle C (C4)
        let octaves = 12; // Number of octaves to cover

        // Generate MIDI note numbers for C Major scale across multiple octaves
        let mut midis = Vec::new();

        for octave in 0..octaves {
            // C Major scale: C, D, E, F, G, A, B, C
            let base_note = start_note + (octave * 12);
            midis.push(base_note); // C
            midis.push(base_note + 2); // D
            midis.push(base_note + 3); // Eb
            midis.push(base_note + 5); // F
            midis.push(base_note + 7); // G
            midis.push(base_note + 8); // A
            midis.push(base_note + 10); // B
            midis.push(base_note + 12); // Next C
        }

        // Remove duplicates to avoid repeating notes in the last octave
        midis.sort();
        midis.dedup();

        // Generate frequencies for each note in the scale
        let frequencies: Vec<f32> = midis
            .iter()
            .map(|&midi_note| midi_to_freq(midi_note as f32))
            .collect();

        // The window length, which must be a power of two and at least four,
        // determines the frequency resolution. Latency is equal to the window length.
        let window_length = 512;

        let synth = resynth::<U2, U2, _>(window_length, move |fft| {
            for channel in 0..=1 {
                for i in 0..fft.bins() {
                    for f in &frequencies {
                        let diff = (fft.frequency(i) - *f).abs();
                        let tolerance = 25.0;
                        if diff < tolerance {
                            fft.set(channel, i, fft.at(channel, i));
                        }
                    }
                }
            }
        });

        let graph = synth;

        Self {
            rms,
            peak,
            amplitude,
            graph: Box::new(graph),
            params: Arc::new(GainParams::new()),

            input_buffer: BufferArray::<U2>::new(),
            output_buffer: BufferArray::<U2>::new(),
        }
    }
}

impl Plugin for Gain {
    const NAME: &'static str = "Gain";
    const VENDOR: &'static str = "Moist Plugins GmbH";
    // You can use `env!("CARGO_PKG_HOMEPAGE")` to reference the homepage field from the
    // `Cargo.toml` file here
    const URL: &'static str = "https://youtu.be/dQw4w9WgXcQ";
    const EMAIL: &'static str = "info@example.com";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // The first audio IO layout is used as the default. The other layouts may be selected either
    // explicitly or automatically by the host or the user depending on the plugin API/backend.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),

            aux_input_ports: &[],
            aux_output_ports: &[],

            // Individual ports and the layout as a whole can be named here. By default these names
            // are generated as needed. This layout will be called 'Stereo', while the other one is
            // given the name 'Mono' based no the number of input and output channels.
            names: PortNames::const_default(),
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    // Setting this to `true` will tell the wrapper to split the buffer up into smaller blocks
    // whenever there are inter-buffer parameter changes. This way no changes to the plugin are
    // required to support sample accurate automation and the wrapper handles all of the boring
    // stuff like making sure transport and other timing information stays consistent between the
    // splits.
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    // If the plugin can send or receive SysEx messages, it can define a type to wrap around those
    // messages here. The type implements the `SysExMessage` trait, which allows conversion to and
    // from plain byte buffers.
    type SysExMessage = ();
    // More advanced plugins can use this to run expensive background tasks. See the field's
    // documentation for more information. `()` means that the plugin does not have any background
    // tasks.
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // TODO:
        // use BigBlockAdapter

        // offset is the sample offset from beginning of buffer,
        // we dont care about that here
        for (_offset, mut block) in buffer.iter_blocks(MAX_BUFFER_SIZE) {
            // write into input buffer
            for (sample_index, mut channel_samples) in block.iter_samples().enumerate() {
                for channel_index in 0..=1 {
                    let sample = *channel_samples.get_mut(channel_index).unwrap();
                    self.input_buffer
                        .buffer_mut()
                        .set_f32(channel_index, sample_index, sample);
                }
            }

            let level = match self.params.meter_type.value() {
                LevelDetection::Rms => self.rms.value(),
                LevelDetection::Peak => self.peak.value(),
            };

            let threshold = self.params.threshold.value();
            let ratio = self.params.ratio.value();
            let knee = self.params.knee_width.value();

            self.amplitude
                .set(calculate_gain_reduction(level, threshold, ratio, knee));

            self.graph.process(
                block.samples(),
                &self.input_buffer.buffer_ref(),
                &mut self.output_buffer.buffer_mut(),
            );

            // write from output buffer
            for (index, mut channel_samples) in block.iter_samples().enumerate() {
                for n in 0..=1 {
                    let sample_from_buf = self.output_buffer.buffer_ref().at_f32(n, index);
                    *channel_samples.get_mut(n).unwrap() = sample_from_buf;
                }
            }
        }

        ProcessStatus::Normal
    }

    // This can be used for cleaning up special resources like socket connections whenever the
    // plugin is deactivated. Most plugins won't need to do anything here.
    fn deactivate(&mut self) {}
}

impl ClapPlugin for Gain {
    const CLAP_ID: &'static str = "com.moist-plugins-gmbh.gain";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A smoothed gain parameter example plugin");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for Gain {
    const VST3_CLASS_ID: [u8; 16] = *b"GainMoistestPlug";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

nih_export_clap!(Gain);
nih_export_vst3!(Gain);

fn midi_to_freq(x: f32) -> f32 {
    440.0 * 2.0_f32.powf((x - 69.0) / 12.0)
}

mod tests {
    use crate::midi_to_freq;

    #[test]
    fn t() {
        assert_eq!(midi_to_freq(69.0), 440.0)
    }
}
