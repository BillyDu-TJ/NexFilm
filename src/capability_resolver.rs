use serde::{Deserialize, Serialize};

use crate::app_state::{
    DataDomain, DensityAnchor, DensityAnchors, PipelineProcessingReport, PipelineStageRecord,
    PipelineStageStatus, ProcessingContract, DENSITY_ANCHOR_ALGORITHM_VERSION,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PipelineImageKind {
    RawBayer,
    RawXTrans,
    DirectRgb,
    Missing,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolverProfile {
    pub profile_id: String,
    pub payload_digest: String,
    pub available: bool,
    pub capture_validation_error: Option<String>,
    pub has_dark: bool,
    pub has_open_gate: bool,
    pub has_flat: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PipelineResolverInput {
    /// Legacy is a persisted compatibility request. Every non-Legacy value is
    /// resolved from the current Roll binding instead of trusting a previous
    /// runtime result.
    pub persisted_contract: ProcessingContract,
    pub roll_profile_id: Option<String>,
    pub profile: Option<ResolverProfile>,
    pub image_kind: PipelineImageKind,
    pub density_anchors: DensityAnchors,
    pub raw_decode_version: i64,
    /// Ephemeral failure from this invocation only. It is reported and causes
    /// one fallback, but is never written back as the next request.
    pub runtime_failure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LayerCapability {
    pub layer: String,
    pub status: PipelineStageStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PipelineResolution {
    pub requested_path: ProcessingContract,
    pub resolved_path: ProcessingContract,
    pub resolved_profile_id: Option<String>,
    pub resolved_payload_digest: Option<String>,
    pub capture: LayerCapability,
    pub density: LayerCapability,
    pub film: LayerCapability,
    pub flat: LayerCapability,
    pub usable_density_anchors: DensityAnchors,
    pub rejected_anchor_reasons: Vec<String>,
    pub processing_report: PipelineProcessingReport,
}

fn capability(layer: &str, status: PipelineStageStatus, detail: &str) -> LayerCapability {
    LayerCapability {
        layer: layer.to_string(),
        status,
        detail: detail.to_string(),
    }
}

fn anchor_mismatch(
    anchor: &DensityAnchor,
    expected_domain: DataDomain,
    profile: Option<&ResolverProfile>,
    raw_decode_version: i64,
) -> Option<&'static str> {
    let provenance = &anchor.provenance;
    if provenance.input_domain != expected_domain {
        return Some("anchor_input_domain_mismatch");
    }
    if expected_domain == DataDomain::RelativeTransmissionRgb {
        let Some(profile) = profile else {
            return Some("anchor_profile_missing");
        };
        if provenance.calibration_profile_id.as_deref() != Some(profile.profile_id.as_str()) {
            return Some("anchor_profile_mismatch");
        }
        if provenance.calibration_payload_digest.as_deref() != Some(profile.payload_digest.as_str())
        {
            return Some("anchor_payload_mismatch");
        }
        if provenance.raw_decode_version != Some(raw_decode_version) {
            return Some("anchor_raw_decode_version_mismatch");
        }
        if provenance.algorithm_version != DENSITY_ANCHOR_ALGORITHM_VERSION {
            return Some("anchor_algorithm_version_mismatch");
        }
        if provenance.legacy {
            return Some("anchor_legacy_unverified");
        }
    }
    None
}

fn filter_anchors(
    anchors: &DensityAnchors,
    expected_domain: DataDomain,
    profile: Option<&ResolverProfile>,
    raw_decode_version: i64,
) -> (DensityAnchors, Vec<String>) {
    let mut usable = DensityAnchors::default();
    let mut rejected = Vec::new();
    for (name, anchor, target) in [
        (
            "d_min_base",
            anchors.d_min_base.as_ref(),
            &mut usable.d_min_base,
        ),
        (
            "d_max_full_exposure",
            anchors.d_max_full_exposure.as_ref(),
            &mut usable.d_max_full_exposure,
        ),
    ] {
        if let Some(anchor) = anchor {
            if let Some(reason) =
                anchor_mismatch(anchor, expected_domain, profile, raw_decode_version)
            {
                rejected.push(format!("density_anchor_rejected|{name}|{reason}"));
            } else {
                *target = Some(anchor.clone());
            }
        }
    }
    (usable, rejected)
}

fn fallback_resolution(
    input: &PipelineResolverInput,
    requested_path: ProcessingContract,
    reason: String,
) -> PipelineResolution {
    let (anchors, rejected) = filter_anchors(
        &input.density_anchors,
        DataDomain::ProPhotoEstimate,
        None,
        input.raw_decode_version,
    );
    let resolved_path = anchors.prophoto_contract();
    let mut report = PipelineProcessingReport::smart_auto_fallback(reason.clone());
    report.fallback_reasons.extend(rejected.iter().cloned());
    report.stages.push(PipelineStageRecord {
        stage: "capture_capability".to_string(),
        input_domain: DataDomain::RawMosaic,
        output_domain: DataDomain::ProPhotoEstimate,
        status: PipelineStageStatus::Unavailable,
        detail: reason.clone(),
    });
    PipelineResolution {
        requested_path,
        resolved_path,
        resolved_profile_id: None,
        resolved_payload_digest: None,
        capture: capability("capture", PipelineStageStatus::Unavailable, &reason),
        density: capability(
            "density",
            PipelineStageStatus::Default,
            "relative_prophoto_estimate",
        ),
        film: capability(
            "film",
            PipelineStageStatus::Default,
            "generic_positive_render",
        ),
        flat: capability(
            "flat",
            PipelineStageStatus::Unavailable,
            "not_used_in_smart_auto",
        ),
        usable_density_anchors: anchors,
        rejected_anchor_reasons: rejected,
        processing_report: report,
    }
}

/// Resolve the highest currently trustworthy path without mutating stored
/// Roll bindings or image state. Re-running with recovered inputs therefore
/// automatically restores Capture Corrected.
pub fn resolve_pipeline(input: &PipelineResolverInput) -> PipelineResolution {
    if input.persisted_contract == ProcessingContract::LegacyV1 {
        return PipelineResolution {
            requested_path: ProcessingContract::LegacyV1,
            resolved_path: ProcessingContract::LegacyV1,
            resolved_profile_id: None,
            resolved_payload_digest: None,
            capture: capability("capture", PipelineStageStatus::Used, "legacy_v1"),
            density: capability("density", PipelineStageStatus::Used, "legacy_status_m"),
            film: capability("film", PipelineStageStatus::Default, "legacy_render"),
            flat: capability("flat", PipelineStageStatus::Unavailable, "legacy_v1"),
            usable_density_anchors: input.density_anchors.clone(),
            rejected_anchor_reasons: Vec::new(),
            processing_report: PipelineProcessingReport::default(),
        };
    }

    let requested_path = if input.roll_profile_id.is_some() {
        ProcessingContract::CaptureCorrectedV11
    } else {
        input.density_anchors.prophoto_contract()
    };
    if requested_path != ProcessingContract::CaptureCorrectedV11 {
        let (anchors, rejected) = filter_anchors(
            &input.density_anchors,
            DataDomain::ProPhotoEstimate,
            None,
            input.raw_decode_version,
        );
        let resolved_path = anchors.prophoto_contract();
        let mut report = PipelineProcessingReport::smart_auto();
        report.fallback_reasons.extend(rejected.iter().cloned());
        return PipelineResolution {
            requested_path,
            resolved_path,
            resolved_profile_id: None,
            resolved_payload_digest: None,
            capture: capability("capture", PipelineStageStatus::Default, "smart_auto"),
            density: capability(
                "density",
                PipelineStageStatus::Default,
                "relative_prophoto_estimate",
            ),
            film: capability(
                "film",
                PipelineStageStatus::Default,
                "generic_positive_render",
            ),
            flat: capability("flat", PipelineStageStatus::Unavailable, "not_applicable"),
            usable_density_anchors: anchors,
            rejected_anchor_reasons: rejected,
            processing_report: report,
        };
    }

    let Some(profile) = input
        .profile
        .as_ref()
        .filter(|profile| input.roll_profile_id.as_deref() == Some(profile.profile_id.as_str()))
    else {
        return fallback_resolution(input, requested_path, "profile_missing".to_string());
    };
    if !profile.available {
        return fallback_resolution(input, requested_path, "profile_unavailable".to_string());
    }
    if let Some(reason) = &profile.capture_validation_error {
        return fallback_resolution(
            input,
            requested_path,
            format!("capture_payload_invalid|{reason}"),
        );
    }
    if !profile.has_dark || !profile.has_open_gate {
        return fallback_resolution(
            input,
            requested_path,
            if profile.has_dark {
                "open_gate_reference_missing".to_string()
            } else {
                "dark_reference_missing".to_string()
            },
        );
    }
    match input.image_kind {
        PipelineImageKind::RawBayer => {}
        PipelineImageKind::RawXTrans => {
            return fallback_resolution(input, requested_path, "xtrans_not_supported".to_string())
        }
        PipelineImageKind::DirectRgb => {
            return fallback_resolution(
                input,
                requested_path,
                "capture_requires_raw_bayer".to_string(),
            )
        }
        PipelineImageKind::Missing => {
            return fallback_resolution(input, requested_path, "source_missing".to_string())
        }
        PipelineImageKind::Unsupported => {
            return fallback_resolution(input, requested_path, "source_unsupported".to_string())
        }
    }
    if let Some(reason) = &input.runtime_failure {
        return fallback_resolution(
            input,
            requested_path,
            format!("capture_runtime_failure|{reason}"),
        );
    }

    let (anchors, rejected) = filter_anchors(
        &input.density_anchors,
        DataDomain::RelativeTransmissionRgb,
        Some(profile),
        input.raw_decode_version,
    );
    let mut report = PipelineProcessingReport::capture_corrected(profile.has_flat);
    report.fallback_reasons.extend(rejected.iter().cloned());
    PipelineResolution {
        requested_path,
        resolved_path: ProcessingContract::CaptureCorrectedV11,
        resolved_profile_id: Some(profile.profile_id.clone()),
        resolved_payload_digest: Some(profile.payload_digest.clone()),
        capture: capability(
            "capture",
            PipelineStageStatus::Used,
            "dark_open_homogeneous_correction",
        ),
        density: capability(
            "density",
            PipelineStageStatus::Default,
            "relative_transmission_no_measured_density",
        ),
        film: capability(
            "film",
            PipelineStageStatus::Default,
            "generic_positive_render",
        ),
        flat: capability(
            "flat",
            if profile.has_flat {
                PipelineStageStatus::Default
            } else {
                PipelineStageStatus::Unavailable
            },
            if profile.has_flat {
                "stored_not_applied_pending_independent_definition"
            } else {
                "optional_flat_not_available"
            },
        ),
        usable_density_anchors: anchors,
        rejected_anchor_reasons: rejected,
        processing_report: report,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::{
        DensityAnchorConfidence, DensityAnchorProvenance, DensityAnchorScope, DensityAnchorSource,
    };

    fn anchor(domain: DataDomain, profile: Option<&ResolverProfile>) -> DensityAnchor {
        DensityAnchor {
            density: [0.1, 0.2, 0.3],
            source: DensityAnchorSource::SampledFilmBase,
            scope: DensityAnchorScope::Roll,
            confidence: DensityAnchorConfidence::UserSampled,
            reference_id: Some("reference.raw".to_string()),
            provenance: DensityAnchorProvenance {
                input_domain: domain,
                calibration_profile_id: profile.map(|profile| profile.profile_id.clone()),
                calibration_payload_digest: profile.map(|profile| profile.payload_digest.clone()),
                raw_decode_version: Some(9),
                algorithm_version: DENSITY_ANCHOR_ALGORITHM_VERSION.to_string(),
                legacy: false,
            },
        }
    }

    fn profile() -> ResolverProfile {
        ResolverProfile {
            profile_id: "profile-a".to_string(),
            payload_digest: "payload-a".to_string(),
            available: true,
            capture_validation_error: None,
            has_dark: true,
            has_open_gate: true,
            has_flat: false,
        }
    }

    fn input(profile: Option<ResolverProfile>) -> PipelineResolverInput {
        PipelineResolverInput {
            persisted_contract: ProcessingContract::SmartAutoProPhotoV11,
            roll_profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
            profile,
            image_kind: PipelineImageKind::RawBayer,
            density_anchors: DensityAnchors::default(),
            raw_decode_version: 9,
            runtime_failure: None,
        }
    }

    #[test]
    fn prophoto_anchor_never_enters_capture_corrected() {
        let profile = profile();
        let mut input = input(Some(profile.clone()));
        input.density_anchors.d_min_base = Some(anchor(DataDomain::ProPhotoEstimate, None));
        let result = resolve_pipeline(&input);
        assert_eq!(
            result.resolved_path,
            ProcessingContract::CaptureCorrectedV11
        );
        assert!(result.usable_density_anchors.d_min_base.is_none());
        assert!(result.rejected_anchor_reasons[0].contains("anchor_input_domain_mismatch"));
    }

    #[test]
    fn same_profile_and_domain_anchor_is_usable() {
        let profile = profile();
        let mut input = input(Some(profile.clone()));
        input.density_anchors.d_min_base =
            Some(anchor(DataDomain::RelativeTransmissionRgb, Some(&profile)));
        let result = resolve_pipeline(&input);
        assert!(result.usable_density_anchors.d_min_base.is_some());
        assert!(result.rejected_anchor_reasons.is_empty());
    }

    #[test]
    fn no_profile_missing_reference_and_xtrans_fall_back() {
        let no_profile = resolve_pipeline(&input(None));
        assert_eq!(
            no_profile.resolved_path,
            ProcessingContract::SmartAutoProPhotoV11
        );

        let mut missing = profile();
        missing.has_open_gate = false;
        let missing = resolve_pipeline(&input(Some(missing)));
        assert!(
            missing.processing_report.fallback_reasons[0].contains("open_gate_reference_missing")
        );

        let mut xtrans = input(Some(profile()));
        xtrans.image_kind = PipelineImageKind::RawXTrans;
        assert!(
            resolve_pipeline(&xtrans).processing_report.fallback_reasons[0]
                .contains("xtrans_not_supported")
        );
    }

    #[test]
    fn flat_is_optional_and_reported_as_not_applied() {
        let result = resolve_pipeline(&input(Some(profile())));
        assert_eq!(
            result.resolved_path,
            ProcessingContract::CaptureCorrectedV11
        );
        assert_eq!(result.flat.status, PipelineStageStatus::Unavailable);
        assert_eq!(result.flat.detail, "optional_flat_not_available");
    }

    #[test]
    fn runtime_failure_is_non_sticky_and_recovers() {
        let original = input(Some(profile()));
        let mut failed = original.clone();
        failed.runtime_failure = Some("temporary_decode_error".to_string());
        assert_ne!(
            resolve_pipeline(&failed).resolved_path,
            ProcessingContract::CaptureCorrectedV11
        );
        assert_eq!(
            resolve_pipeline(&original).resolved_path,
            ProcessingContract::CaptureCorrectedV11
        );
        assert_eq!(original.roll_profile_id.as_deref(), Some("profile-a"));
    }

    #[test]
    fn payload_digest_failure_falls_back_and_recovery_restores_capture() {
        let valid = input(Some(profile()));
        let mut changed_profile = profile();
        changed_profile.capture_validation_error =
            Some("capture_payload_digest_mismatch".to_string());
        let changed = input(Some(changed_profile));

        let fallback = resolve_pipeline(&changed);
        assert_eq!(
            fallback.requested_path,
            ProcessingContract::CaptureCorrectedV11
        );
        assert_eq!(
            fallback.resolved_path,
            ProcessingContract::SmartAutoProPhotoV11
        );
        assert!(fallback.processing_report.fallback_reasons[0]
            .contains("capture_payload_digest_mismatch"));
        assert_eq!(
            resolve_pipeline(&valid).resolved_path,
            ProcessingContract::CaptureCorrectedV11
        );
    }
}
