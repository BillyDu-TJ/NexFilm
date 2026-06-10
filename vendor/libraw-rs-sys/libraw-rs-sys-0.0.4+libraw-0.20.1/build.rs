use std::env;
use std::path::Path;

fn main() {
    let out_dir_ = env::var_os("OUT_DIR").unwrap();
    let out_dir = Path::new(&out_dir_);

    build(&out_dir);
    #[cfg(feature = "bindgen")]
    bindings(&out_dir);
}

fn build(out_dir: &Path) {
    let mut libraw = cc::Build::new();
    libraw.cpp(true);
    libraw.include("libraw/");

    libraw.file("libraw/src/decoders/canon_600.cpp");
    libraw.file("libraw/src/decoders/crx.cpp");
    libraw.file("libraw/src/decoders/decoders_dcraw.cpp");
    libraw.file("libraw/src/decoders/decoders_libraw.cpp");
    libraw.file("libraw/src/decoders/decoders_libraw_dcrdefs.cpp");
    libraw.file("libraw/src/decoders/dng.cpp");
    libraw.file("libraw/src/decoders/fp_dng.cpp");
    libraw.file("libraw/src/decoders/fuji_compressed.cpp");
    libraw.file("libraw/src/decoders/generic.cpp");
    libraw.file("libraw/src/decoders/kodak_decoders.cpp");
    libraw.file("libraw/src/decoders/load_mfbacks.cpp");
    libraw.file("libraw/src/decoders/smal.cpp");
    libraw.file("libraw/src/decoders/unpack.cpp");
    libraw.file("libraw/src/decoders/unpack_thumb.cpp");
    libraw.file("libraw/src/demosaic/aahd_demosaic.cpp");
    libraw.file("libraw/src/demosaic/ahd_demosaic.cpp");
    libraw.file("libraw/src/demosaic/dcb_demosaic.cpp");
    libraw.file("libraw/src/demosaic/dht_demosaic.cpp");
    libraw.file("libraw/src/demosaic/misc_demosaic.cpp");
    libraw.file("libraw/src/demosaic/xtrans_demosaic.cpp");
    libraw.file("libraw/src/integration/dngsdk_glue.cpp");
    libraw.file("libraw/src/integration/rawspeed_glue.cpp");
    libraw.file("libraw/src/metadata/adobepano.cpp");
    libraw.file("libraw/src/metadata/canon.cpp");
    libraw.file("libraw/src/metadata/ciff.cpp");
    libraw.file("libraw/src/metadata/cr3_parser.cpp");
    libraw.file("libraw/src/metadata/epson.cpp");
    libraw.file("libraw/src/metadata/exif_gps.cpp");
    libraw.file("libraw/src/metadata/fuji.cpp");
    libraw.file("libraw/src/metadata/hasselblad_model.cpp");
    libraw.file("libraw/src/metadata/identify.cpp");
    libraw.file("libraw/src/metadata/identify_tools.cpp");
    libraw.file("libraw/src/metadata/kodak.cpp");
    libraw.file("libraw/src/metadata/leica.cpp");
    libraw.file("libraw/src/metadata/makernotes.cpp");
    libraw.file("libraw/src/metadata/mediumformat.cpp");
    libraw.file("libraw/src/metadata/minolta.cpp");
    libraw.file("libraw/src/metadata/misc_parsers.cpp");
    libraw.file("libraw/src/metadata/nikon.cpp");
    libraw.file("libraw/src/metadata/normalize_model.cpp");
    libraw.file("libraw/src/metadata/olympus.cpp");
    libraw.file("libraw/src/metadata/p1.cpp");
    libraw.file("libraw/src/metadata/pentax.cpp");
    libraw.file("libraw/src/metadata/samsung.cpp");
    libraw.file("libraw/src/metadata/sony.cpp");
    libraw.file("libraw/src/metadata/tiff.cpp");
    libraw.file("libraw/src/postprocessing/aspect_ratio.cpp");
    libraw.file("libraw/src/postprocessing/dcraw_process.cpp");
    libraw.file("libraw/src/postprocessing/mem_image.cpp");
    libraw.file("libraw/src/postprocessing/postprocessing_aux.cpp");
    //libraw.file("libraw/src/postprocessing/postprocessing_ph.cpp");
    libraw.file("libraw/src/postprocessing/postprocessing_utils.cpp");
    libraw.file("libraw/src/postprocessing/postprocessing_utils_dcrdefs.cpp");
    libraw.file("libraw/src/preprocessing/ext_preprocess.cpp");
    //libraw.file("libraw/src/preprocessing/preprocessing_ph.cpp");
    libraw.file("libraw/src/preprocessing/raw2image.cpp");
    libraw.file("libraw/src/preprocessing/subtract_black.cpp");
    libraw.file("libraw/src/tables/cameralist.cpp");
    libraw.file("libraw/src/tables/colorconst.cpp");
    libraw.file("libraw/src/tables/colordata.cpp");
    libraw.file("libraw/src/tables/wblists.cpp");
    libraw.file("libraw/src/utils/curves.cpp");
    libraw.file("libraw/src/utils/decoder_info.cpp");
    libraw.file("libraw/src/utils/init_close_utils.cpp");
    libraw.file("libraw/src/utils/open.cpp");
    libraw.file("libraw/src/utils/phaseone_processing.cpp");
    libraw.file("libraw/src/utils/read_utils.cpp");
    libraw.file("libraw/src/utils/thumb_utils.cpp");
    libraw.file("libraw/src/utils/utils_dcraw.cpp");
    libraw.file("libraw/src/utils/utils_libraw.cpp");
    libraw.file("libraw/src/write/apply_profile.cpp");
    libraw.file("libraw/src/write/file_write.cpp");
    libraw.file("libraw/src/write/tiff_writer.cpp");
    //libraw.file("libraw/src/write/write_ph.cpp");
    libraw.file("libraw/src/x3f/x3f_parse_process.cpp");
    libraw.file("libraw/src/x3f/x3f_utils_patched.cpp");
    libraw.file("libraw/src/libraw_c_api.cpp");
    // libraw.file("libraw/src/libraw_cxx.cpp");
    libraw.file("libraw/src/libraw_datastream.cpp");

    libraw.warnings(false);
    libraw.extra_warnings(false);
    // do I really have to supress all of these?
    libraw.flag_if_supported("-Wno-format-truncation");
    libraw.flag_if_supported("-Wno-unused-result");
    libraw.flag_if_supported("-Wno-format-overflow");
    // thread safety
    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("msvc") {
        libraw.flag("-pthread");
        libraw.static_flag(true);
    } else {
        libraw.define("LIBRAW_NODLL", None);
    }
    libraw.compile("raw");

    println!(
        "cargo:rustc-link-search=native={}",
        out_dir.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=raw");
}

#[cfg(feature = "bindgen")]
fn bindings(out_dir: &Path) {
    let bindings = bindgen::Builder::default()
        .header("libraw/libraw/libraw.h")
        .use_core()
        .ctypes_prefix("libc")
        .generate_comments(true)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        // API improvements
        .derive_eq(true)
        .size_t_is_usize(true)
        // these are never part of the API
        .blacklist_function("_.*")
        // consts creating duplications
        .blacklist_item("FP_NAN")
        .blacklist_item("FP_INFINITE")
        .blacklist_item("FP_ZERO")
        .blacklist_item("FP_SUBNORMAL")
        .blacklist_item("FP_NORMAL")
        // Rust doesn't support long double, and bindgen can't skip it
        // https://github.com/rust-lang/rust-bindgen/issues/1549
        .blacklist_function("acoshl")
        .blacklist_function("acosl")
        .blacklist_function("asinhl")
        .blacklist_function("asinl")
        .blacklist_function("atan2l")
        .blacklist_function("atanhl")
        .blacklist_function("atanl")
        .blacklist_function("cbrtl")
        .blacklist_function("ceill")
        .blacklist_function("copysignl")
        .blacklist_function("coshl")
        .blacklist_function("cosl")
        .blacklist_function("dreml")
        .blacklist_function("ecvt_r")
        .blacklist_function("erfcl")
        .blacklist_function("erfl")
        .blacklist_function("exp2l")
        .blacklist_function("expl")
        .blacklist_function("expm1l")
        .blacklist_function("fabsl")
        .blacklist_function("fcvt_r")
        .blacklist_function("fdiml")
        .blacklist_function("finitel")
        .blacklist_function("floorl")
        .blacklist_function("fmal")
        .blacklist_function("fmaxl")
        .blacklist_function("fminl")
        .blacklist_function("fmodl")
        .blacklist_function("frexpl")
        .blacklist_function("gammal")
        .blacklist_function("hypotl")
        .blacklist_function("ilogbl")
        .blacklist_function("isinfl")
        .blacklist_function("isnanl")
        .blacklist_function("j0l")
        .blacklist_function("j1l")
        .blacklist_function("jnl")
        .blacklist_function("ldexpl")
        .blacklist_function("lgammal")
        .blacklist_function("lgammal_r")
        .blacklist_function("llrintl")
        .blacklist_function("llroundl")
        .blacklist_function("log10l")
        .blacklist_function("log1pl")
        .blacklist_function("log2l")
        .blacklist_function("logbl")
        .blacklist_function("logl")
        .blacklist_function("lrintl")
        .blacklist_function("lroundl")
        .blacklist_function("modfl")
        .blacklist_function("nanl")
        .blacklist_function("nearbyintl")
        .blacklist_function("nextafterl")
        .blacklist_function("nexttoward")
        .blacklist_function("nexttowardf")
        .blacklist_function("nexttowardl")
        .blacklist_function("powl")
        .blacklist_function("qecvt")
        .blacklist_function("qecvt_r")
        .blacklist_function("qfcvt")
        .blacklist_function("qfcvt_r")
        .blacklist_function("qgcvt")
        .blacklist_function("remainderl")
        .blacklist_function("remquol")
        .blacklist_function("rintl")
        .blacklist_function("roundl")
        .blacklist_function("scalbl")
        .blacklist_function("scalblnl")
        .blacklist_function("scalbnl")
        .blacklist_function("significandl")
        .blacklist_function("sinhl")
        .blacklist_function("sinl")
        .blacklist_function("sqrtl")
        .blacklist_function("strtold")
        .blacklist_function("tanhl")
        .blacklist_function("tanl")
        .blacklist_function("tgammal")
        .blacklist_function("truncl")
        .blacklist_function("y0l")
        .blacklist_function("y1l")
        .blacklist_function("ynl")
        .generate()
        .expect("Unable to generate bindings");

    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
