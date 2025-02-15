use std::sync::Arc;

use nih_plug::{
    formatters::{self, v2s_f32_rounded},
    params::{EnumParam, FloatParam, Params},
    prelude::{FloatRange, SmoothingStyle},
    util,
};

use crate::LevelDetection;

pub const DEFAULT_THRESHOLD: f32 = -10.0;
pub const DEFAULT_RATIO: f32 = 4.0;
pub const DEFAULT_KNEE: f32 = 5.0;
pub const DEFAULT_ATTACK_TIME: f32 = 0.001;
pub const DEFAULT_RELEASE_TIME: f32 = 0.05;

#[derive(Params)]
pub struct GainParams {
    #[id = "lvldetection"]
    pub meter_type: EnumParam<LevelDetection>,
    #[id = "threshold"]
    pub threshold: FloatParam,
    /// The compression ratio as the left side of the ratio **in decibels**.
    /// For example, a ratio of `2.0` would be equivalent to a ratio of 2:1,
    /// which means that for every 2db that *the level* is above the `threshold`, 1db will pass through.
    #[id = "ratio"]
    pub ratio: FloatParam,
    /// The time it takes before the compressor starts compressing after *the level* is above the threshold.
    ///
    /// **NOTE**: The actual underlying value is the filter coefficient for the compressor, however the value is converted and displayed in (milli)seconds.
    #[id = "attack"]
    pub attack_time: FloatParam,
    /// The time it takes for the compressor to stop compressing after *the level* falls below the threshold.
    ///
    /// **NOTE**: The actual underlying value is the release filter coefficient for the compressor, however the value is converted and displayed in (milli)seconds.
    #[id = "release"]
    pub release_time: FloatParam,
    /// The knee width **in decibels**. This smooths the transition between compression and no compression around the threshold.
    /// If you'd like a *hard-knee compressor*, set this value to `0.0`.
    #[id = "knee"]
    pub knee_width: FloatParam,
    /// Modify the gain of the incoming signal ***before*** compression.
    #[id = "ingain"]
    pub input_gain: FloatParam,
    /// Modify the gain of the incoming signal ***after*** compression ***AND*** after dry/wet has been applied.
    #[id = "outgain"]
    pub output_gain: FloatParam,
    /// Blends the pre-compressed signal with the processed, compressed signal.
    /// `1.0` (100%) means that only the compressed signal will be output,
    /// while `0.0` (0%) means that essentially, no compression is applied.  
    #[id = "drywet"]
    pub dry_wet: FloatParam,
}

impl GainParams {
    pub fn new() -> Self {
        Self {
            // Persisted fields can be initialized like any other fields, and they'll keep their
            // values when restoring the plugin's state.
            meter_type: EnumParam::new("Level Detection", LevelDetection::Rms),
            // THRESHOLD
            threshold: FloatParam::new(
                "Threshold",
                DEFAULT_THRESHOLD,
                FloatRange::Skewed {
                    min: -100.0,
                    max: 5.0,
                    factor: FloatRange::skew_factor(2.25),
                },
            )
            // our threshold is already in dB land, so we don't need any conversion/formatting
            // TODO: play with smoothing style/timing
            .with_smoother(SmoothingStyle::Linear(10.0))
            .with_unit(" dB")
            // TODO:
            // create a custom formatter for -inf dB
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            // TODO:
            // do we need string_to_value..?

            // RATIO
            ratio: FloatParam::new(
                "Ratio",
                DEFAULT_RATIO, // default compression ratio of 4:1 dB
                FloatRange::Skewed {
                    min: 1.0,
                    max: 100.0,
                    factor: FloatRange::skew_factor(-1.8),
                },
            )
            .with_smoother(SmoothingStyle::Linear(10.0))
            // TODO: customize formatter
            .with_value_to_string(formatters::v2s_compression_ratio(2))
            .with_unit(" dB"),
            // ATTACK TIME
            attack_time: FloatParam::new(
                "Attack Time",
                DEFAULT_ATTACK_TIME,
                FloatRange::Skewed {
                    min: 0.0, // 0 seconds atk time, meaning the compressor takes effect instantly
                    max: 1.0,
                    factor: FloatRange::skew_factor(-2.0), // just happened to be right in the middle
                },
            )
            .with_smoother(SmoothingStyle::Linear(10.0))
            .with_value_to_string(v2s_time_formatter()),
            // RELEASE
            release_time: FloatParam::new(
                "Release Time",
                DEFAULT_RELEASE_TIME,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 5.0,
                    factor: FloatRange::skew_factor(-2.25), // kinda funky but i tried
                },
            )
            .with_smoother(SmoothingStyle::Linear(10.0))
            .with_value_to_string(v2s_time_formatter()),
            // KNEE WIDTH
            knee_width: FloatParam::new(
                "Knee Width",
                DEFAULT_KNEE,
                FloatRange::Linear {
                    min: 0.0,
                    max: 20.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(10.0))
            .with_unit(" dB")
            .with_value_to_string(v2s_f32_rounded(1)),
            // INPUT GAIN
            // basically, the exact same as this. LOL
            // https://github.com/robbert-vdh/nih-plug/blob/ffe9b61fcb0441c9d33f4413f5ebe7394637b21f/plugins/examples/gain/src/lib.rs#L67
            input_gain: FloatParam::new(
                "Input Gain",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-30.0),
                    max: util::db_to_gain(30.0),
                    // This makes the range appear as if it was linear when displaying the values as
                    // decibels
                    factor: FloatRange::gain_skew_factor(-30.0, 30.0),
                },
            )
            // Because the gain parameter is stored as linear gain instead of storing the value as
            // decibels, we need logarithmic smoothing
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            // There are many predefined formatters we can use here. If the gain was stored as
            // decibels instead of as a linear gain value, we could have also used the
            // `.with_step_size(0.1)` function to get internal rounding.
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
            // OUTPUT GAIN
            output_gain: FloatParam::new(
                "Output Gain",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-30.0),
                    max: util::db_to_gain(30.0),
                    factor: FloatRange::gain_skew_factor(-30.0, 30.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
            // DRY/WET
            dry_wet: FloatParam::new("Dry/Wet", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 }) // 1.0 default for full compressor effect
                .with_smoother(SmoothingStyle::Linear(10.0))
                .with_value_to_string(v2s_rounded_multiplied(1)),
        }
    }
}

// very slightly modified NIH-plug formatter

pub fn v2s_rounded_multiplied(digits: usize) -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    let rounding_multiplier = 10u32.pow(digits as u32) as f32;
    Arc::new(move |value| {
        let v = value * 100.0;
        // See above
        if (v * rounding_multiplier).round() / rounding_multiplier == 0.0 {
            format!("{:.digits$}%", 0.0)
        } else {
            format!("{v:.digits$}%")
        }
    })
}

pub fn v2s_time_formatter() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(move |value| {
        // time in MS
        let t = value * 1000.0;
        let mut unit = "ms";
        let mut output = t;
        if t >= 1000.0 {
            unit = "S";
            output /= 1000.0;
        }

        format!("{output:.2} {unit}")
    })
}
