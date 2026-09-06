//! Independent scanner input-profile boundary.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScannerProfileTarget {
    LinearRgb,
    Pcs,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScannerProfileConfidence {
    Unknown,
    Estimated,
    Characterized,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScannerProfileRecord {
    pub profile: ScannerInputProfile,
    pub source_digest: String,
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScannerInputProfile {
    #[serde(default)]
    pub profile_id: String,
    pub manufacturer: String,
    pub model: String,
    pub driver_software: String,
    pub resolution: u32,
    pub bit_depth: u16,
    pub positive_negative_mode: String,
    pub infrared: bool,
    pub multi_exposure: bool,
    pub lamp_holder: String,
    pub file_transfer_curve: String,
    pub crop_geometry: String,
    pub input_encoding: String,
    pub icc_digest: String,
    pub profile_source: String,
    pub license: String,
    pub verified: bool,
    pub confidence: ScannerProfileConfidence,
    pub target: ScannerProfileTarget,
    pub compatible_film_class: String,
    pub matrix: [[f32; 3]; 3],
    #[serde(default)]
    pub offset: [f32; 3],
}

impl ScannerInputProfile {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("manufacturer", &self.manufacturer),
            ("model", &self.model),
            ("driver_software", &self.driver_software),
            ("positive_negative_mode", &self.positive_negative_mode),
            ("lamp_holder", &self.lamp_holder),
            ("file_transfer_curve", &self.file_transfer_curve),
            ("crop_geometry", &self.crop_geometry),
            ("profile_source", &self.profile_source),
            ("license", &self.license),
            ("input_encoding", &self.input_encoding),
            ("icc_digest", &self.icc_digest),
            ("compatible_film_class", &self.compatible_film_class),
        ] {
            if value.trim().is_empty() {
                return Err(format!("scanner_profile_{name}_missing"));
            }
        }
        if self.resolution == 0 || self.bit_depth == 0 {
            return Err("scanner_profile_dimensions_invalid".into());
        }
        if self.matrix.iter().flatten().any(|v| !v.is_finite())
            || self.offset.iter().any(|v| !v.is_finite())
        {
            return Err("scanner_profile_transform_non_finite".into());
        }
        if self.verified && self.confidence != ScannerProfileConfidence::Characterized {
            return Err("scanner_profile_verified_requires_characterized_confidence".into());
        }
        Ok(())
    }

    pub fn capability_label(&self) -> &'static str {
        if self.verified && self.confidence == ScannerProfileConfidence::Characterized {
            "Scanner Input Characterized"
        } else {
            "Scanner Input Estimate"
        }
    }

    pub fn apply_linear_rgb(&self, input: [f32; 3]) -> Result<[f32; 3], String> {
        self.validate()?;
        if input.iter().any(|v| !v.is_finite()) {
            return Err("scanner_input_non_finite".into());
        }
        Ok([
            self.matrix[0][0] * input[0]
                + self.matrix[0][1] * input[1]
                + self.matrix[0][2] * input[2]
                + self.offset[0],
            self.matrix[1][0] * input[0]
                + self.matrix[1][1] * input[1]
                + self.matrix[1][2] * input[2]
                + self.offset[1],
            self.matrix[2][0] * input[0]
                + self.matrix[2][1] * input[1]
                + self.matrix[2][2] * input[2]
                + self.offset[2],
        ])
    }

    /// Digest of the canonical configuration with the declared digest field
    /// blanked, avoiding a self-referential file hash while still making the
    /// imported declaration tamper evident.
    pub fn canonical_config_digest(&self) -> Result<String, String> {
        let mut canonical = self.clone();
        canonical.icc_digest.clear();
        let bytes = serde_json::to_vec(&canonical)
            .map_err(|error| format!("scanner_profile_serialize_failed|{error}"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

pub fn import_local_profile(
    path: impl AsRef<Path>,
) -> Result<(ScannerInputProfile, String), String> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|e| format!("scanner_profile_read_failed|{e}"))?;
    let profile: ScannerInputProfile =
        serde_json::from_slice(&bytes).map_err(|e| format!("scanner_profile_parse_failed|{e}"))?;
    profile.validate()?;
    if profile.icc_digest != profile.canonical_config_digest()? {
        return Err("scanner_profile_digest_mismatch".into());
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    Ok((profile, digest))
}

pub fn source_digest(path: impl AsRef<Path>) -> Result<String, String> {
    let bytes = std::fs::read(path.as_ref())
        .map_err(|error| format!("scanner_profile_read_failed|{error}"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn record_is_current(record: &ScannerProfileRecord) -> bool {
    source_digest(&record.source_path).ok().as_deref() == Some(record.source_digest.as_str())
        && record.profile.validate().is_ok()
        && record.profile.canonical_config_digest().ok().as_deref()
            == Some(record.profile.icc_digest.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> ScannerInputProfile {
        ScannerInputProfile {
            profile_id: String::new(),
            manufacturer: "Fixture".into(),
            model: "Scanner 1".into(),
            driver_software: "driver".into(),
            resolution: 4000,
            bit_depth: 16,
            positive_negative_mode: "negative".into(),
            infrared: false,
            multi_exposure: false,
            lamp_holder: "holder".into(),
            file_transfer_curve: "linear".into(),
            crop_geometry: "full".into(),
            input_encoding: "linear_rgb".into(),
            icc_digest: "digest".into(),
            profile_source: "local".into(),
            license: "internal".into(),
            verified: false,
            confidence: ScannerProfileConfidence::Estimated,
            target: ScannerProfileTarget::LinearRgb,
            compatible_film_class: "colour-negative".into(),
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            offset: [0.0; 3],
        }
    }
    #[test]
    fn estimate_is_not_density() {
        assert_eq!(profile().capability_label(), "Scanner Input Estimate");
    }
    #[test]
    fn invalid_verified_profile_is_rejected() {
        let mut p = profile();
        p.verified = true;
        assert!(p.validate().is_err());
    }

    #[test]
    fn local_profile_round_trip_uses_canonical_digest_and_detects_source_change() {
        let root =
            std::env::temp_dir().join(format!("nexfilm-scanner-profile-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("scanner.json");
        let mut value = profile();
        value.icc_digest = value.canonical_config_digest().unwrap();
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let (loaded, digest) = import_local_profile(&path).unwrap();
        let record = ScannerProfileRecord {
            profile: loaded,
            source_digest: digest,
            source_path: path.to_string_lossy().to_string(),
        };
        assert!(record_is_current(&record));
        assert_eq!(
            record.profile.apply_linear_rgb([0.2, 0.4, 0.6]).unwrap(),
            [0.2, 0.4, 0.6]
        );
        std::fs::write(&path, b"{}").unwrap();
        assert!(!record_is_current(&record));
        std::fs::remove_dir_all(root).unwrap();
    }
}
