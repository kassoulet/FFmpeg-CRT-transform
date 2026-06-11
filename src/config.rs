//! `.cfg` parsing + derived values.
//!
//! The format matches the batch script exactly: lines are `KEY value ; comment`,
//! comments start with `;`, blank lines ignored. We keep raw string values in a
//! map and expose typed accessors so the 16 presets in `presets/` and the
//! `test-suite/` configs work unchanged. Derived values replicate the integer
//! arithmetic of `ffcrt.sh` (which uses bash `$(( ))`, i.e. truncating integer
//! division) so canvas sizes line up with the reference.

use anyhow::{anyhow, bail, Result};
use std::collections::HashMap;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frac_simple() {
        let f = parse_frac("2/3").unwrap();
        assert_eq!(f.num, 2);
        assert_eq!(f.den, 3);
    }

    #[test]
    fn parse_frac_whole() {
        let f = parse_frac("5").unwrap();
        assert_eq!(f.num, 5);
        assert_eq!(f.den, 1);
    }

    #[test]
    fn parse_frac_invalid() {
        assert!(parse_frac("abc").is_err());
        assert!(parse_frac("").is_err());
    }

    #[test]
    fn load_skips_comments_and_blanks() {
        let text = "KEY_A 1\n; comment\n\nKEY_B yes\n";
        let path = std::env::temp_dir().join("test_config.cfg");
        std::fs::write(&path, text).unwrap();
        let cfg = Config::load(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(cfg.str_or("KEY_A", ""), "1");
        assert_eq!(cfg.str_or("KEY_B", ""), "yes");
        assert_eq!(cfg.str_or("NONEXIST", "default"), "default");
    }

    #[test]
    fn yes_false_by_default() {
        let cfg = Config {
            raw: HashMap::new(),
        };
        assert!(!cfg.yes("ANYTHING"));
    }

    #[test]
    fn yes_true_for_yes() {
        let mut raw = HashMap::new();
        raw.insert("FLAG".into(), "yes".into());
        let cfg = Config { raw };
        assert!(cfg.yes("FLAG"));
        assert!(!cfg.yes("OTHER"));
    }

    #[test]
    fn i64_or_present_and_default() {
        let mut raw = HashMap::new();
        raw.insert("VAL".into(), "42".into());
        let cfg = Config { raw };
        assert_eq!(cfg.i64_or("VAL", 0), 42);
        assert_eq!(cfg.i64_or("MISSING", 99), 99);
    }

    #[test]
    fn f64_or_present() {
        let mut raw = HashMap::new();
        raw.insert("PI".into(), "3.14".into());
        let cfg = Config { raw };
        assert!((cfg.f64_or("PI", 0.0) - 3.14).abs() < 1e-9);
    }

    #[test]
    fn scan_factor_values() {
        assert_eq!(ScanFactor::Single.factor(), 1.0);
        assert_eq!(ScanFactor::Double.factor(), 2.0);
        assert_eq!(ScanFactor::Half.factor(), 0.5);
        assert_eq!(ScanFactor::Single.count(480), 480);
        assert_eq!(ScanFactor::Double.count(480), 960);
        assert_eq!(ScanFactor::Half.count(481), 240);
    }

    #[test]
    fn validate_clean_config_is_empty() {
        let cfg = Config {
            raw: HashMap::new(),
        };
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn validate_prescale_zero_warns() {
        let mut raw = HashMap::new();
        raw.insert("PRESCALE_BY".into(), "0".into());
        let cfg = Config { raw };
        let w = cfg.validate();
        assert!(!w.is_empty());
        assert!(w[0].contains("PRESCALE_BY"));
    }

    #[test]
    fn validate_sl_weight_zero_warns() {
        let mut raw = HashMap::new();
        raw.insert("SL_WEIGHT".into(), "0".into());
        let cfg = Config { raw };
        let w = cfg.validate();
        assert!(w.iter().any(|s| s.contains("SL_WEIGHT")));
    }

    #[test]
    fn validate_alpha_out_of_range_warns() {
        let mut raw = HashMap::new();
        raw.insert("OVL_ALPHA".into(), "1.5".into());
        let cfg = Config { raw };
        let w = cfg.validate();
        assert!(w.iter().any(|s| s.contains("OVL_ALPHA")));
    }

    #[test]
    fn validate_unknown_ofilter_warns() {
        let mut raw = HashMap::new();
        raw.insert("OFILTER".into(), "fancyfilter".into());
        let cfg = Config { raw };
        let w = cfg.validate();
        assert!(w.iter().any(|s| s.contains("OFILTER")));
    }

    #[test]
    fn validate_known_ofilter_is_silent() {
        for name in &[
            "neighbor",
            "fast_bilinear",
            "bilinear",
            "bicubic",
            "lanczos",
            "gauss",
        ] {
            let mut raw = HashMap::new();
            raw.insert("OFILTER".into(), (*name).into());
            let cfg = Config { raw };
            assert!(cfg.validate().is_empty(), "filter {name} should not warn");
        }
    }

    #[test]
    fn derived_compute_simple() {
        let mut raw = HashMap::new();
        raw.insert("PRESCALE_BY".into(), "2".into());
        raw.insert("PX_ASPECT".into(), "1/1".into());
        raw.insert("OY".into(), "1080".into());
        let cfg = Config { raw };
        let d = Derived::compute(&cfg, 320, 200).unwrap();
        assert_eq!(d.prescale, 2);
        assert_eq!(d.sxint, 640);
        assert_eq!(d.px, 640);
        assert_eq!(d.py, 400);
        assert_eq!(d.oy, 1080);
    }

    #[test]
    fn config_to_map_contains_all_keys() {
        let mut raw = HashMap::new();
        raw.insert("OY".into(), "720".into());
        raw.insert("PRESCALE_BY".into(), "3".into());
        let cfg = Config { raw };
        let map = cfg.to_map();
        assert_eq!(map.get("OY").map(String::as_str), Some("720"));
        assert_eq!(map.get("PRESCALE_BY").map(String::as_str), Some("3"));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn derived_report_contains_key_fields() {
        let mut raw = HashMap::new();
        raw.insert("PRESCALE_BY".into(), "2".into());
        raw.insert("OY".into(), "480".into());
        raw.insert("OASPECT".into(), "4/3".into());
        let cfg = Config { raw };
        let d = Derived::compute(&cfg, 320, 240).unwrap();
        let r = d.report();
        assert!(r.contains("320×240"), "missing input dims: {r}");
        assert!(r.contains("640×480"), "missing canvas dims: {r}");
        assert!(r.contains("output_bpc"), "missing bpc line: {r}");
    }
}

pub struct Config {
    raw: HashMap<String, String>,
}

/// A `num/den` fraction such as `PX_ASPECT 5/6` or `OASPECT 4/3`.
#[derive(Clone, Copy)]
pub struct Frac {
    pub num: i64,
    pub den: i64,
}
impl Frac {
    pub fn as_f64(&self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Couldn't read config {}: {e}", path.display()))?;
        let mut raw = HashMap::new();
        for line in text.lines() {
            let line = line.trim_start();
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            // KEY value rest...  (value is the first whitespace token after key)
            let mut it = line.split_whitespace();
            let key = match it.next() {
                Some(k) => k,
                None => continue,
            };
            let value = it.next().unwrap_or("");
            // A bare key with no value (or value that is the start of a comment)
            // gets an empty string.
            let value = if value.starts_with(';') { "" } else { value };
            raw.insert(key.to_string(), value.to_string());
        }
        Ok(Config { raw })
    }

    fn opt(&self, key: &str) -> Option<&str> {
        self.raw.get(key).map(|s| s.as_str())
    }

    pub fn str_or(&self, key: &str, default: &str) -> String {
        self.opt(key).unwrap_or(default).to_string()
    }

    /// Case-insensitive yes/no flag.
    pub fn yes(&self, key: &str) -> bool {
        matches!(
            self.opt(key).map(|s| s.to_ascii_lowercase()).as_deref(),
            Some("yes")
        )
    }

    pub fn f64_or(&self, key: &str, default: f64) -> f64 {
        self.opt(key)
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    }

    pub fn i64_or(&self, key: &str, default: i64) -> i64 {
        self.opt(key)
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    }

    pub fn frac(&self, key: &str, default: Frac) -> Frac {
        match self.opt(key) {
            Some(s) => parse_frac(s).unwrap_or(default),
            None => default,
        }
    }

    /// Override or add a single config key (used by `--set` CLI flag).
    pub fn set(&mut self, key: &str, value: &str) {
        self.raw.insert(key.to_string(), value.to_string());
    }

    /// Return all key-value pairs, sorted by key.  Used by `--validate`.
    pub fn entries(&self) -> Vec<(&str, &str)> {
        let mut pairs: Vec<(&str, &str)> = self
            .raw
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        pairs.sort_by_key(|(k, _)| *k);
        pairs
    }

    /// Return all key-value pairs as an owned `BTreeMap` (C3 introspection API).
    /// Keys are in alphabetical order; values are raw strings as they appear
    /// in the `.cfg` file.
    pub fn to_map(&self) -> std::collections::BTreeMap<String, String> {
        self.raw
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Validate config values and return a list of human-readable warnings.
    /// Warnings are non-fatal: the pipeline still runs, but results may be
    /// unexpected. Returns an empty vec when everything looks sane.
    pub fn validate(&self) -> Vec<String> {
        let mut w: Vec<String> = Vec::new();

        // PRESCALE_BY must be >= 1 (0 or negative collapses the canvas to zero).
        let prescale = self.i64_or("PRESCALE_BY", 1);
        if prescale < 1 {
            w.push(format!(
                "PRESCALE_BY={prescale} is < 1; canvas will be forced to PRESCALE_BY=1"
            ));
        }

        // SL_WEIGHT is used as `1/weight` (division by zero if 0).
        if let Some(s) = self.opt("SL_WEIGHT") {
            if let Ok(v) = s.parse::<f64>() {
                if v <= 0.0 {
                    w.push(format!(
                        "SL_WEIGHT={v} is <= 0; scanline profile will use 0.001 minimum"
                    ));
                }
            }
        }

        // Opacity / alpha parameters should be in [0, 1].
        for key in &[
            "OVL_ALPHA",
            "SL_ALPHA",
            "HALATION_ALPHA",
            "P_DECAY_ALPHA",
            "PXGRID_ALPHA",
        ] {
            if let Some(s) = self.opt(key) {
                if let Ok(v) = s.parse::<f64>() {
                    if !(0.0..=1.0).contains(&v) {
                        w.push(format!(
                            "{key}={v} is outside [0, 1]; will be used as-is (may clip)"
                        ));
                    }
                }
            }
        }

        // OFILTER must be one of the known resize filter names.
        const KNOWN_FILTERS: &[&str] = &[
            "neighbor",
            "fast_bilinear",
            "bilinear",
            "bicubic",
            "lanczos",
            "gauss",
        ];
        if let Some(s) = self.opt("OFILTER") {
            if !KNOWN_FILTERS.contains(&s.to_ascii_lowercase().as_str()) {
                w.push(format!(
                    "OFILTER={s:?} is not recognised; will fall back to bicubic. \
                     Known values: {}",
                    KNOWN_FILTERS.join(", ")
                ));
            }
        }

        // VIDEO_CRF must be in [0, 51].
        if let Some(s) = self.opt("VIDEO_CRF") {
            if let Ok(v) = s.parse::<i64>() {
                if !(0..=51).contains(&v) {
                    w.push(format!("VIDEO_CRF={v} is outside [0, 51]; will be clamped"));
                }
            }
        }

        // MAX_DURATION, if present, must be positive.
        if let Some(s) = self.opt("MAX_DURATION") {
            if let Ok(v) = s.parse::<f64>() {
                if v <= 0.0 {
                    w.push(format!(
                        "MAX_DURATION={v} is <= 0; will be treated as unlimited"
                    ));
                }
            }
        }

        // FLAT_PANEL=yes conflicts with SCANLINES_ON=yes (scanlines are silently
        // suppressed) and with CRT_CURVATURE > 0 (curvature has no visible
        // effect on flat-panel output).
        if self.yes("FLAT_PANEL") {
            if self.yes("SCANLINES_ON") {
                w.push(
                    "FLAT_PANEL=yes and SCANLINES_ON=yes: scanlines are disabled \
                     for flat-panel mode"
                        .to_string(),
                );
            }
            let curv = self.f64_or("CRT_CURVATURE", 0.0);
            if curv > 0.0 {
                w.push(format!(
                    "FLAT_PANEL=yes and CRT_CURVATURE={curv}: curvature has no \
                     effect in flat-panel mode"
                ));
            }
        }

        w
    }
}

pub fn parse_frac(s: &str) -> Result<Frac> {
    if let Some((n, d)) = s.split_once('/') {
        Ok(Frac {
            num: n.trim().parse()?,
            den: d.trim().parse()?,
        })
    } else {
        Ok(Frac {
            num: s.trim().parse()?,
            den: 1,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScanFactor {
    Single,
    Double,
    Half,
}
impl ScanFactor {
    /// Multiplier applied to the prescale period (single=1, double=2, half=0.5).
    pub fn factor(&self) -> f64 {
        match self {
            ScanFactor::Single => 1.0,
            ScanFactor::Double => 2.0,
            ScanFactor::Half => 0.5,
        }
    }
    /// SL_COUNT: number of scanline rows = IY * factor (integer, matching bash).
    pub fn count(&self, iy: i64) -> i64 {
        match self {
            ScanFactor::Single => iy,
            ScanFactor::Double => iy * 2,
            ScanFactor::Half => iy / 2,
        }
    }
}

/// All derived quantities computed once up front (mirrors the top of `ffcrt.sh`).
#[allow(dead_code)] // ix/iy/sl_count kept for parity with the script's var set
pub struct Derived {
    pub ix: i64,
    pub iy: i64,
    pub prescale: i64,
    pub px_aspect: Frac,
    pub sxint: i64, // IX * PRESCALE_BY
    pub px: i64,    // IX * PRESCALE_BY * px_aspect (integer-truncated)
    pub py: i64,    // IY * PRESCALE_BY
    pub oy: i64,
    pub oaspect: Frac,
    pub ox: i64, // round(OY * OASPECT)
    pub omargin: i64,
    pub vsigma: f64,
    pub scan_factor: ScanFactor,
    pub sl_count: i64,
    pub crt_curvature: f64,
    pub bezel_curvature: f64, // max(BEZEL, CRT)
    pub flat_panel: bool,
    pub output_bpc: u8, // 8 or 16, from OFORMAT (images)
}

impl Derived {
    pub fn compute(cfg: &Config, ix: i64, iy: i64) -> Result<Self> {
        let prescale = cfg.i64_or("PRESCALE_BY", 1).max(1);
        let px_aspect = cfg.frac("PX_ASPECT", Frac { num: 1, den: 1 });
        let oy = cfg.i64_or("OY", 1080);
        let oaspect = cfg.frac("OASPECT", Frac { num: 4, den: 3 });
        let omargin = cfg.i64_or("OMARGIN", 0);

        let sxint = ix * prescale;
        // bash: $((IX * PRESCALE_BY * num / den)) — left-to-right integer math.
        let px = ix * prescale * px_aspect.num / px_aspect.den;
        let py = iy * prescale;
        let ox = (oy as f64 * oaspect.as_f64()).round() as i64;

        let v_px_blur = cfg.f64_or("V_PX_BLUR", 0.0);
        let vsigma = if v_px_blur == 0.0 {
            0.1
        } else {
            v_px_blur / 100.0 * prescale as f64
        };

        let scan_factor = match cfg
            .str_or("SCAN_FACTOR", "single")
            .to_ascii_lowercase()
            .as_str()
        {
            "double" => ScanFactor::Double,
            "half" => ScanFactor::Half,
            _ => ScanFactor::Single,
        };
        let sl_count = scan_factor.count(iy);

        let flat_panel = cfg.yes("FLAT_PANEL");
        // FLAT_PANEL forces CRT curvature off (and, in callers, scanlines +
        // overlay). The bezel-curvature max is computed *after* that override,
        // matching the order in ffcrt.sh.
        let crt_curvature = if flat_panel {
            0.0
        } else {
            cfg.f64_or("CRT_CURVATURE", 0.0)
        };
        let mut bezel_curvature = cfg.f64_or("BEZEL_CURVATURE", 0.0);
        if bezel_curvature < crt_curvature {
            bezel_curvature = crt_curvature;
        }

        let output_bpc = if cfg.i64_or("OFORMAT", 0) == 1 { 16 } else { 8 };

        if px <= 0 || py <= 0 {
            bail!("Computed canvas size is non-positive (PX={px}, PY={py}); check PRESCALE_BY / PX_ASPECT");
        }

        Ok(Derived {
            ix,
            iy,
            prescale,
            px_aspect,
            sxint,
            px,
            py,
            oy,
            oaspect,
            ox,
            omargin,
            vsigma,
            scan_factor,
            sl_count,
            crt_curvature,
            bezel_curvature,
            flat_panel,
            output_bpc,
        })
    }

    /// Human-readable summary of all derived variables (C3 introspection API).
    ///
    /// Intended for embedding consumers that want to log or display the
    /// effective settings after integer-truncating arithmetic.
    pub fn report(&self) -> String {
        format!(
            "input:       {}×{}\n\
             prescale:    {} (canvas {}×{})\n\
             px_aspect:   {}/{}\n\
             sxint:       {}\n\
             output:      {}×{} (margin {})\n\
             oaspect:     {}/{}\n\
             vsigma:      {:.4}\n\
             scan_factor: {:?}  sl_count={}\n\
             crt_curve:   {:.4}  bezel_curve={:.4}\n\
             flat_panel:  {}\n\
             output_bpc:  {}",
            self.ix,
            self.iy,
            self.prescale,
            self.px,
            self.py,
            self.px_aspect.num,
            self.px_aspect.den,
            self.sxint,
            self.ox,
            self.oy,
            self.omargin,
            self.oaspect.num,
            self.oaspect.den,
            self.vsigma,
            self.scan_factor,
            self.sl_count,
            self.crt_curvature,
            self.bezel_curvature,
            self.flat_panel,
            self.output_bpc,
        )
    }
}
